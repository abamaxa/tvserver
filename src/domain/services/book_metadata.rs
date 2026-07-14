use crate::domain::models::{ensure_default_book_thumbnail, BookMetadata, DEFAULT_BOOK_THUMBNAIL};
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use serde_json::json;
use std::{
    fs::{self, File},
    io::{BufWriter, Read},
    path::{Path, PathBuf},
};
use zip::ZipArchive;

const MAX_CONTAINER_BYTES: u64 = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BookMetadataExtraction {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
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
}

pub fn extract_epub_metadata(
    epub_path: &Path,
    thumbnail_dir: &Path,
    thumbnail_key: &str,
) -> Result<BookMetadataExtraction, BookMetadataExtractionError> {
    let file = File::open(epub_path)
        .map_err(|error| BookMetadataExtractionError::Open(error.to_string()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| BookMetadataExtractionError::Archive(error.to_string()))?;

    let container =
        read_bounded_entry(&mut archive, "META-INF/container.xml", MAX_CONTAINER_BYTES)?;
    let package_path = parse_container(&container)?;
    let package = read_bounded_entry(&mut archive, &package_path, MAX_PACKAGE_BYTES)?;
    let parsed = parse_package(&package)?;
    let isbn = parsed.isbn();

    let (thumbnail, warnings) = match extract_cover(
        &mut archive,
        &package_path,
        &parsed,
        thumbnail_dir,
        thumbnail_key,
    ) {
        Ok(thumbnail) => (thumbnail, Vec::new()),
        Err(cover_warning) => {
            tracing::warn!(epub = %epub_path.display(), "{cover_warning}");
            let mut warnings = vec![cover_warning];
            if let Err(error) = ensure_default_book_thumbnail(thumbnail_dir) {
                let warning = format!("could not prepare default book thumbnail: {error}");
                tracing::warn!(epub = %epub_path.display(), "{warning}");
                warnings.push(warning);
            }
            (DEFAULT_BOOK_THUMBNAIL.to_string(), warnings)
        }
    };

    Ok(BookMetadataExtraction {
        title: parsed.title,
        authors: parsed.authors,
        description: parsed.description,
        publisher: parsed.publisher,
        published_date: parsed.published_date,
        language: parsed.language,
        isbn,
        thumbnail,
        metadata: BookMetadata {
            raw: Some(json!({
                "epub": {
                    "packagePath": package_path,
                    "coverWarnings": warnings,
                }
            })),
            extraction_error: None,
        },
        warnings,
    })
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
    let mut depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                depth += 1;
                if package_path.is_none() && local_name(element.name().as_ref()) == b"rootfile" {
                    package_path = parse_rootfile_path(&reader, &element)?;
                }
            }
            Ok(Event::Empty(element)) if local_name(element.name().as_ref()) == b"rootfile" => {
                if package_path.is_none() {
                    package_path = parse_rootfile_path(&reader, &element)?;
                }
            }
            Ok(Event::End(_)) => {
                if depth == 0 {
                    return Err(BookMetadataExtractionError::InvalidPackage(
                        "invalid META-INF/container.xml: unexpected closing element".to_string(),
                    ));
                }
                depth -= 1;
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
    if depth != 0 {
        return Err(BookMetadataExtractionError::InvalidPackage(
            "invalid META-INF/container.xml: unclosed element".to_string(),
        ));
    }
    package_path.ok_or_else(|| {
        BookMetadataExtractionError::InvalidPackage(
            "META-INF/container.xml has no rootfile full-path".to_string(),
        )
    })
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
            .find(|identifier| identifier.scheme.as_deref().is_some_and(is_isbn_signal))
            .or_else(|| {
                self.identifiers.iter().find(|identifier| {
                    identifier.id.as_ref().is_some_and(|id| {
                        self.refinements.iter().any(|refinement| {
                            refinement.identifier_id == *id && is_isbn_signal(&refinement.value)
                        })
                    })
                })
            })
            .or_else(|| {
                self.identifiers
                    .iter()
                    .find(|identifier| isbn_value(&identifier.value).is_some())
            })
            .and_then(|identifier| isbn_value(&identifier.value))
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

fn parse_package(bytes: &[u8]) -> Result<ParsedPackage, BookMetadataExtractionError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut parsed = ParsedPackage::default();
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
                }
                depth += 1;
                if name == b"metadata" {
                    in_metadata = true;
                } else if name == b"manifest" {
                    in_manifest = true;
                } else if in_manifest && name == b"item" {
                    parse_manifest_item(&reader, &element, &mut parsed)?;
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
                } else if depth == 0 {
                    return Err(BookMetadataExtractionError::InvalidPackage(
                        "package element must be the document root".to_string(),
                    ));
                } else if in_manifest && name == b"item" {
                    parse_manifest_item(&reader, &element, &mut parsed)?;
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
    Ok(parsed)
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
    let decoded = image::load_from_memory(&bytes)
        .map_err(|error| format!("could not decode EPUB cover {cover_path:?}: {error}"))?;
    let thumbnail_name = thumbnail_filename(thumbnail_key)
        .ok_or_else(|| "EPUB cover thumbnail key is empty or unsafe".to_string())?;

    fs::create_dir_all(thumbnail_dir)
        .map_err(|error| format!("could not create EPUB thumbnail directory: {error}"))?;
    let thumbnail_path = thumbnail_dir.join(&thumbnail_name);
    write_jpeg(&thumbnail_path, &decoded).map_err(|error| {
        let _ = fs::remove_file(&thumbnail_path);
        format!("could not write EPUB cover thumbnail: {error}")
    })?;
    Ok(thumbnail_name)
}

fn is_image_manifest_item(item: &ManifestItem) -> bool {
    matches!(
        item.media_type.as_deref(),
        Some("image/jpeg" | "image/jpg" | "image/png")
    )
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

fn thumbnail_filename(key: &str) -> Option<String> {
    let key = key.trim();
    (!key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')))
    .then(|| format!("{key}.jpg"))
}

fn write_jpeg(path: &PathBuf, image: &image::DynamicImage) -> Result<(), image::ImageError> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 85);
    encoder.encode_image(&image.to_rgb8())
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
    let without_prefix = trimmed
        .strip_prefix("urn:isbn:")
        .or_else(|| trimmed.strip_prefix("URN:ISBN:"))
        .or_else(|| trimmed.strip_prefix("ISBN:"))
        .unwrap_or(trimmed)
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
    use std::{
        fs::{self, File},
        io::Write,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };
    use zip::{write::SimpleFileOptions, ZipWriter};

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
        let options = SimpleFileOptions::default();
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

    fn png_bytes() -> Vec<u8> {
        let image = image::RgbImage::from_pixel(3, 2, image::Rgb([12, 34, 56]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
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
    fn malformed_package_xml_is_a_hard_extraction_error() {
        let temp = TestDir::new();
        let epub_path = temp.path().join("book.epub");
        write_epub(&epub_path, "<package><metadata>", &[]);

        let error =
            extract_epub_metadata(&epub_path, &temp.path().join("covers"), "broken").unwrap_err();

        assert!(error.to_string().contains("invalid package document XML"));
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
}
