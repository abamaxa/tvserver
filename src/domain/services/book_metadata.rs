use crate::domain::{
    algorithm::title_case,
    config::{get_book_dir, get_book_thumbnail_dir},
    models::{
        ensure_default_book_thumbnail, BookDetails, BookFormat, BookMetadata, BookState,
        DEFAULT_BOOK_THUMBNAIL,
    },
    traits::{FileStorer, Repository},
};
use lopdf::{decode_text_string, Dictionary, Document};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use zip::ZipArchive;

const MAX_EPUB_ARCHIVE_ENTRIES: u16 = 4_096;
const MAX_CENTRAL_DIRECTORY_BYTES: u32 = 8 * 1024 * 1024;
const MAX_EOCD_TAIL_BYTES: u64 = 22 + u16::MAX as u64;
const MAX_CONTAINER_BYTES: u64 = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;
const MAX_COVER_DIMENSION: u32 = 8_192;
const MAX_COVER_PIXELS: u64 = 8_000_000;
const MAX_COVER_DECODE_ALLOC_BYTES: u64 = 48 * 1024 * 1024;
const SVG_FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/book/FiraSans-Regular.ttf"
));
static NEXT_THUMBNAIL_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookMetadataExtraction {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub page_count: Option<i64>,
    pub thumbnail: String,
    pub metadata: BookMetadata,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BookMetadataExtractionError {
    #[error("could not open EPUB: {0}")]
    Open(String),
    #[error("could not read EPUB archive: {0}")]
    Archive(String),
    #[error("invalid EPUB package: {0}")]
    InvalidPackage(String),
    #[error("could not read PDF: {0}")]
    Pdf(String),
}

pub async fn generate_book_metadata(
    path: PathBuf,
    storer: FileStorer,
    repository: Repository,
    suggested_collection: Option<String>,
) -> anyhow::Result<Option<BookDetails>> {
    let book_dir = get_book_dir();
    let book_root = PathBuf::from(&book_dir);
    let thumbnail_root = get_book_thumbnail_dir(&book_dir);
    generate_book_metadata_with_roots(
        path,
        storer,
        repository,
        suggested_collection,
        book_root,
        thumbnail_root,
    )
    .await
}

async fn generate_book_metadata_with_roots(
    path: PathBuf,
    storer: FileStorer,
    repository: Repository,
    suggested_collection: Option<String>,
    book_root: PathBuf,
    thumbnail_root: PathBuf,
) -> anyhow::Result<Option<BookDetails>> {
    let format = book_format(&path)?;
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        anyhow::bail!(
            "book source must be a regular file and not a symlink: {}",
            path.display()
        );
    }
    if is_book_file_being_written(&path).await? {
        tracing::info!(book = %path.display(), "Skipping book file that is still being written");
        return Ok(None);
    }
    if metadata.len() == 0 {
        anyhow::bail!("cannot ingest zero-byte book: {}", path.display());
    }

    let checksum = super::video_metadata::calculate_checksum(&path).await?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("book path has no UTF-8 file name: {}", path.display()))?
        .to_string();
    let collection = match suggested_collection.as_deref() {
        Some(collection) => title_case(collection),
        None => collection_from_source(&path, &book_root)?,
    };
    validate_collection(&collection)?;

    let mut details = BookDetails::new(file_name.clone(), collection.clone(), &path, format);
    details.checksum = checksum;
    details.search_phrase = suggested_collection.clone();

    let extraction_path = path.clone();
    let extraction_thumbnail_root = thumbnail_root.clone();
    let thumbnail_key = checksum.to_string();
    let extraction = tokio::task::spawn_blocking(move || match format {
        BookFormat::Pdf => {
            extract_pdf_metadata(&extraction_path, &extraction_thumbnail_root, &thumbnail_key)
        }
        BookFormat::Epub => {
            extract_epub_metadata(&extraction_path, &extraction_thumbnail_root, &thumbnail_key)
        }
    })
    .await
    .map_err(|error| anyhow::anyhow!("book metadata worker failed: {error}"))?;

    match extraction {
        Ok(extraction) => apply_extraction(&mut details, extraction),
        Err(error) => {
            tracing::warn!(book = %path.display(), "book metadata extraction failed: {error}");
            if let Err(thumbnail_error) = ensure_default_book_thumbnail(&thumbnail_root) {
                tracing::warn!(
                    book = %path.display(),
                    "could not prepare default book thumbnail: {thumbnail_error}"
                );
            }
            details.thumbnail = DEFAULT_BOOK_THUMBNAIL.to_string();
            details.metadata.extraction_error = Some(error.to_string());
            details.state = BookState::MetadataError;
        }
    }

    let destination_directory = book_root.join(&collection);
    let destination = destination_directory.join(&file_name);
    storer.create_folder(&destination_directory).await?;
    if absolute_path(&path)? != absolute_path(&destination)? {
        let source = path.to_str().ok_or_else(|| {
            anyhow::anyhow!("book source path is not valid UTF-8: {}", path.display())
        })?;
        let destination_path = destination.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "book destination path is not valid UTF-8: {}",
                destination.display()
            )
        })?;
        storer.rename(source, destination_path).await?;
    }

    details.collection = collection;
    details.file_name = file_name;
    details.dir_path = None;
    repository.save_book(&details).await?;
    Ok(Some(details))
}

fn apply_extraction(details: &mut BookDetails, extraction: BookMetadataExtraction) {
    if let Some(title) = extraction.title {
        details.title = title;
    }
    details.authors = extraction.authors;
    details.description = extraction.description;
    details.publisher = extraction.publisher;
    details.published_date = extraction.published_date;
    details.language = extraction.language;
    details.isbn = extraction.isbn;
    details.page_count = extraction.page_count;
    details.thumbnail = extraction.thumbnail;
    details.metadata = extraction.metadata;
    details.state = if details.metadata.extraction_error.is_some() {
        BookState::MetadataError
    } else {
        BookState::Ready
    };
}

fn book_format(path: &Path) -> anyhow::Result<BookFormat> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => Ok(BookFormat::Pdf),
        Some("epub") => Ok(BookFormat::Epub),
        _ => anyhow::bail!("unsupported book file: {}", path.display()),
    }
}

fn collection_from_source(path: &Path, book_root: &Path) -> anyhow::Result<String> {
    let Some(parent) = path.parent() else {
        return Ok(String::new());
    };
    let parent = absolute_path(parent)?;
    let book_root = absolute_path(book_root)?;
    match parent.strip_prefix(&book_root) {
        Ok(relative) => relative
            .to_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("book collection path is not valid UTF-8")),
        Err(_) => Ok(parent
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string()),
    }
}

fn validate_collection(collection: &str) -> anyhow::Result<()> {
    if Path::new(collection)
        .components()
        .all(|component| matches!(component, std::path::Component::Normal(_)))
        || collection.is_empty()
    {
        Ok(())
    } else {
        anyhow::bail!("book collection must be a relative path without traversal: {collection}")
    }
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    std::path::absolute(path).map_err(|error| {
        anyhow::anyhow!("could not make path absolute ({}): {error}", path.display())
    })
}

async fn is_book_file_being_written(path: &Path) -> std::io::Result<bool> {
    let initial_size = tokio::fs::metadata(path).await?.len();
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    Ok(tokio::fs::metadata(path).await?.len() != initial_size)
}

pub trait PdfThumbnailRenderer {
    fn render_thumbnail(&self, pdf_path: &Path, thumbnail_path: &Path) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultPdfThumbnailRenderer;

#[cfg(not(feature = "pdf-thumbnails"))]
impl PdfThumbnailRenderer for DefaultPdfThumbnailRenderer {
    fn render_thumbnail(&self, _pdf_path: &Path, _thumbnail_path: &Path) -> Result<(), String> {
        Err("PDF thumbnail rendering is disabled".to_string())
    }
}

#[cfg(feature = "pdf-thumbnails")]
impl PdfThumbnailRenderer for DefaultPdfThumbnailRenderer {
    fn render_thumbnail(&self, pdf_path: &Path, thumbnail_path: &Path) -> Result<(), String> {
        use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

        let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
            .or_else(|_| Pdfium::bind_to_system_library())
            .map_err(|error| format!("could not load Pdfium: {error}"))?;
        let pdfium = Pdfium::new(bindings);
        let document = pdfium
            .load_pdf_from_file(pdf_path, None)
            .map_err(|error| format!("could not load PDF with Pdfium: {error}"))?;
        let page = document
            .pages()
            .first()
            .map_err(|error| format!("could not open first PDF page: {error}"))?;
        let image = page
            .render_with_config(
                &PdfRenderConfig::new()
                    .set_target_width(512)
                    .set_maximum_height(768),
            )
            .map_err(|error| format!("could not render first PDF page: {error}"))?
            .as_image()
            .into_rgb8();
        image
            .save_with_format(thumbnail_path, image::ImageFormat::Jpeg)
            .map_err(|error| format!("could not write PDF thumbnail: {error}"))
    }
}

pub fn extract_pdf_metadata(
    pdf_path: &Path,
    thumbnail_dir: &Path,
    thumbnail_key: &str,
) -> Result<BookMetadataExtraction, BookMetadataExtractionError> {
    extract_pdf_metadata_with_renderer(
        pdf_path,
        thumbnail_dir,
        thumbnail_key,
        &DefaultPdfThumbnailRenderer,
    )
}

pub fn extract_pdf_metadata_with_renderer<R: PdfThumbnailRenderer + ?Sized>(
    pdf_path: &Path,
    thumbnail_dir: &Path,
    thumbnail_key: &str,
    renderer: &R,
) -> Result<BookMetadataExtraction, BookMetadataExtractionError> {
    let document = Document::load(pdf_path)
        .map_err(|error| BookMetadataExtractionError::Pdf(error.to_string()))?;
    let info = pdf_info_dictionary(&document);
    let title = info
        .and_then(|info| pdf_info_text(info, b"Title"))
        .or_else(|| Some(filename_derived_title(pdf_path)));
    let authors = info
        .and_then(|info| pdf_info_text(info, b"Author"))
        .into_iter()
        .collect();
    let description = info.and_then(|info| pdf_info_text(info, b"Subject"));
    let keywords = info.and_then(|info| pdf_info_text(info, b"Keywords"));
    let creation_date = info.and_then(|info| pdf_info_text(info, b"CreationDate"));
    let page_count = i64::try_from(document.get_pages().len()).ok();

    let (thumbnail, warnings) =
        match render_pdf_thumbnail(renderer, pdf_path, thumbnail_dir, thumbnail_key) {
            Ok(thumbnail) => (thumbnail, Vec::new()),
            Err(warning) => default_thumbnail_fallback(pdf_path, thumbnail_dir, warning),
        };

    Ok(BookMetadataExtraction {
        title,
        authors,
        description,
        page_count,
        thumbnail,
        metadata: BookMetadata {
            raw: Some(json!({
                "pdf": {
                    "keywords": keywords,
                    "creationDate": creation_date,
                    "thumbnailWarnings": warnings,
                }
            })),
            extraction_error: None,
        },
        warnings,
        ..Default::default()
    })
}

fn pdf_info_dictionary(document: &Document) -> Option<&Dictionary> {
    document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|info| document.dereference(info).ok())
        .and_then(|(_, info)| info.as_dict().ok())
}

fn pdf_info_text(info: &Dictionary, key: &[u8]) -> Option<String> {
    info.get(key)
        .ok()
        .and_then(|value| decode_text_string(value).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn filename_derived_title(pdf_path: &Path) -> String {
    pdf_path
        .file_stem()
        .unwrap_or_else(|| pdf_path.as_os_str())
        .to_string_lossy()
        .replace(['.', '_'], " ")
}

fn render_pdf_thumbnail<R: PdfThumbnailRenderer + ?Sized>(
    renderer: &R,
    pdf_path: &Path,
    thumbnail_dir: &Path,
    thumbnail_key: &str,
) -> Result<String, String> {
    let thumbnail = thumbnail_filename(thumbnail_key)?;
    fs::create_dir_all(thumbnail_dir)
        .map_err(|error| format!("could not create PDF thumbnail directory: {error}"))?;
    let thumbnail_path = thumbnail_dir.join(&thumbnail);
    renderer
        .render_thumbnail(pdf_path, &thumbnail_path)
        .map_err(|error| format!("could not render PDF thumbnail: {error}"))?;
    Ok(thumbnail)
}

pub fn extract_epub_metadata(
    epub_path: &Path,
    thumbnail_dir: &Path,
    thumbnail_key: &str,
) -> Result<BookMetadataExtraction, BookMetadataExtractionError> {
    let mut file = File::open(epub_path)
        .map_err(|error| BookMetadataExtractionError::Open(error.to_string()))?;
    preflight_zip(&mut file)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| BookMetadataExtractionError::Archive(error.to_string()))?;

    let container =
        read_bounded_entry(&mut archive, "META-INF/container.xml", MAX_CONTAINER_BYTES)?;
    let package_path = parse_container(&container)?;
    let package = read_bounded_entry(&mut archive, &package_path, MAX_PACKAGE_BYTES)?;
    let (parsed, extraction_error) = match parse_package(&package)? {
        PackageParseOutcome::Complete(parsed) => (parsed, None),
        PackageParseOutcome::Malformed { parsed, error } => (parsed, Some(error)),
    };
    let isbn = parsed.isbn();

    let (thumbnail, warnings) = match extraction_error.as_ref() {
        Some(error) => default_thumbnail_fallback(epub_path, thumbnail_dir, error.clone()),
        None => match extract_cover(
            &mut archive,
            &package_path,
            &parsed,
            thumbnail_dir,
            thumbnail_key,
        ) {
            Ok(thumbnail) => (thumbnail, Vec::new()),
            Err(cover_warning) => {
                default_thumbnail_fallback(epub_path, thumbnail_dir, cover_warning)
            }
        },
    };

    Ok(BookMetadataExtraction {
        title: parsed.title,
        authors: parsed.authors,
        description: parsed.description,
        publisher: parsed.publisher,
        published_date: parsed.published_date,
        language: parsed.language,
        isbn,
        page_count: None,
        thumbnail,
        metadata: BookMetadata {
            raw: Some(json!({
                "epub": {
                    "packagePath": package_path,
                    "coverWarnings": warnings,
                }
            })),
            extraction_error,
        },
        warnings,
    })
}

fn default_thumbnail_fallback(
    book_path: &Path,
    thumbnail_dir: &Path,
    warning: String,
) -> (String, Vec<String>) {
    tracing::warn!(book = %book_path.display(), "{warning}");
    let mut warnings = vec![warning];
    if let Err(error) = ensure_default_book_thumbnail(thumbnail_dir) {
        let warning = format!("could not prepare default book thumbnail: {error}");
        tracing::warn!(book = %book_path.display(), "{warning}");
        warnings.push(warning);
    }
    (DEFAULT_BOOK_THUMBNAIL.to_string(), warnings)
}

fn preflight_zip(file: &mut File) -> Result<(), BookMetadataExtractionError> {
    let file_len = file
        .seek(SeekFrom::End(0))
        .map_err(|error| BookMetadataExtractionError::Archive(error.to_string()))?;
    if file_len < 22 {
        return Err(BookMetadataExtractionError::Archive(
            "invalid ZIP: missing end-of-central-directory record".to_string(),
        ));
    }

    let tail_len = file_len.min(MAX_EOCD_TAIL_BYTES);
    file.seek(SeekFrom::End(-(tail_len as i64)))
        .map_err(|error| BookMetadataExtractionError::Archive(error.to_string()))?;
    let mut tail = vec![0; tail_len as usize];
    file.read_exact(&mut tail)
        .map_err(|error| BookMetadataExtractionError::Archive(error.to_string()))?;

    let eocd_index = (0..=tail.len() - 22).rev().find(|index| {
        tail[*index..].starts_with(&0x0605_4b50_u32.to_le_bytes())
            && read_u16(&tail, *index + 20)
                .is_some_and(|comment_len| *index + 22 + usize::from(comment_len) == tail.len())
    });
    let Some(eocd_index) = eocd_index else {
        return Err(BookMetadataExtractionError::Archive(
            "invalid ZIP: missing end-of-central-directory record".to_string(),
        ));
    };

    let disk_number = read_u16(&tail, eocd_index + 4).unwrap();
    let central_directory_disk = read_u16(&tail, eocd_index + 6).unwrap();
    let entries_on_disk = read_u16(&tail, eocd_index + 8).unwrap();
    let total_entries = read_u16(&tail, eocd_index + 10).unwrap();
    let central_directory_size = read_u32(&tail, eocd_index + 12).unwrap();
    let central_directory_offset = read_u32(&tail, eocd_index + 16).unwrap();

    if entries_on_disk == u16::MAX
        || total_entries == u16::MAX
        || central_directory_size == u32::MAX
        || central_directory_offset == u32::MAX
    {
        return Err(BookMetadataExtractionError::Archive(
            "ZIP64 archives are not supported by the bounded EPUB reader".to_string(),
        ));
    }
    if disk_number != 0 || central_directory_disk != 0 || entries_on_disk != total_entries {
        return Err(BookMetadataExtractionError::Archive(
            "multi-disk ZIP archives are not supported".to_string(),
        ));
    }
    if total_entries > MAX_EPUB_ARCHIVE_ENTRIES {
        return Err(BookMetadataExtractionError::Archive(format!(
            "archive entry count {total_entries} exceeds limit {MAX_EPUB_ARCHIVE_ENTRIES}"
        )));
    }
    if central_directory_size > MAX_CENTRAL_DIRECTORY_BYTES {
        return Err(BookMetadataExtractionError::Archive(format!(
            "central directory size {central_directory_size} exceeds limit {MAX_CENTRAL_DIRECTORY_BYTES}"
        )));
    }

    let absolute_eocd_offset = file_len - tail_len + eocd_index as u64;
    let central_directory_end = u64::from(central_directory_offset)
        .checked_add(u64::from(central_directory_size))
        .ok_or_else(|| {
            BookMetadataExtractionError::Archive(
                "invalid ZIP: central directory range overflow".to_string(),
            )
        })?;
    if central_directory_end > absolute_eocd_offset {
        return Err(BookMetadataExtractionError::Archive(
            "invalid ZIP: central directory extends beyond its end record".to_string(),
        ));
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|error| BookMetadataExtractionError::Archive(error.to_string()))?;
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_bounded_entry(
    archive: &mut ZipArchive<File>,
    name: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, BookMetadataExtractionError> {
    let mut entry = archive.by_name(name).map_err(|error| {
        BookMetadataExtractionError::InvalidPackage(format!(
            "missing archive entry {name:?}: {error}"
        ))
    })?;
    if entry.size() > max_bytes {
        return Err(BookMetadataExtractionError::InvalidPackage(format!(
            "archive entry {name:?} exceeds the {max_bytes}-byte limit"
        )));
    }

    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| BookMetadataExtractionError::Archive(error.to_string()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(BookMetadataExtractionError::InvalidPackage(format!(
            "archive entry {name:?} exceeds the {max_bytes}-byte limit"
        )));
    }
    Ok(bytes)
}

fn parse_container(bytes: &[u8]) -> Result<String, BookMetadataExtractionError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut package_path = None;
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut saw_container_root = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = local_name(element.name().as_ref()).to_vec();
                validate_container_element(&stack, &name, &mut saw_container_root)?;
                if name == b"rootfile" && package_path.is_none() {
                    package_path = parse_rootfile_path(&reader, &element)?;
                }
                stack.push(name);
            }
            Ok(Event::Empty(element)) => {
                let name = local_name(element.name().as_ref()).to_vec();
                validate_container_element(&stack, &name, &mut saw_container_root)?;
                if name == b"rootfile" && package_path.is_none() {
                    package_path = parse_rootfile_path(&reader, &element)?;
                }
            }
            Ok(Event::End(element)) => {
                let element_name = element.name();
                let closing = local_name(element_name.as_ref());
                let Some(opening) = stack.pop() else {
                    return Err(BookMetadataExtractionError::InvalidPackage(
                        "invalid META-INF/container.xml: unexpected closing element".to_string(),
                    ));
                };
                if opening != closing {
                    return Err(BookMetadataExtractionError::InvalidPackage(
                        "invalid META-INF/container.xml: mismatched closing element".to_string(),
                    ));
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(BookMetadataExtractionError::InvalidPackage(format!(
                    "invalid META-INF/container.xml: {error}"
                )))
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(BookMetadataExtractionError::InvalidPackage(
            "invalid META-INF/container.xml: unclosed element".to_string(),
        ));
    }
    if !saw_container_root {
        return Err(BookMetadataExtractionError::InvalidPackage(
            "container element must be the document root".to_string(),
        ));
    }
    package_path.ok_or_else(|| {
        BookMetadataExtractionError::InvalidPackage(
            "META-INF/container.xml has no rootfile full-path".to_string(),
        )
    })
}

fn validate_container_element(
    stack: &[Vec<u8>],
    name: &[u8],
    saw_container_root: &mut bool,
) -> Result<(), BookMetadataExtractionError> {
    if stack.is_empty() {
        if *saw_container_root || name != b"container" {
            return Err(BookMetadataExtractionError::InvalidPackage(
                "container element must be the document root".to_string(),
            ));
        }
        *saw_container_root = true;
        return Ok(());
    }
    if name == b"rootfiles" && !(stack.len() == 1 && stack[0] == b"container") {
        return Err(BookMetadataExtractionError::InvalidPackage(
            "rootfiles must be directly inside container".to_string(),
        ));
    }
    if name == b"rootfile"
        && !(stack.len() == 2 && stack[0] == b"container" && stack[1] == b"rootfiles")
    {
        return Err(BookMetadataExtractionError::InvalidPackage(
            "rootfile must be inside container/rootfiles".to_string(),
        ));
    }
    Ok(())
}

fn parse_rootfile_path(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
) -> Result<Option<String>, BookMetadataExtractionError> {
    match attribute(reader, element, b"full-path")? {
        Some(path) => {
            let normalized = normalize_archive_path(&path).ok_or_else(|| {
                BookMetadataExtractionError::InvalidPackage(format!(
                    "unsafe package document path {path:?}"
                ))
            })?;
            Ok(Some(normalized))
        }
        None => Ok(None),
    }
}

#[derive(Default)]
struct ParsedPackage {
    package_root_recognized: bool,
    title: Option<String>,
    authors: Vec<String>,
    description: Option<String>,
    publisher: Option<String>,
    published_date: Option<String>,
    language: Option<String>,
    identifiers: Vec<Identifier>,
    refinements: Vec<IdentifierRefinement>,
    manifest: Vec<ManifestItem>,
    epub2_cover_id: Option<String>,
}

impl ParsedPackage {
    fn isbn(&self) -> Option<String> {
        self.identifiers
            .iter()
            .filter(|identifier| identifier.scheme.as_deref().is_some_and(is_isbn_signal))
            .find_map(|identifier| isbn_value(&identifier.value))
            .or_else(|| {
                self.identifiers
                    .iter()
                    .filter(|identifier| {
                        identifier.id.as_ref().is_some_and(|id| {
                            self.refinements.iter().any(|refinement| {
                                refinement.identifier_id == *id && is_isbn_signal(&refinement.value)
                            })
                        })
                    })
                    .find_map(|identifier| isbn_value(&identifier.value))
            })
            .or_else(|| {
                self.identifiers
                    .iter()
                    .find_map(|identifier| isbn_value(&identifier.value))
            })
    }
}

struct Identifier {
    id: Option<String>,
    scheme: Option<String>,
    value: String,
}

struct IdentifierRefinement {
    identifier_id: String,
    value: String,
}

struct ManifestItem {
    id: Option<String>,
    href: String,
    media_type: Option<String>,
    properties: Vec<String>,
}

enum PackageParseOutcome {
    Complete(ParsedPackage),
    Malformed {
        parsed: ParsedPackage,
        error: String,
    },
}

fn parse_package(bytes: &[u8]) -> Result<PackageParseOutcome, BookMetadataExtractionError> {
    let mut parsed = ParsedPackage::default();
    match parse_package_into(bytes, &mut parsed) {
        Ok(()) => Ok(PackageParseOutcome::Complete(parsed)),
        Err(error) if parsed.package_root_recognized => Ok(PackageParseOutcome::Malformed {
            parsed,
            error: error.to_string(),
        }),
        Err(error) => Err(error),
    }
}

fn parse_package_into(
    bytes: &[u8],
    parsed: &mut ParsedPackage,
) -> Result<(), BookMetadataExtractionError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut saw_package = false;
    let mut in_metadata = false;
    let mut in_manifest = false;
    let mut depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                if depth == 0 {
                    if saw_package || name != b"package" {
                        return Err(BookMetadataExtractionError::InvalidPackage(
                            "package element must be the document root".to_string(),
                        ));
                    }
                    saw_package = true;
                    parsed.package_root_recognized = true;
                }
                depth += 1;
                if name == b"metadata" {
                    in_metadata = true;
                } else if name == b"manifest" {
                    in_manifest = true;
                } else if in_manifest && name == b"item" {
                    parse_manifest_item(&reader, &element, parsed)?;
                } else if in_metadata {
                    match name {
                        b"title" => {
                            set_first(&mut parsed.title, read_element_text(&mut reader, &element)?);
                            depth -= 1;
                        }
                        b"creator" => {
                            push_nonempty(
                                &mut parsed.authors,
                                read_element_text(&mut reader, &element)?,
                            );
                            depth -= 1;
                        }
                        b"description" => {
                            set_first(
                                &mut parsed.description,
                                read_element_text(&mut reader, &element)?,
                            );
                            depth -= 1;
                        }
                        b"publisher" => {
                            set_first(
                                &mut parsed.publisher,
                                read_element_text(&mut reader, &element)?,
                            );
                            depth -= 1;
                        }
                        b"date" => {
                            set_first(
                                &mut parsed.published_date,
                                read_element_text(&mut reader, &element)?,
                            );
                            depth -= 1;
                        }
                        b"language" => {
                            set_first(
                                &mut parsed.language,
                                read_element_text(&mut reader, &element)?,
                            );
                            depth -= 1;
                        }
                        b"identifier" => {
                            let id = attribute(&reader, &element, b"id")?;
                            let scheme = attribute(&reader, &element, b"scheme")?;
                            let value = read_element_text(&mut reader, &element)?;
                            if !value.is_empty() {
                                parsed.identifiers.push(Identifier { id, scheme, value });
                            }
                            depth -= 1;
                        }
                        b"meta" => {
                            let refines = attribute(&reader, &element, b"refines")?;
                            let property = attribute(&reader, &element, b"property")?;
                            if attribute(&reader, &element, b"name")?.as_deref() == Some("cover") {
                                parsed.epub2_cover_id = attribute(&reader, &element, b"content")?;
                            }
                            let value = read_element_text(&mut reader, &element)?;
                            if property.as_deref() == Some("identifier-type") {
                                if let Some(identifier_id) = refines
                                    .and_then(|value| value.strip_prefix('#').map(str::to_string))
                                {
                                    parsed.refinements.push(IdentifierRefinement {
                                        identifier_id,
                                        value,
                                    });
                                }
                            }
                            depth -= 1;
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Empty(element)) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                if depth == 0 && !saw_package && name == b"package" {
                    saw_package = true;
                    parsed.package_root_recognized = true;
                } else if depth == 0 {
                    return Err(BookMetadataExtractionError::InvalidPackage(
                        "package element must be the document root".to_string(),
                    ));
                } else if in_manifest && name == b"item" {
                    parse_manifest_item(&reader, &element, parsed)?;
                } else if in_metadata && name == b"meta" {
                    if attribute(&reader, &element, b"name")?.as_deref() == Some("cover") {
                        parsed.epub2_cover_id = attribute(&reader, &element, b"content")?;
                    }
                }
            }
            Ok(Event::End(element)) => {
                if depth == 0 {
                    return Err(BookMetadataExtractionError::InvalidPackage(
                        "invalid package document XML: unexpected closing element".to_string(),
                    ));
                }
                depth -= 1;
                if local_name(element.name().as_ref()) == b"metadata" {
                    in_metadata = false;
                } else if local_name(element.name().as_ref()) == b"manifest" {
                    in_manifest = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(BookMetadataExtractionError::InvalidPackage(format!(
                    "invalid package document XML: {error}"
                )))
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(BookMetadataExtractionError::InvalidPackage(
            "invalid package document XML: unclosed element".to_string(),
        ));
    }
    if !saw_package {
        return Err(BookMetadataExtractionError::InvalidPackage(
            "package document has no package element".to_string(),
        ));
    }
    Ok(())
}

fn parse_manifest_item(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    parsed: &mut ParsedPackage,
) -> Result<(), BookMetadataExtractionError> {
    let Some(href) = attribute(reader, element, b"href")? else {
        return Ok(());
    };
    let properties = attribute(reader, element, b"properties")?
        .unwrap_or_default()
        .split_ascii_whitespace()
        .map(str::to_string)
        .collect();
    parsed.manifest.push(ManifestItem {
        id: attribute(reader, element, b"id")?,
        href,
        media_type: attribute(reader, element, b"media-type")?,
        properties,
    });
    Ok(())
}

fn extract_cover(
    archive: &mut ZipArchive<File>,
    package_path: &str,
    parsed: &ParsedPackage,
    thumbnail_dir: &Path,
    thumbnail_key: &str,
) -> Result<String, String> {
    let cover = parsed
        .manifest
        .iter()
        .find(|item| {
            item.properties
                .iter()
                .any(|property| property == "cover-image")
        })
        .or_else(|| {
            parsed.epub2_cover_id.as_ref().and_then(|cover_id| {
                parsed
                    .manifest
                    .iter()
                    .find(|item| item.id.as_deref() == Some(cover_id))
            })
        })
        .or_else(|| {
            parsed.manifest.iter().find(|item| {
                is_image_manifest_item(item)
                    && (item.id.as_deref().is_some_and(is_cover_name)
                        || Path::new(&item.href)
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .is_some_and(is_cover_name))
            })
        })
        .ok_or_else(|| "EPUB does not declare a usable cover image".to_string())?;

    if !is_image_manifest_item(cover) {
        return Err(format!(
            "EPUB cover manifest item {:?} has unsupported media type {:?}",
            cover.href, cover.media_type
        ));
    }
    let cover_path = resolve_opf_relative_path(package_path, &cover.href)
        .ok_or_else(|| format!("EPUB cover has unsafe archive path {:?}", cover.href))?;
    let bytes = read_bounded_entry(archive, &cover_path, MAX_COVER_BYTES)
        .map_err(|error| format!("could not read EPUB cover {cover_path:?}: {error}"))?;
    let decoded = decode_cover(&bytes, cover.media_type.as_deref())
        .map_err(|error| format!("could not decode EPUB cover {cover_path:?}: {error}"))?;
    let thumbnail_name = thumbnail_filename(thumbnail_key)?;

    fs::create_dir_all(thumbnail_dir)
        .map_err(|error| format!("could not create EPUB thumbnail directory: {error}"))?;
    let thumbnail_path = thumbnail_dir.join(&thumbnail_name);
    write_jpeg_atomically(&thumbnail_path, &decoded)
        .map_err(|error| format!("could not write EPUB cover thumbnail: {error}"))?;
    Ok(thumbnail_name)
}

fn decode_cover(bytes: &[u8], media_type: Option<&str>) -> Result<image::DynamicImage, String> {
    if media_type.is_some_and(|media_type| media_type.eq_ignore_ascii_case("image/svg+xml")) {
        return decode_svg_cover(bytes);
    }

    decode_raster_cover(bytes)
}

fn decode_raster_cover(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_COVER_DIMENSION);
    limits.max_image_height = Some(MAX_COVER_DIMENSION);
    limits.max_alloc = Some(MAX_COVER_DECODE_ALLOC_BYTES);

    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    reader.limits(limits);
    let decoded = reader.decode().map_err(|error| error.to_string())?;
    validate_cover_dimensions(decoded.width(), decoded.height())?;
    Ok(decoded)
}

fn decode_svg_cover(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    let svg = std::str::from_utf8(bytes)
        .map_err(|_| "SVG cover must be uncompressed UTF-8 XML".to_owned())?;
    let external_reference_rejected = Arc::new(AtomicBool::new(false));
    let embedded_reference_rejected = Arc::new(AtomicBool::new(false));
    let mut options = resvg::usvg::Options::default();
    options.font_family = "Fira Sans".to_owned();
    let fontdb = options.fontdb_mut();
    fontdb.load_font_data(SVG_FONT_BYTES.to_vec());
    fontdb.set_serif_family("Fira Sans");
    fontdb.set_sans_serif_family("Fira Sans");
    fontdb.set_cursive_family("Fira Sans");
    fontdb.set_fantasy_family("Fira Sans");
    fontdb.set_monospace_family("Fira Sans");
    options.image_href_resolver = resvg::usvg::ImageHrefResolver {
        resolve_data: {
            let embedded_reference_rejected = Arc::clone(&embedded_reference_rejected);
            Box::new(move |_, data, _| {
                let within_byte_budget =
                    u64::try_from(data.len()).is_ok_and(|length| length <= MAX_COVER_BYTES);
                if !within_byte_budget || decode_raster_cover(data.as_slice()).is_err() {
                    embedded_reference_rejected.store(true, Ordering::Relaxed);
                    return None;
                }

                match image::guess_format(data.as_slice()) {
                    Ok(image::ImageFormat::Gif) => Some(resvg::usvg::ImageKind::GIF(data)),
                    Ok(image::ImageFormat::Jpeg) => Some(resvg::usvg::ImageKind::JPEG(data)),
                    Ok(image::ImageFormat::Png) => Some(resvg::usvg::ImageKind::PNG(data)),
                    Ok(image::ImageFormat::WebP) => Some(resvg::usvg::ImageKind::WEBP(data)),
                    _ => {
                        embedded_reference_rejected.store(true, Ordering::Relaxed);
                        None
                    }
                }
            })
        },
        resolve_string: {
            let external_reference_rejected = Arc::clone(&external_reference_rejected);
            Box::new(move |_, _| {
                external_reference_rejected.store(true, Ordering::Relaxed);
                None
            })
        },
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|error| format!("invalid SVG cover: {error}"))?;
    if external_reference_rejected.load(Ordering::Relaxed) {
        return Err("SVG cover contains an external image reference".to_owned());
    }
    if embedded_reference_rejected.load(Ordering::Relaxed) {
        return Err("SVG cover contains an unsupported or oversized embedded image".to_owned());
    }
    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());

    validate_cover_dimensions(width, height)?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "SVG cover is too large to render".to_owned())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );

    let image = image::RgbaImage::from_raw(width, height, pixmap.take())
        .ok_or_else(|| "SVG renderer returned an invalid pixel buffer".to_owned())?;
    Ok(image::DynamicImage::ImageRgba8(image))
}

fn validate_cover_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width > MAX_COVER_DIMENSION || height > MAX_COVER_DIMENSION {
        return Err(format!(
            "image dimensions {width}x{height} exceed limit {MAX_COVER_DIMENSION}x{MAX_COVER_DIMENSION}"
        ));
    }
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels > MAX_COVER_PIXELS {
        return Err(format!(
            "image pixel count {pixels} exceeds limit {MAX_COVER_PIXELS}"
        ));
    }
    Ok(())
}

fn is_image_manifest_item(item: &ManifestItem) -> bool {
    const SUPPORTED_COVER_MEDIA_TYPES: &[&str] =
        &["image/gif", "image/jpeg", "image/jpg", "image/png", "image/svg+xml", "image/webp"];

    item.media_type.as_deref().is_some_and(|media_type| {
        SUPPORTED_COVER_MEDIA_TYPES
            .iter()
            .any(|supported| media_type.eq_ignore_ascii_case(supported))
    })
}

fn is_cover_name(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "cover" | "cover-image" | "coverimage" | "front-cover" | "front_cover"
    )
}

fn resolve_opf_relative_path(package_path: &str, href: &str) -> Option<String> {
    let href = href.split(['#', '?']).next()?;
    let decoded = urlencoding::decode(href).ok()?;
    if decoded.starts_with('/') || decoded.contains('\\') {
        return None;
    }
    let package_parent = package_path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    let joined = if package_parent.is_empty() {
        decoded.into_owned()
    } else {
        format!("{package_parent}/{decoded}")
    };
    normalize_archive_path(&joined)
}

fn thumbnail_filename(key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("book thumbnail key is empty or unsafe".to_string());
    }
    let filename = format!("{key}.jpg");
    if filename.eq_ignore_ascii_case(DEFAULT_BOOK_THUMBNAIL) {
        return Err(format!(
            "book thumbnail key is reserved for {DEFAULT_BOOK_THUMBNAIL}"
        ));
    }
    Ok(filename)
}

fn write_jpeg_atomically(path: &Path, image: &image::DynamicImage) -> Result<(), String> {
    let (temp_path, file) = create_thumbnail_temp_file(path)?;
    let result = (|| {
        let mut writer = BufWriter::new(file);
        {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 85);
            encoder
                .encode_image(&image.to_rgb8())
                .map_err(|error| format!("could not encode temporary thumbnail: {error}"))?;
        }
        writer
            .flush()
            .map_err(|error| format!("could not flush temporary thumbnail: {error}"))?;
        let file = writer.into_inner().map_err(|error| {
            format!("could not close temporary thumbnail: {}", error.into_error())
        })?;
        file.sync_all()
            .map_err(|error| format!("could not sync temporary thumbnail: {error}"))?;
        drop(file);
        fs::rename(&temp_path, path)
            .map_err(|error| format!("could not rename temporary thumbnail: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn create_thumbnail_temp_file(path: &Path) -> Result<(PathBuf, File), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "thumbnail path has no parent directory".to_string())?;
    let filename = path
        .file_name()
        .and_then(|filename| filename.to_str())
        .ok_or_else(|| "thumbnail filename is not valid UTF-8".to_string())?;
    for _ in 0..16 {
        let sequence = NEXT_THUMBNAIL_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(".{filename}.{}.{}.tmp", std::process::id(), sequence));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("could not create temporary thumbnail: {error}"));
            }
        }
    }
    Err("could not allocate a unique temporary thumbnail filename".to_string())
}

fn read_element_text(
    reader: &mut Reader<&[u8]>,
    element: &BytesStart<'_>,
) -> Result<String, BookMetadataExtractionError> {
    reader
        .read_text(element.name())
        .and_then(|text| {
            quick_xml::escape::unescape(&text)
                .map(|text| text.trim().to_string())
                .map_err(quick_xml::Error::from)
        })
        .map_err(|error| {
            BookMetadataExtractionError::InvalidPackage(format!(
                "invalid package document XML: {error}"
            ))
        })
}

fn attribute(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    wanted: &[u8],
) -> Result<Option<String>, BookMetadataExtractionError> {
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| {
            BookMetadataExtractionError::InvalidPackage(format!("invalid XML attribute: {error}"))
        })?;
        if local_name(attribute.key.as_ref()) == wanted {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(|error| {
                    BookMetadataExtractionError::InvalidPackage(format!(
                        "invalid XML attribute value: {error}"
                    ))
                });
        }
    }
    Ok(None)
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn normalize_archive_path(path: &str) -> Option<String> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part if part.contains(':') || part.contains('\0') => return None,
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn set_first(slot: &mut Option<String>, value: String) {
    if slot.is_none() && !value.is_empty() {
        *slot = Some(value);
    }
}

fn push_nonempty(values: &mut Vec<String>, value: String) {
    if !value.is_empty() {
        values.push(value);
    }
}

fn is_isbn_signal(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "isbn" | "15")
        || value.to_ascii_lowercase().contains("isbn")
}

fn isbn_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    let without_prefix = if lowercase.starts_with("urn:isbn:") {
        &trimmed["urn:isbn:".len()..]
    } else if lowercase.starts_with("isbn:") {
        &trimmed["isbn:".len()..]
    } else {
        trimmed
    }
    .trim();
    let compact: String = without_prefix
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-')
        .collect();
    let valid = matches!(compact.len(), 10 | 13)
        && compact.chars().enumerate().all(|(index, character)| {
            character.is_ascii_digit()
                || (compact.len() == 10 && index == 9 && matches!(character, 'x' | 'X'))
        });
    valid.then(|| without_prefix.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use lopdf::{dictionary, text_string, Document, Object};
    use std::{
        fs::{self, File},
        io::Write,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    };
    use zip::{write::SimpleFileOptions, ZipWriter};

    use crate::{
        adaptors::{FileSystemStore, SqlRepository},
        domain::{
            models::{BookFormat, BookState},
            traits::{FileStorer, Repository},
        },
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("tvserver-epub-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_epub(path: &Path, package_document: &str, extra_entries: &[(&str, &[u8])]) {
        write_epub_at(path, "OPS/package.opf", package_document, extra_entries);
    }

    fn write_epub_at(
        path: &Path,
        package_path: &str,
        package_document: &str,
        extra_entries: &[(&str, &[u8])],
    ) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(
            format!(r#"<?xml version="1.0"?>
                <container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
                  <rootfiles><rootfile full-path="{package_path}" media-type="application/oebps-package+xml"/></rootfiles>
                </container>"#).as_bytes(),
        )
        .unwrap();
        zip.start_file(package_path, options).unwrap();
        zip.write_all(package_document.as_bytes()).unwrap();
        for (name, contents) in extra_entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();
    }

    struct FailingPdfRenderer;

    impl PdfThumbnailRenderer for FailingPdfRenderer {
        fn render_thumbnail(&self, _pdf_path: &Path, _thumbnail_path: &Path) -> Result<(), String> {
            Err("test renderer unavailable".to_string())
        }
    }

    fn write_pdf(path: &Path, info: Option<lopdf::Dictionary>, page_count: usize) {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_ids = (0..page_count)
            .map(|_| {
                document.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                })
            })
            .collect::<Vec<_>>();
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => page_count as i64,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        if let Some(info) = info {
            let info_id = document.add_object(info);
            document.trailer.set("Info", info_id);
        }
        document.save(path).unwrap();
    }

    #[test]
    fn extracts_pdf_info_metadata_and_page_count_without_pdfium() {
        let temp = TestDir::new();
        let pdf_path = temp.path().join("metadata.pdf");
        write_pdf(
            &pdf_path,
            Some(dictionary! {
                "Title" => text_string("The PDF Title"),
                "Author" => text_string("Ada Author"),
                "Subject" => text_string("A useful PDF"),
                "Keywords" => text_string("rust, books"),
                "CreationDate" => text_string("D:20260714091500+02'00'"),
            }),
            2,
        );

        let result = extract_pdf_metadata_with_renderer(
            &pdf_path,
            &temp.path().join("covers"),
            "pdf-42",
            &FailingPdfRenderer,
        )
        .expect("valid PDF metadata should extract without Pdfium");

        assert_eq!(result.title.as_deref(), Some("The PDF Title"));
        assert_eq!(result.authors, ["Ada Author"]);
        assert_eq!(result.description.as_deref(), Some("A useful PDF"));
        assert_eq!(result.page_count, Some(2));
        assert_eq!(
            result.metadata.raw.as_ref().unwrap()["pdf"]["keywords"],
            "rust, books"
        );
        assert_eq!(
            result.metadata.raw.as_ref().unwrap()["pdf"]["creationDate"],
            "D:20260714091500+02'00'"
        );
    }

    #[test]
    fn pdf_without_info_uses_filename_title_and_empty_authors() {
        let temp = TestDir::new();
        let pdf_path = temp.path().join("the.hidden_library.pdf");
        write_pdf(&pdf_path, None, 1);

        let result = extract_pdf_metadata_with_renderer(
            &pdf_path,
            &temp.path().join("covers"),
            "pdf-fallback",
            &FailingPdfRenderer,
        )
        .unwrap();

        assert_eq!(result.title.as_deref(), Some("the hidden library"));
        assert!(result.authors.is_empty());
        assert_eq!(result.description, None);
        assert_eq!(result.page_count, Some(1));
    }

    #[test]
    fn pdf_renderer_failure_materializes_and_assigns_default_thumbnail() {
        let temp = TestDir::new();
        let pdf_path = temp.path().join("book.pdf");
        let covers = temp.path().join("covers");
        write_pdf(&pdf_path, None, 1);

        let result = extract_pdf_metadata_with_renderer(
            &pdf_path,
            &covers,
            "renderer-failure",
            &FailingPdfRenderer,
        )
        .unwrap();

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert_eq!(
            fs::read(covers.join(DEFAULT_BOOK_THUMBNAIL)).unwrap(),
            crate::domain::models::default_book_thumbnail_bytes()
        );
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("test renderer unavailable")));
    }

    #[test]
    fn pdf_unsafe_thumbnail_key_uses_format_neutral_warning() {
        let temp = TestDir::new();
        let pdf_path = temp.path().join("book.pdf");
        write_pdf(&pdf_path, None, 1);

        let result = extract_pdf_metadata_with_renderer(
            &pdf_path,
            &temp.path().join("covers"),
            "../unsafe",
            &FailingPdfRenderer,
        )
        .unwrap();

        assert!(result
            .warnings
            .iter()
            .any(|warning| warning == "book thumbnail key is empty or unsafe"));
    }

    #[test]
    fn generated_valid_epub_starts_with_stored_mimetype_entry() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(&epub_path, "<package><metadata/><manifest/></package>", &[]);

        let file = File::open(&epub_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut first = archive.by_index(0).unwrap();
        let mut contents = String::new();
        first.read_to_string(&mut contents).unwrap();

        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), zip::CompressionMethod::Stored);
        assert_eq!(contents, "application/epub+zip");
    }

    #[test]
    fn extracts_namespaced_epub_metadata_and_multiple_creators() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            r#"<?xml version="1.0"?>
              <package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                <metadata>
                  <dc:title>Practical EPUB</dc:title>
                  <dc:creator>Ada Author</dc:creator>
                  <dc:creator>Bea Writer</dc:creator>
                  <dc:description>A &amp; B guide</dc:description>
                  <dc:publisher>Example Press</dc:publisher>
                  <dc:date>2026-07-14</dc:date>
                  <dc:language>en</dc:language>
                  <dc:identifier id="book-id">urn:isbn:978-1-4028-9462-6</dc:identifier>
                </metadata>
                <manifest/>
              </package>"#,
            &[],
        );

        let result = extract_epub_metadata(&epub_path, &temp.path().join("covers"), "42")
            .expect("valid EPUB should extract");

        assert_eq!(result.title.as_deref(), Some("Practical EPUB"));
        assert_eq!(result.authors, ["Ada Author", "Bea Writer"]);
        assert_eq!(result.description.as_deref(), Some("A & B guide"));
        assert_eq!(result.publisher.as_deref(), Some("Example Press"));
        assert_eq!(result.published_date.as_deref(), Some("2026-07-14"));
        assert_eq!(result.language.as_deref(), Some("en"));
        assert_eq!(result.isbn.as_deref(), Some("978-1-4028-9462-6"));
        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
    }

    fn png_bytes_with_dimensions(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([12, 34, 56]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn png_bytes() -> Vec<u8> {
        png_bytes_with_dimensions(3, 2)
    }

    fn gif_bytes() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==")
            .unwrap()
    }

    fn webp_bytes() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode("UklGRh4AAABXRUJQVlA4TBEAAAAvAAAAAAdQwAIWsP+BiOh/AAA=")
            .unwrap()
    }

    fn extract_declared_cover(
        media_type: &str,
        file_name: &str,
        cover: &[u8],
        thumbnail_key: &str,
    ) -> (TestDir, BookMetadataExtraction) {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let package = format!(
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/>
                 <manifest><item id="cover" href="{file_name}" media-type="{media_type}" properties="cover-image"/></manifest>
               </package>"#
        );
        let archive_path = format!("OPS/{file_name}");
        write_epub(&epub_path, &package, &[(archive_path.as_str(), cover)]);
        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), thumbnail_key).unwrap();
        (temp, result)
    }

    #[test]
    fn extracts_epub3_cover_using_safe_opf_relative_path() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let cover = png_bytes();
        write_epub_at(
            &epub_path,
            "OPS/Text/package.opf",
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Covered</dc:title></metadata>
                 <manifest>
                   <item id="front" href="../Images/front%20cover.png" media-type="image/png" properties="nav cover-image"/>
                 </manifest>
               </package>"#,
            &[("OPS/Images/front cover.png", cover.as_slice())],
        );

        let covers = temp.path().join("covers");
        let result = extract_epub_metadata(&epub_path, &covers, "stable-42").unwrap();

        assert_eq!(result.thumbnail, "stable-42.jpg");
        assert!(result.warnings.is_empty());
        let generated = image::open(covers.join(&result.thumbnail)).unwrap();
        assert_eq!((generated.width(), generated.height()), (3, 2));
    }

    #[test]
    fn extracts_epub2_cover_named_by_metadata() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let cover = png_bytes();
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
                 <metadata><meta name="cover" content="cover-item"/></metadata>
                 <manifest><item id="cover-item" href="images/cover.png" media-type="image/png"/></manifest>
               </package>"#,
            &[("OPS/images/cover.png", cover.as_slice())],
        );

        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "epub2").unwrap();

        assert_eq!(result.thumbnail, "epub2.jpg");
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn extracts_gif_cover() {
        let (temp, result) =
            extract_declared_cover("image/gif", "cover.gif", &gif_bytes(), "gif-cover");

        assert_eq!(result.thumbnail, "gif-cover.jpg");
        assert!(result.warnings.is_empty());
        let generated = image::open(temp.path().join("covers").join(&result.thumbnail)).unwrap();
        assert_eq!((generated.width(), generated.height()), (1, 1));
    }

    #[test]
    fn extracts_webp_cover() {
        let (temp, result) =
            extract_declared_cover("image/webp", "cover.webp", &webp_bytes(), "webp-cover");

        assert_eq!(result.thumbnail, "webp-cover.jpg");
        assert!(result.warnings.is_empty());
        let generated = image::open(temp.path().join("covers").join(&result.thumbnail)).unwrap();
        assert_eq!((generated.width(), generated.height()), (1, 1));
    }

    #[test]
    fn accepts_case_insensitive_cover_media_type() {
        let (temp, result) =
            extract_declared_cover("IMAGE/PNG", "cover.png", &png_bytes(), "uppercase-cover");

        assert_eq!(result.thumbnail, "uppercase-cover.jpg");
        assert!(result.warnings.is_empty());
        assert!(temp.path().join("covers/uppercase-cover.jpg").is_file());
    }

    #[test]
    fn rasterizes_svg_cover() {
        let svg =
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1" viewBox="0 0 2 1">
            <rect width="2" height="1" fill="#336699"/>
        </svg>"##;
        let (temp, result) = extract_declared_cover("image/svg+xml", "cover.svg", svg, "svg-cover");

        assert_eq!(result.thumbnail, "svg-cover.jpg");
        assert!(result.warnings.is_empty());
        let generated = image::open(temp.path().join("covers").join(&result.thumbnail)).unwrap();
        assert_eq!((generated.width(), generated.height()), (2, 1));
    }

    #[test]
    fn svg_cover_rejects_external_file_references() {
        let temp = TestDir::new();
        let outside_image = temp.path().join("outside.png");
        fs::write(&outside_image, png_bytes_with_dimensions(1, 1)).unwrap();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
                <image href="{}" width="1" height="1"/>
            </svg>"#,
            outside_image.display()
        );

        let (_, result) =
            extract_declared_cover("image/svg+xml", "cover.svg", svg.as_bytes(), "external-svg");

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("external")));
    }

    #[test]
    fn svg_cover_rejects_relative_file_references() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
            <image href="assets/book/default-book.jpg" width="1" height="1"/>
        </svg>"#;

        let (_, result) = extract_declared_cover("image/svg+xml", "cover.svg", svg, "relative-svg");

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("external")));
    }

    #[test]
    fn svg_cover_rejects_oversized_embedded_raster() {
        let oversized = png_bytes_with_dimensions(MAX_COVER_DIMENSION + 1, 1);
        let encoded = base64::engine::general_purpose::STANDARD.encode(oversized);
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
                <image href="data:image/png;base64,{encoded}" width="1" height="1"/>
            </svg>"#
        );

        let (_, result) =
            extract_declared_cover("image/svg+xml", "cover.svg", svg.as_bytes(), "embedded-svg");

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("embedded")));
    }

    #[test]
    fn svgz_cover_is_rejected_without_decompression() {
        let svgz = base64::engine::general_purpose::STANDARD
            .decode("H4sIACcvVmoAA7MpLktXqMjNySu2VcooKSmw0tcvLy/XKzfWyy9K1zcyMDDQB6pQUijPTCnJsFUyVFLISM1MzygBMe1silKTS7BK6dvZgPTZAQB+uO4kXwAAAA==")
            .unwrap();

        let (_, result) =
            extract_declared_cover("image/svg+xml", "cover.svgz", &svgz, "svgz-cover");

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("uncompressed")));
    }

    #[test]
    fn svg_cover_renders_text_with_bundled_font() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="30">
            <text x="2" y="22" font-family="Arial, sans-serif" font-size="20">Book</text>
        </svg>"#;

        let rendered = decode_svg_cover(svg).unwrap().to_rgba8();

        assert!(rendered.pixels().any(|pixel| pixel.0[3] != 0));
    }

    #[test]
    fn unsafe_cover_path_uses_default_and_preserves_metadata_with_warning() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata><dc:title>Still Useful</dc:title></metadata>
                 <manifest><item id="cover" href="../../outside.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[],
        );

        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "unsafe").unwrap();

        assert_eq!(result.title.as_deref(), Some("Still Useful"));
        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("unsafe")));
        assert!(result.metadata.extraction_error.is_none());
    }

    #[test]
    fn corrupt_cover_uses_default_and_records_decode_problem() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/>
                 <manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", b"not an image")],
        );

        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "corrupt").unwrap();

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("decode")));
    }

    #[test]
    fn oversized_cover_dimensions_fall_back_without_partial_thumbnail() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let oversized_cover = png_bytes_with_dimensions(8_193, 1);
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata><dc:title>Bounded Cover</dc:title></metadata>
                 <manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", oversized_cover.as_slice())],
        );

        let covers = temp.path().join("covers");
        let result = extract_epub_metadata(&epub_path, &covers, "oversized").unwrap();

        assert_eq!(result.title.as_deref(), Some("Bounded Cover"));
        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.to_ascii_lowercase().contains("limit")));
        assert!(covers.join(DEFAULT_BOOK_THUMBNAIL).is_file());
        assert!(!covers.join("oversized.jpg").exists());
    }

    #[test]
    fn missing_optional_metadata_and_cover_is_a_successful_fallback() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf" version="3.0">
                 <opf:metadata/><opf:manifest/>
               </opf:package>"#,
            &[],
        );

        let covers = temp.path().join("covers");
        let result = extract_epub_metadata(&epub_path, &covers, "minimal").unwrap();

        assert_eq!(result.title, None);
        assert!(result.authors.is_empty());
        assert_eq!(result.description, None);
        assert_eq!(result.publisher, None);
        assert_eq!(result.published_date, None);
        assert_eq!(result.language, None);
        assert_eq!(result.isbn, None);
        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(covers.join(DEFAULT_BOOK_THUMBNAIL).is_file());
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("does not declare")));
    }

    #[test]
    fn epub3_identifier_refinement_selects_isbn_from_multiple_identifiers() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            r##"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata>
                   <dc:identifier id="uuid">urn:uuid:12345678</dc:identifier>
                   <dc:identifier id="print-isbn">978 1 4028 9462 6</dc:identifier>
                   <meta refines="#print-isbn" property="identifier-type" scheme="onix:codelist5">15</meta>
                 </metadata>
                 <manifest/>
               </package>"##,
            &[],
        );

        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "isbn").unwrap();

        assert_eq!(result.isbn.as_deref(), Some("978 1 4028 9462 6"));
    }

    #[test]
    fn invalid_signaled_identifier_does_not_hide_later_valid_isbn() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="2.0">
                 <metadata>
                   <dc:identifier scheme="ISBN">not-an-isbn</dc:identifier>
                   <dc:identifier scheme="ISBN">978-1-4028-9462-6</dc:identifier>
                 </metadata>
                 <manifest/>
               </package>"#,
            &[],
        );

        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "isbn").unwrap();

        assert_eq!(result.isbn.as_deref(), Some("978-1-4028-9462-6"));
    }

    #[test]
    fn lowercase_isbn_prefix_is_accepted() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata><dc:identifier>isbn:978-1-4028-9462-6</dc:identifier></metadata>
                 <manifest/>
               </package>"#,
            &[],
        );

        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "isbn").unwrap();

        assert_eq!(result.isbn.as_deref(), Some("978-1-4028-9462-6"));
    }

    #[test]
    fn cover_thumbnail_write_failure_is_soft_and_keeps_metadata() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let cover = png_bytes();
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata><dc:title>Write Failure</dc:title></metadata>
                 <manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", cover.as_slice())],
        );
        let not_a_directory = temp.path().join("not-a-directory");
        fs::write(&not_a_directory, b"file").unwrap();

        let result = extract_epub_metadata(&epub_path, &not_a_directory, "write-failure").unwrap();

        assert_eq!(result.title.as_deref(), Some("Write Failure"));
        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("thumbnail directory")));
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("default book thumbnail")));
    }

    #[test]
    fn atomic_thumbnail_write_preserves_final_and_cleans_temp_on_rename_failure() {
        let temp = TestDir::new();
        let final_path = temp.path().join("existing.jpg");
        fs::create_dir(&final_path).unwrap();
        fs::write(final_path.join("marker"), b"preserve me").unwrap();
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            2,
            image::Rgb([1, 2, 3]),
        ));

        let error = write_jpeg_atomically(&final_path, &image).unwrap_err();

        assert!(error.contains("rename"));
        assert_eq!(fs::read(final_path.join("marker")).unwrap(), b"preserve me");
        let entries: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from("existing.jpg")]);
    }

    #[test]
    fn malformed_package_metadata_is_a_successful_fallback_with_diagnostics() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            "<package><metadata><title>Partial Title</title></metadata><manifest>",
            &[],
        );

        let result =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "broken").unwrap();

        assert_eq!(result.title.as_deref(), Some("Partial Title"));
        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .metadata
            .extraction_error
            .as_deref()
            .is_some_and(|error| error.contains("invalid package document XML")));
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid package document XML")));
    }

    #[test]
    fn unsafe_package_document_path_is_a_hard_extraction_error() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let file = File::create(&epub_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("META-INF/container.xml", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"<container><rootfiles><rootfile full-path="../package.opf"/></rootfiles></container>"#).unwrap();
        zip.finish().unwrap();

        let error =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "broken").unwrap_err();

        assert!(error.to_string().contains("unsafe package document path"));
    }

    fn write_epub_with_custom_container(path: &Path, container: &str) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(container.as_bytes()).unwrap();
        zip.start_file("OPS/package.opf", options).unwrap();
        zip.write_all(b"<package><metadata/><manifest/></package>")
            .unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn container_document_with_wrong_root_is_a_hard_error() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("wrong-root.epub");
        write_epub_with_custom_container(
            &epub_path,
            r#"<wrapper><rootfiles><rootfile full-path="OPS/package.opf"/></rootfiles></wrapper>"#,
        );

        let error = extract_epub_metadata(&epub_path, &temp.path().join("covers"), "wrong-root")
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("container element must be the document root"));
    }

    #[test]
    fn rootfile_outside_rootfiles_is_a_hard_error() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("misplaced-rootfile.epub");
        write_epub_with_custom_container(
            &epub_path,
            r#"<container><rootfile full-path="OPS/package.opf"/></container>"#,
        );

        let error =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "misplaced-rootfile")
                .unwrap_err();

        assert!(error
            .to_string()
            .contains("rootfile must be inside container/rootfiles"));
    }

    #[test]
    fn unsafe_thumbnail_key_falls_back_instead_of_creating_a_colliding_name() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let cover = png_bytes();
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/>
                 <manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", cover.as_slice())],
        );

        let covers = temp.path().join("covers");
        let result = extract_epub_metadata(&epub_path, &covers, "../same/key").unwrap();

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("thumbnail key")));
        assert!(!covers.join("same_key.jpg").exists());
    }

    #[test]
    fn default_thumbnail_name_is_reserved_and_fallback_bytes_are_preserved() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        let cover = png_bytes();
        write_epub(
            &epub_path,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/>
                 <manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", cover.as_slice())],
        );
        let covers = temp.path().join("covers");
        ensure_default_book_thumbnail(&covers).unwrap();
        let expected_default = fs::read(covers.join(DEFAULT_BOOK_THUMBNAIL)).unwrap();

        let result = extract_epub_metadata(&epub_path, &covers, "default-book").unwrap();

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("reserved")));
        assert_eq!(
            fs::read(covers.join(DEFAULT_BOOK_THUMBNAIL)).unwrap(),
            expected_default
        );
    }

    #[test]
    fn package_document_with_non_package_root_is_a_hard_error() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(
            &epub_path,
            "<wrapper><package><metadata/><manifest/></package></wrapper>",
            &[],
        );

        let error =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "broken").unwrap_err();

        assert!(error
            .to_string()
            .contains("package element must be the document root"));
    }

    #[test]
    fn archive_entry_budget_is_rejected_from_eocd_before_zip_parsing() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("many-entries.epub");
        let mut file = File::create(&epub_path).unwrap();
        file.write_all(&0x0605_4b50_u32.to_le_bytes()).unwrap();
        file.write_all(&0_u16.to_le_bytes()).unwrap();
        file.write_all(&0_u16.to_le_bytes()).unwrap();
        file.write_all(&4_097_u16.to_le_bytes()).unwrap();
        file.write_all(&4_097_u16.to_le_bytes()).unwrap();
        file.write_all(&0_u32.to_le_bytes()).unwrap();
        file.write_all(&0_u32.to_le_bytes()).unwrap();
        file.write_all(&0_u16.to_le_bytes()).unwrap();
        drop(file);

        let error = extract_epub_metadata(&epub_path, &temp.path().join("covers"), "many-entries")
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("archive entry count 4097 exceeds limit 4096"));
    }

    async fn ingestion_dependencies(book_root: &Path) -> (FileStorer, Repository) {
        let storer: FileStorer = Arc::new(FileSystemStore::new(
            book_root.to_str().expect("test path should be UTF-8"),
        ));
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        (storer, repository)
    }

    #[tokio::test]
    async fn ingests_epub_metadata_moves_file_and_saves_record() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("practical.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata>
                   <dc:title>Practical EPUB</dc:title>
                   <dc:creator>Ada Author</dc:creator>
                   <dc:language>en</dc:language>
                 </metadata>
                 <manifest/>
               </package>"#,
            &[],
        );
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let details = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            Some("science fiction".to_string()),
            book_root.clone(),
            thumbnail_root.clone(),
        )
        .await
        .unwrap()
        .expect("stable EPUB should be ingested");

        assert_eq!(details.title, "Practical EPUB");
        assert_eq!(details.authors, ["Ada Author"]);
        assert_eq!(details.language.as_deref(), Some("en"));
        assert_eq!(details.format, BookFormat::Epub);
        assert_eq!(details.collection, "Science Fiction");
        assert_eq!(details.state, BookState::Ready);
        assert_eq!(details.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(!source.exists());
        assert!(book_root.join("Science Fiction/practical.epub").exists());
        assert!(thumbnail_root.join(DEFAULT_BOOK_THUMBNAIL).exists());
        let saved = repository.retrieve_book(details.checksum).await.unwrap();
        assert_eq!(saved.file_name, details.file_name);
        assert_eq!(saved.collection, details.collection);
        assert_eq!(saved.title, details.title);
        assert_eq!(saved.authors, details.authors);
        assert_eq!(saved.state, details.state);
    }

    #[tokio::test]
    async fn ingests_pdf_metadata_moves_file_and_saves_record() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("manual.pdf");
        write_pdf(
            &source,
            Some(dictionary! {
                "Title" => text_string("PDF Manual"),
                "Author" => text_string("Pat Writer"),
            }),
            3,
        );
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let details = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            None,
            book_root.clone(),
            thumbnail_root.clone(),
        )
        .await
        .unwrap()
        .expect("stable PDF should be ingested");

        assert_eq!(details.title, "PDF Manual");
        assert_eq!(details.authors, ["Pat Writer"]);
        assert_eq!(details.page_count, Some(3));
        assert_eq!(details.format, BookFormat::Pdf);
        assert_eq!(details.collection, "downloads");
        assert_eq!(details.state, BookState::Ready);
        assert_eq!(details.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert!(!source.exists());
        assert!(book_root.join("downloads/manual.pdf").exists());
        let saved = repository.retrieve_book(details.checksum).await.unwrap();
        assert_eq!(saved.file_name, details.file_name);
        assert_eq!(saved.collection, details.collection);
        assert_eq!(saved.title, details.title);
        assert_eq!(saved.authors, details.authors);
        assert_eq!(saved.page_count, details.page_count);
        assert_eq!(saved.state, details.state);
    }

    #[tokio::test]
    async fn corrupt_epub_uses_weak_metadata_default_thumbnail_and_still_saves() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("fallback_title.epub");
        fs::write(&source, b"not an epub").unwrap();
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let details = generate_book_metadata_with_roots(
            source,
            storer,
            repository.clone(),
            Some("fallbacks".to_string()),
            book_root,
            thumbnail_root,
        )
        .await
        .unwrap()
        .expect("stable corrupt EPUB should still be recorded");

        assert_eq!(details.title, "fallback title");
        assert!(details.authors.is_empty());
        assert_eq!(details.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert_eq!(details.state, BookState::MetadataError);
        assert!(details.metadata.extraction_error.is_some());
        let saved = repository.retrieve_book(details.checksum).await.unwrap();
        assert_eq!(saved.title, details.title);
        assert_eq!(saved.thumbnail, details.thumbnail);
        assert_eq!(saved.state, details.state);
        assert_eq!(saved.metadata, details.metadata);
    }
}
