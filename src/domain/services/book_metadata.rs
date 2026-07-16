use crate::domain::{
    algorithm::{collection_id_to_path, path_to_collection_id, title_case},
    config::{get_book_dir, get_book_thumbnail_dir},
    models::{
        ensure_default_book_thumbnail, BookDetails, BookFormat, BookMetadata, BookState,
        DEFAULT_BOOK_THUMBNAIL,
    },
    traits::{FileStorer, PrivateSnapshot, Repository, StagedFile},
};
use lopdf::{decode_text_string, Dictionary, Document};
use once_cell::sync::Lazy;
use quick_xml::{
    events::{BytesStart, Event},
    Reader,
};
use serde_json::json;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex, Weak,
    },
};
use zip::ZipArchive;
use tokio_util::sync::CancellationToken;

const MAX_EPUB_ARCHIVE_ENTRIES: u16 = 4_096;
const MAX_CENTRAL_DIRECTORY_BYTES: u32 = 8 * 1024 * 1024;
const MAX_EOCD_TAIL_BYTES: u64 = 22 + u16::MAX as u64;
const MAX_CONTAINER_BYTES: u64 = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;
const MAX_COVER_DIMENSION: u32 = 8_192;
const MAX_COVER_PIXELS: u64 = 8_000_000;
const MAX_COVER_DECODE_ALLOC_BYTES: u64 = 48 * 1024 * 1024;
const BOOK_STABILITY_ATTEMPTS: usize = 3;
const BOOK_STABILITY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
const SVG_FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/book/FiraSans-Regular.ttf"
));
static BOOK_DESTINATION_LOCKS: Lazy<StdMutex<HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));
static BOOK_THUMBNAIL_LOCKS: Lazy<StdMutex<HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));
#[cfg(test)]
static BOOK_EXTRACTION_BARRIERS: Lazy<StdMutex<HashMap<PathBuf, Weak<ExtractionTestBarrier>>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));
#[cfg(test)]
static BOOK_POST_EXTRACTION_BARRIERS: Lazy<StdMutex<HashMap<PathBuf, Weak<ExtractionTestBarrier>>>> =
    Lazy::new(|| StdMutex::new(HashMap::new()));

#[cfg(test)]
struct ExtractionTestBarrier {
    started: tokio::sync::Notify,
    released: StdMutex<bool>,
    released_signal: std::sync::Condvar,
}

#[cfg(test)]
impl ExtractionTestBarrier {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.released_signal.notify_all();
    }

    fn wait(&self) {
        self.started.notify_one();
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.released_signal.wait(released).unwrap();
        }
    }
}

#[cfg(test)]
fn install_extraction_test_barrier(path: &Path) -> Arc<ExtractionTestBarrier> {
    let barrier = Arc::new(ExtractionTestBarrier {
        started: tokio::sync::Notify::new(),
        released: StdMutex::new(false),
        released_signal: std::sync::Condvar::new(),
    });
    BOOK_EXTRACTION_BARRIERS
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), Arc::downgrade(&barrier));
    barrier
}

#[cfg(test)]
fn install_post_extraction_test_barrier(path: &Path) -> Arc<ExtractionTestBarrier> {
    let barrier = Arc::new(ExtractionTestBarrier {
        started: tokio::sync::Notify::new(),
        released: StdMutex::new(false),
        released_signal: std::sync::Condvar::new(),
    });
    BOOK_POST_EXTRACTION_BARRIERS
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), Arc::downgrade(&barrier));
    barrier
}

#[cfg(test)]
fn wait_for_extraction_test_barrier(path: &Path) {
    let barrier = BOOK_EXTRACTION_BARRIERS
        .lock()
        .unwrap()
        .get(path)
        .and_then(Weak::upgrade);
    if let Some(barrier) = barrier {
        barrier.wait();
    }
}

#[cfg(test)]
fn wait_for_post_extraction_test_barrier(path: &Path) {
    let barrier = BOOK_POST_EXTRACTION_BARRIERS
        .lock()
        .unwrap()
        .get(path)
        .and_then(Weak::upgrade);
    if let Some(barrier) = barrier {
        barrier.wait();
    }
}

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
    generate_book_metadata_with_cancellation(
        path,
        storer,
        repository,
        suggested_collection,
        CancellationToken::new(),
    )
    .await
}

pub(crate) async fn generate_book_metadata_with_cancellation(
    path: PathBuf,
    storer: FileStorer,
    repository: Repository,
    suggested_collection: Option<String>,
    cancellation: CancellationToken,
) -> anyhow::Result<Option<BookDetails>> {
    let book_dir = get_book_dir();
    let book_root = PathBuf::from(&book_dir);
    let thumbnail_root = get_book_thumbnail_dir(&book_dir);
    generate_book_metadata_with_roots_and_cancellation(
        path,
        storer,
        repository,
        suggested_collection,
        book_root,
        thumbnail_root,
        cancellation,
    )
    .await
}

#[cfg(test)]
async fn generate_book_metadata_with_roots(
    path: PathBuf,
    storer: FileStorer,
    repository: Repository,
    suggested_collection: Option<String>,
    book_root: PathBuf,
    thumbnail_root: PathBuf,
) -> anyhow::Result<Option<BookDetails>> {
    generate_book_metadata_with_roots_and_cancellation(
        path,
        storer,
        repository,
        suggested_collection,
        book_root,
        thumbnail_root,
        CancellationToken::new(),
    )
    .await
}

async fn generate_book_metadata_with_roots_and_cancellation(
    path: PathBuf,
    storer: FileStorer,
    repository: Repository,
    suggested_collection: Option<String>,
    book_root: PathBuf,
    thumbnail_root: PathBuf,
    cancellation: CancellationToken,
) -> anyhow::Result<Option<BookDetails>> {
    if cancellation.is_cancelled() {
        anyhow::bail!("book ingestion cancelled before staging: {}", path.display());
    }
    let format = book_format(&path)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("book path has no UTF-8 file name: {}", path.display()))?
        .to_string();
    let collection = match suggested_collection.as_deref() {
        Some(collection) => title_case(collection),
        None => collection_from_source(&path, &book_root)?,
    };
    let collection_path = validate_collection(&collection)?;
    let destination_directory = book_root.join(collection_path);
    let destination = destination_directory.join(&file_name);
    let source_absolute = absolute_path(&path)?;
    let destination_absolute = absolute_path(&destination)?;
    let destination_lock = destination_lock(&destination_absolute)?;
    let _destination_guard = destination_lock.lock_owned().await;
    if source_absolute != destination_absolute {
        match tokio::fs::symlink_metadata(&destination).await {
            Ok(_) => anyhow::bail!(
                "book destination already exists; refusing to replace it: {}",
                destination.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    let source = path.to_str().ok_or_else(|| {
        anyhow::anyhow!("book source path is not valid UTF-8: {}", path.display())
    })?;
    let staged = storer.stage_no_follow(source).await?;
    let mut staged_guard = StagedSourceGuard::new(staged.clone());
    let staged_path = staged.staged_path.clone();
    if cancellation.is_cancelled() {
        storer.restore_staged(&staged).await?;
        staged_guard.disarm();
        anyhow::bail!("book ingestion cancelled after staging: {}", path.display());
    }
    if let Err(error) = storer.create_folder(&destination_directory).await {
        storer.restore_staged(&staged).await.map_err(|restore_error| {
            anyhow::anyhow!(
                "{error}; additionally failed to restore staged source: {restore_error}"
            )
        })?;
        staged_guard.disarm();
        return Err(error);
    }

    let mut accepted = None;
    for attempt in 1..=BOOK_STABILITY_ATTEMPTS {
        let before = match staged_fingerprint(&staged_path).await {
            Ok(before) => before,
            Err(error) => {
                tracing::warn!(book = %path.display(), attempt, "Unsafe staged source fingerprint: {error}");
                continue;
            }
        };
        if before.len == 0 {
            storer.restore_staged(&staged).await?;
            staged_guard.disarm();
            anyhow::bail!("cannot ingest zero-byte book: {}", path.display());
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                storer.restore_staged(&staged).await?;
                staged_guard.disarm();
                anyhow::bail!("book ingestion cancelled during stability check: {}", path.display());
            }
            _ = tokio::time::sleep(BOOK_STABILITY_INTERVAL) => {}
        }
        let stable = match staged_fingerprint(&staged_path).await {
            Ok(stable) => stable,
            Err(error) => {
                tracing::warn!(book = %path.display(), attempt, "Unsafe staged source after stability wait: {error}");
                continue;
            }
        };
        if before != stable {
            tracing::info!(
                book = %path.display(),
                attempt,
                "Book source changed during stability check; retrying"
            );
            continue;
        }

        let snapshot = match storer.create_private_snapshot(&staged).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    book = %path.display(),
                    attempt,
                    "Could not create a stable private book snapshot: {error}"
                );
                continue;
            }
        };
        if cancellation.is_cancelled() {
            let cleanup = cleanup_prepublication(
                &storer,
                &staged,
                Some(&snapshot),
                None,
                CleanupMode::Restore,
            )
            .await;
            staged_guard.disarm();
            return Err(with_cleanup_error(
                anyhow::anyhow!("book ingestion cancelled after snapshot creation: {}", path.display()),
                cleanup,
            ));
        }
        let after_copy = match staged_fingerprint(&staged_path).await {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                let primary = anyhow::anyhow!(
                    "staged source became unsafe during private snapshot copy: {error}"
                );
                if let Err(cleanup_error) = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    None,
                    CleanupMode::RetryIfClean,
                )
                .await
                {
                    staged_guard.disarm();
                    return Err(with_cleanup_error(primary, Err(cleanup_error)));
                }
                tracing::warn!(
                    book = %path.display(),
                    attempt,
                    "Staged source became unsafe during private snapshot copy: {error}"
                );
                continue;
            }
        };
        if stable != after_copy {
            let primary = anyhow::anyhow!("book source changed during private snapshot copy");
            if let Err(cleanup_error) = cleanup_prepublication(
                &storer,
                &staged,
                Some(&snapshot),
                None,
                CleanupMode::RetryIfClean,
            )
            .await
            {
                staged_guard.disarm();
                return Err(with_cleanup_error(primary, Err(cleanup_error)));
            }
            tracing::warn!(
                book = %path.display(),
                attempt,
                "Book source changed during private snapshot copy; retrying"
            );
            continue;
        }

        let checksum_result = async {
            let before = private_snapshot_fingerprint(&snapshot).await?;
            let checksum = super::video_metadata::calculate_checksum(&snapshot.path).await?;
            let after = private_snapshot_fingerprint(&snapshot).await?;
            if before != after {
                anyhow::bail!("private snapshot identity changed during checksum calculation");
            }
            anyhow::Result::<_>::Ok(checksum)
        }
        .await;
        let checksum = match checksum_result {
            Ok(checksum) => checksum,
            Err(error) => {
                let cleanup = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    None,
                    CleanupMode::Restore,
                )
                .await;
                staged_guard.disarm();
                return Err(with_cleanup_error(error.into(), cleanup));
            }
        };
        let thumbnail_key = checksum.to_string();
        let generated_thumbnail_path = thumbnail_root.join(format!("{thumbnail_key}.jpg"));
        let thumbnail_lock = match thumbnail_lock(&generated_thumbnail_path) {
            Ok(lock) => lock,
            Err(error) => {
                let cleanup = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    None,
                    CleanupMode::Restore,
                )
                .await;
                staged_guard.disarm();
                return Err(with_cleanup_error(error, cleanup));
            }
        };
        let thumbnail_lease = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                let cleanup = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    None,
                    CleanupMode::Restore,
                )
                .await;
                staged_guard.disarm();
                return Err(with_cleanup_error(
                    anyhow::anyhow!("book ingestion cancelled while waiting for thumbnail ownership: {}", path.display()),
                    cleanup,
                ));
            }
            lease = thumbnail_lock.lock_owned() => lease,
        };
        let existing = match repository.retrieve_book(checksum).await {
            Ok(existing) => Some(existing),
            Err(sqlx::Error::RowNotFound) => None,
            Err(error) => {
                let cleanup = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    None,
                    CleanupMode::Restore,
                )
                .await;
                staged_guard.disarm();
                return Err(with_cleanup_error(error.into(), cleanup));
            }
        };
        if let Some(existing) = existing {
            let canonical_relative = (|| -> anyhow::Result<PathBuf> {
                let mut canonical_relative = validate_collection(&existing.collection)?;
                let existing_file = Path::new(&existing.file_name);
                if existing_file.components().count() != 1
                    || !matches!(
                        existing_file.components().next(),
                        Some(std::path::Component::Normal(_))
                    )
                {
                    anyhow::bail!("stored book file name is not a safe path component");
                }
                canonical_relative.push(existing_file);
                Ok(canonical_relative)
            })();
            let canonical_exists = match canonical_relative {
                Ok(relative) => storer.regular_file_exists_no_follow(&relative).await,
                Err(error) => Err(error),
            };
            match canonical_exists {
                Ok(true) => {
                    let cleanup = cleanup_healthy_duplicate(&storer, &staged, &snapshot).await;
                    staged_guard.disarm();
                    cleanup?;
                    return Ok(Some(existing));
                }
                Ok(false) => {}
                Err(error) => {
                    let cleanup = cleanup_prepublication(
                        &storer,
                        &staged,
                        Some(&snapshot),
                        None,
                        CleanupMode::Restore,
                    )
                    .await;
                    staged_guard.disarm();
                    return Err(with_cleanup_error(error, cleanup));
                }
            }
        }
        let thumbnail_preexisted = generated_thumbnail_path.symlink_metadata().is_ok();
        let mut thumbnail_guard = GeneratedThumbnailGuard::new(
            generated_thumbnail_path,
            thumbnail_preexisted,
        );
        let snapshot_before_extraction = match private_snapshot_fingerprint(&snapshot).await {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                let cleanup = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    Some(&mut thumbnail_guard),
                    CleanupMode::Restore,
                )
                .await;
                staged_guard.disarm();
                return Err(with_cleanup_error(error, cleanup));
            }
        };
        let extraction_path = snapshot.path.clone();
        let extraction_thumbnail_root = thumbnail_root.clone();
        let extraction = match tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            wait_for_extraction_test_barrier(&extraction_thumbnail_root);
            let extraction = match format {
                BookFormat::Pdf => extract_pdf_metadata(
                    &extraction_path,
                    &extraction_thumbnail_root,
                    &thumbnail_key,
                ),
                BookFormat::Epub => extract_epub_metadata(
                    &extraction_path,
                    &extraction_thumbnail_root,
                    &thumbnail_key,
                ),
            };
            #[cfg(test)]
            wait_for_post_extraction_test_barrier(&extraction_thumbnail_root);
            extraction
        })
        .await {
            Ok(extraction) => extraction,
            Err(error) => {
                let cleanup = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    Some(&mut thumbnail_guard),
                    CleanupMode::Restore,
                )
                .await;
                staged_guard.disarm();
                return Err(with_cleanup_error(
                    anyhow::anyhow!("book metadata worker failed: {error}"),
                    cleanup,
                ));
            }
        };
        if cancellation.is_cancelled() {
            let cleanup = cleanup_prepublication(
                &storer,
                &staged,
                Some(&snapshot),
                Some(&mut thumbnail_guard),
                CleanupMode::Restore,
            )
            .await;
            staged_guard.disarm();
            return Err(with_cleanup_error(
                anyhow::anyhow!("book ingestion cancelled during metadata extraction: {}", path.display()),
                cleanup,
            ));
        }

        let snapshot_before_verification = private_snapshot_fingerprint(&snapshot).await;
        let verified_checksum = match &snapshot_before_verification {
            Ok(_) => super::video_metadata::calculate_checksum(&snapshot.path).await,
            Err(error) => Err(std::io::Error::other(format!("{error:#}"))),
        };
        let snapshot_seal = match storer.seal_private_snapshot(&snapshot).await {
            Ok(seal) => seal,
            Err(error) => {
                let cleanup = cleanup_prepublication(
                    &storer,
                    &staged,
                    Some(&snapshot),
                    Some(&mut thumbnail_guard),
                    CleanupMode::Restore,
                )
                .await;
                staged_guard.disarm();
                return Err(with_cleanup_error(error, cleanup));
            }
        };
        let snapshot_after_verification = private_snapshot_fingerprint(&snapshot).await;
        let snapshot_is_verified = matches!(
            (
                &snapshot_before_verification,
                &verified_checksum,
                &snapshot_after_verification,
            ),
            (Ok(before), Ok(verified_checksum), Ok(after))
                if snapshot_before_extraction == *before
                    && before == after
                    && checksum == *verified_checksum
        );
        if !snapshot_is_verified {
            let primary = anyhow::anyhow!(
                "private book snapshot changed during metadata extraction"
            );
            if let Err(cleanup_error) = cleanup_prepublication(
                &storer,
                &staged,
                Some(&snapshot),
                Some(&mut thumbnail_guard),
                CleanupMode::RetryIfClean,
            )
            .await
            {
                staged_guard.disarm();
                return Err(with_cleanup_error(primary, Err(cleanup_error)));
            }
            tracing::warn!(
                book = %path.display(),
                attempt,
                "Private book snapshot changed during metadata extraction; retrying"
            );
            continue;
        }

        let mut details = BookDetails::new(file_name.clone(), collection.clone(), &path, format);
        details.checksum = checksum;
        details.search_phrase = suggested_collection.clone();
        match extraction {
            Ok(mut extraction) => {
                if format == BookFormat::Pdf
                    && extraction.title.as_deref()
                        == Some(filename_derived_title(&staged_path).as_str())
                {
                    extraction.title = Some(filename_derived_title(&path));
                }
                apply_extraction(&mut details, extraction)
            }
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
        accepted = Some((
            details,
            thumbnail_guard,
            snapshot,
            snapshot_seal,
            thumbnail_lease,
        ));
        break;
    }

    let (mut details, mut thumbnail_guard, snapshot, snapshot_seal, _thumbnail_lease) = match accepted {
        Some(accepted) => accepted,
        None => {
            let error = anyhow::anyhow!(
                "book source did not become stable after {BOOK_STABILITY_ATTEMPTS} attempts: {}",
                path.display()
            );
            storer.restore_staged(&staged).await.map_err(|restore_error| {
                anyhow::anyhow!(
                    "{error}; additionally failed to restore staged source: {restore_error}"
                )
            })?;
            staged_guard.disarm();
            return Err(error);
        }
    };

    let destination_path = destination.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "book destination path is not valid UTF-8: {}",
            destination.display()
        )
    })?;
    if cancellation.is_cancelled() {
        let cleanup = cleanup_prepublication(
            &storer,
            &staged,
            Some(&snapshot),
            Some(&mut thumbnail_guard),
            CleanupMode::Restore,
        )
        .await;
        staged_guard.disarm();
        return Err(with_cleanup_error(
            anyhow::anyhow!("book ingestion cancelled before publication: {}", path.display()),
            cleanup,
        ));
    }
    if let Err(error) = storer
        .publish_private_snapshot_no_replace(
            &snapshot,
            destination_path,
            &snapshot_seal,
        )
        .await
    {
        let thumbnail_cleanup = thumbnail_guard.cleanup(&storer).await;
        let snapshot_cleanup = storer.remove_private_snapshot(&snapshot).await;
        let restore = storer.restore_staged(&staged).await;
        staged_guard.disarm();
        if let Err(cleanup_error) = snapshot_cleanup {
            return Err(anyhow::anyhow!(
                "{error}; additionally failed to remove private snapshot: {cleanup_error}"
            ));
        }
        if let Err(cleanup_error) = thumbnail_cleanup {
            return Err(anyhow::anyhow!(
                "{error}; additionally failed to remove generated thumbnail: {cleanup_error}"
            ));
        }
        if let Err(restore_error) = restore {
            return Err(anyhow::anyhow!(
                "{error}; additionally failed to restore staged source: {restore_error}"
            ));
        }
        return Err(error);
    }
    if let Err(error) = storer.discard_staged(&staged).await {
        tracing::warn!(
            staged = %staged.staged_path.display(),
            "Published private book snapshot but could not remove staged downloader source: {error}"
        );
    }
    staged_guard.disarm();

    details.collection = collection;
    details.file_name = file_name;
    details.dir_path = None;
    if let Err(error) = repository.save_book(&details).await {
        let cleanup = thumbnail_guard.cleanup(&storer).await;
        return Err(with_cleanup_error(error.into(), cleanup));
    }
    thumbnail_guard.disarm();
    Ok(Some(details))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StagedFingerprint {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

async fn staged_fingerprint(path: &Path) -> anyhow::Result<StagedFingerprint> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        anyhow::bail!(
            "staged book source must be a regular file and not a symlink: {}",
            path.display()
        );
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(StagedFingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
    })
}

async fn private_snapshot_fingerprint(
    snapshot: &PrivateSnapshot,
) -> anyhow::Result<StagedFingerprint> {
    if !snapshot.path_has_creation_identity()? {
        anyhow::bail!(
            "private snapshot path no longer names its creation-time file identity: {}",
            snapshot.path.display()
        );
    }
    staged_fingerprint(&snapshot.path).await
}

struct StagedSourceGuard {
    staged: Option<StagedFile>,
}

impl StagedSourceGuard {
    fn new(staged: StagedFile) -> Self {
        Self { staged: Some(staged) }
    }

    fn disarm(&mut self) {
        self.staged = None;
    }
}

impl Drop for StagedSourceGuard {
    fn drop(&mut self) {
        if let Some(staged) = self.staged.take() {
            tracing::warn!(
                staged = %staged.staged_path.display(),
                original = %staged.original_path.display(),
                "Staged book source guard dropped before awaited cleanup"
            );
        }
    }
}

struct GeneratedThumbnailGuard {
    path: Option<PathBuf>,
}

impl GeneratedThumbnailGuard {
    fn new(path: PathBuf, preexisted: bool) -> Self {
        Self {
            path: (!preexisted).then_some(path),
        }
    }

    fn disarm(&mut self) {
        self.path = None;
    }

    async fn cleanup(&mut self, storer: &FileStorer) -> anyhow::Result<()> {
        let Some(path) = self.path.take() else {
            return Ok(());
        };
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(DEFAULT_BOOK_THUMBNAIL))
        {
            return Ok(());
        }
        storer.remove_regular_no_follow(&path).await
    }
}

impl Drop for GeneratedThumbnailGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            tracing::warn!(
                thumbnail = %path.display(),
                "Generated book thumbnail guard dropped before awaited cleanup"
            );
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CleanupMode {
    RetryIfClean,
    Restore,
}

async fn cleanup_prepublication(
    storer: &FileStorer,
    staged: &StagedFile,
    snapshot: Option<&PrivateSnapshot>,
    thumbnail: Option<&mut GeneratedThumbnailGuard>,
    mode: CleanupMode,
) -> anyhow::Result<()> {
    let mut failures = Vec::new();
    if let Some(thumbnail) = thumbnail {
        if let Err(error) = thumbnail.cleanup(storer).await {
            failures.push(format!("generated thumbnail cleanup failed: {error}"));
        }
    }
    if let Some(snapshot) = snapshot {
        if let Err(error) = storer.remove_private_snapshot(snapshot).await {
            failures.push(format!("private snapshot cleanup failed: {error}"));
        }
    }
    if mode == CleanupMode::Restore || !failures.is_empty() {
        if let Err(error) = storer.restore_staged(staged).await {
            failures.push(format!("staged source restoration failed: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(failures.join("; "))
    }
}

async fn cleanup_healthy_duplicate(
    storer: &FileStorer,
    staged: &StagedFile,
    snapshot: &PrivateSnapshot,
) -> anyhow::Result<()> {
    if let Err(error) = storer.remove_private_snapshot(snapshot).await {
        let restore = storer.restore_staged(staged).await;
        return Err(with_cleanup_error(
            anyhow::anyhow!("private snapshot cleanup failed: {error}"),
            restore,
        ));
    }
    if let Err(error) = storer.discard_staged(staged).await {
        let restore = storer.restore_staged(staged).await;
        return Err(with_cleanup_error(
            anyhow::anyhow!("staged source discard failed: {error}"),
            restore,
        ));
    }
    Ok(())
}

fn with_cleanup_error(error: anyhow::Error, cleanup: anyhow::Result<()>) -> anyhow::Error {
    match cleanup {
        Ok(()) => error,
        Err(cleanup_error) => anyhow::anyhow!("{error}; additionally {cleanup_error}"),
    }
}

fn apply_extraction(details: &mut BookDetails, extraction: BookMetadataExtraction) {
    if extraction.metadata.extraction_error.is_some() {
        details.thumbnail = DEFAULT_BOOK_THUMBNAIL.to_string();
        details.metadata = extraction.metadata;
        details.state = BookState::MetadataError;
        return;
    }

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
    if parent.as_os_str().is_empty() {
        return Ok(String::new());
    }
    let parent = absolute_path(parent)?;
    let book_root = absolute_path(book_root)?;
    match parent.strip_prefix(&book_root) {
        Ok(relative) => path_to_collection_id(relative)
            .ok_or_else(|| anyhow::anyhow!("book collection path is not valid UTF-8")),
        Err(_) => {
            let fallback = parent.file_name().map(Path::new).unwrap_or_else(|| Path::new(""));
            path_to_collection_id(fallback)
                .ok_or_else(|| anyhow::anyhow!("book collection path is not valid UTF-8"))
        }
    }
}

fn validate_collection(collection: &str) -> anyhow::Result<PathBuf> {
    collection_id_to_path(collection).ok_or_else(|| {
        anyhow::anyhow!(
            "book collection must be a relative path without traversal: {collection}"
        )
    })
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    std::path::absolute(path).map_err(|error| {
        anyhow::anyhow!("could not make path absolute ({}): {error}", path.display())
    })
}

fn destination_lock(path: &Path) -> anyhow::Result<Arc<tokio::sync::Mutex<()>>> {
    let mut locks = BOOK_DESTINATION_LOCKS
        .lock()
        .map_err(|_| anyhow::anyhow!("book destination reservation lock is poisoned"))?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
    Ok(lock)
}

fn thumbnail_lock(path: &Path) -> anyhow::Result<Arc<tokio::sync::Mutex<()>>> {
    let mut locks = BOOK_THUMBNAIL_LOCKS
        .lock()
        .map_err(|_| anyhow::anyhow!("book thumbnail ownership lock is poisoned"))?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
    Ok(lock)
}

pub trait PdfThumbnailRenderer {
    fn render_thumbnail(&self, pdf_path: &Path) -> Result<image::DynamicImage, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultPdfThumbnailRenderer;

#[cfg(not(feature = "pdf-thumbnails"))]
impl PdfThumbnailRenderer for DefaultPdfThumbnailRenderer {
    fn render_thumbnail(&self, _pdf_path: &Path) -> Result<image::DynamicImage, String> {
        Err("PDF thumbnail rendering is disabled".to_string())
    }
}

#[cfg(feature = "pdf-thumbnails")]
impl PdfThumbnailRenderer for DefaultPdfThumbnailRenderer {
    fn render_thumbnail(&self, pdf_path: &Path) -> Result<image::DynamicImage, String> {
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
        Ok(image::DynamicImage::ImageRgb8(image))
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
    let image = renderer
        .render_thumbnail(pdf_path)
        .map_err(|error| format!("could not render PDF thumbnail: {error}"))?;
    write_jpeg_atomically(&thumbnail_path, &image)?;
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
        publish_thumbnail_temp_no_replace(&temp_path, &file, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temp_path);
    result
}

fn publish_thumbnail_temp_no_replace(
    temp_path: &Path,
    retained_file: &File,
    final_path: &Path,
) -> Result<(), String> {
    let retained_metadata = retained_file
        .metadata()
        .map_err(|error| format!("could not inspect retained temporary thumbnail: {error}"))?;
    if !retained_metadata.is_file() {
        return Err("retained temporary thumbnail must be a regular file".to_string());
    }

    match fs::hard_link(temp_path, final_path) {
        Ok(()) => {
            let published_metadata = match final_path.symlink_metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    return Err(rollback_published_thumbnail(
                        final_path,
                        format!("could not inspect published thumbnail identity: {error}"),
                    ));
                }
            };
            if published_metadata.file_type().is_symlink()
                || !published_metadata.is_file()
                || cap_fs_ext::MetadataExt::dev(&published_metadata)
                    != cap_fs_ext::MetadataExt::dev(&retained_metadata)
                || cap_fs_ext::MetadataExt::ino(&published_metadata)
                    != cap_fs_ext::MetadataExt::ino(&retained_metadata)
            {
                return Err(rollback_published_thumbnail(
                    final_path,
                    "temporary thumbnail changed before publication".to_string(),
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = final_path
                .symlink_metadata()
                .map_err(|inspect_error| {
                    format!("could not inspect existing thumbnail after collision: {inspect_error}")
                })?;
            if existing.file_type().is_symlink() || !existing.is_file() {
                return Err(
                    "existing thumbnail must be a regular file and not a symlink".to_string(),
                );
            }
            Ok(())
        }
        Err(error) => Err(format!(
            "could not publish temporary thumbnail without replacing the final path: {error}"
        )),
    }
}

fn rollback_published_thumbnail(final_path: &Path, error: String) -> String {
    match fs::remove_file(final_path) {
        Ok(()) => error,
        Err(rollback_error) => {
            format!("{error}; additionally failed to remove rejected thumbnail: {rollback_error}")
        }
    }
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
        let temp_path = parent.join(format!(
            ".{filename}.{:032x}.tmp",
            rand::random::<u128>()
        ));
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
    use async_trait::async_trait;
    use base64::Engine as _;
    use lopdf::{dictionary, text_string, Document, Object};
    use std::{
        fs::{self, File},
        io::Write,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            Arc,
        },
    };
    use zip::{write::SimpleFileOptions, ZipWriter};

    use crate::{
        adaptors::{FileSystemStore, SqlRepository},
        domain::{
            algorithm::file_integrity::FileSeal,
            messagebus::{LocalMessageExchange, MessageFilter},
            models::{BookFormat, BookState, CollectionItem, VideoDetails},
            traits::{Databaser, FileStore, FileStorer, Repository, StagedFile, StoreObject},
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
        fn render_thumbnail(&self, _pdf_path: &Path) -> Result<image::DynamicImage, String> {
            Err("test renderer unavailable".to_string())
        }
    }

    struct WritingPdfRenderer;

    impl PdfThumbnailRenderer for WritingPdfRenderer {
        fn render_thumbnail(&self, _pdf_path: &Path) -> Result<image::DynamicImage, String> {
            Ok(image::DynamicImage::ImageRgb8(
                image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3])),
            ))
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

    #[cfg(unix)]
    #[test]
    fn pdf_thumbnail_generation_does_not_follow_preexisting_symlink() {
        use std::os::unix::fs::symlink;

        let temp = TestDir::new();
        let pdf_path = temp.path().join("book.pdf");
        let covers = temp.path().join("covers");
        let target = temp.path().join("target.jpg");
        fs::create_dir_all(&covers).unwrap();
        fs::write(&target, b"preserve target bytes").unwrap();
        symlink(&target, covers.join("pdf-symlink.jpg")).unwrap();
        write_pdf(&pdf_path, None, 1);

        let result = extract_pdf_metadata_with_renderer(
            &pdf_path,
            &covers,
            "pdf-symlink",
            &WritingPdfRenderer,
        )
        .unwrap();

        assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert_eq!(fs::read(&target).unwrap(), b"preserve target bytes");
        assert!(covers
            .join("pdf-symlink.jpg")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(fs::read_dir(&covers).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }

    #[test]
    fn pdf_thumbnail_generation_preserves_preexisting_regular_file_bytes() {
        let temp = TestDir::new();
        let pdf_path = temp.path().join("book.pdf");
        let covers = temp.path().join("covers");
        fs::create_dir_all(&covers).unwrap();
        let existing = covers.join("pdf-existing.jpg");
        fs::write(&existing, b"preexisting PDF thumbnail").unwrap();
        write_pdf(&pdf_path, None, 1);

        let result = extract_pdf_metadata_with_renderer(
            &pdf_path,
            &covers,
            "pdf-existing",
            &WritingPdfRenderer,
        )
        .unwrap();

        assert_eq!(result.thumbnail, "pdf-existing.jpg");
        assert_eq!(fs::read(existing).unwrap(), b"preexisting PDF thumbnail");
        assert!(fs::read_dir(&covers).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
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
    fn no_replace_thumbnail_write_preserves_nonregular_final_and_cleans_temp() {
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

        assert!(error.contains("regular file"));
        assert_eq!(fs::read(final_path.join("marker")).unwrap(), b"preserve me");
        let entries: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from("existing.jpg")]);
    }

    #[test]
    fn thumbnail_publication_rejects_a_swapped_temporary_path() {
        let temp = TestDir::new();
        let final_path = temp.path().join("cover.jpg");
        let (temp_path, mut retained_file) = create_thumbnail_temp_file(&final_path).unwrap();
        retained_file.write_all(b"verified thumbnail bytes").unwrap();
        retained_file.sync_all().unwrap();
        let displaced_path = temp.path().join("displaced-original.tmp");
        fs::rename(&temp_path, &displaced_path).unwrap();
        fs::write(&temp_path, b"attacker replacement bytes").unwrap();

        let result =
            publish_thumbnail_temp_no_replace(&temp_path, &retained_file, &final_path);

        assert!(result.is_err());
        assert!(!final_path.exists());
        assert_eq!(fs::read(&displaced_path).unwrap(), b"verified thumbnail bytes");
        assert_eq!(fs::read(&temp_path).unwrap(), b"attacker replacement bytes");
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

    struct FailingRenameStore {
        inner: FileStorer,
    }

    struct PostSealMutationStore {
        inner: FileStorer,
        seal_calls: AtomicUsize,
    }

    #[cfg(unix)]
    struct SnapshotDirectoryReplacementStore {
        inner: FileStorer,
        book_root: PathBuf,
        replaced: AtomicBool,
        decoy_path: StdMutex<Option<PathBuf>>,
        original_snapshot_path: StdMutex<Option<PathBuf>>,
    }

    struct DestinationRaceStore {
        inner: FileStorer,
    }

    struct SourceSwapStore {
        inner: FileStorer,
        replacement: PathBuf,
        swapped: AtomicBool,
    }

    struct MutatingSnapshotStore {
        inner: FileStorer,
        replacement: PathBuf,
        phase: SnapshotMutationPhase,
        staged_path: StdMutex<Option<PathBuf>>,
        mutated: AtomicBool,
        snapshot_calls: AtomicUsize,
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum SnapshotMutationPhase {
        AfterSnapshotCopy,
        Publication,
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    enum BlockingFilePhase {
        Stage,
        Publication,
    }

    struct BlockingPhaseStore {
        inner: FileStorer,
        phase: BlockingFilePhase,
        started: tokio::sync::Notify,
        releases: tokio::sync::Semaphore,
    }

    struct CleanupAuditStore {
        inner: FileStorer,
        mutate_staged_after_copy: bool,
        fail_thumbnail_cleanup: bool,
        mutated: AtomicBool,
        staged_path: StdMutex<Option<PathBuf>>,
        snapshot_path: StdMutex<Option<PathBuf>>,
        snapshot_calls: AtomicUsize,
        thumbnail_cleanup_calls: AtomicUsize,
        snapshot_cleanup_calls: AtomicUsize,
        restore_calls: AtomicUsize,
    }

    #[async_trait]
    impl FileStore for PostSealMutationStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            self.inner.stage_no_follow(source).await
        }

        async fn create_private_snapshot(&self, staged: &StagedFile) -> anyhow::Result<PrivateSnapshot> {
            self.inner.create_private_snapshot(staged).await
        }

        async fn seal_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<FileSeal> {
            let seal = self.inner.seal_private_snapshot(snapshot).await?;
            self.seal_calls.fetch_add(1, Ordering::SeqCst);
            let modified = std::fs::metadata(&snapshot.path)?.modified().ok();
            let mut bytes = std::fs::read(&snapshot.path)?;
            let last = bytes
                .last_mut()
                .ok_or_else(|| anyhow::anyhow!("test snapshot must not be empty"))?;
            *last ^= 1;
            std::fs::write(&snapshot.path, bytes)?;
            if let Some(modified) = modified {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&snapshot.path)?
                    .set_times(std::fs::FileTimes::new().set_modified(modified))?;
            }
            Ok(seal)
        }

        async fn publish_private_snapshot_no_replace(
            &self,
            snapshot: &PrivateSnapshot,
            destination: &str,
            expected_seal: &FileSeal,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_private_snapshot_no_replace(snapshot, destination, expected_seal)
                .await
        }

        async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(
            &self,
            staged: &StagedFile,
            destination: &str,
        ) -> anyhow::Result<()> {
            self.inner.publish_staged_no_replace(staged, destination).await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    #[cfg(unix)]
    #[async_trait]
    impl FileStore for SnapshotDirectoryReplacementStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            self.inner.stage_no_follow(source).await
        }

        async fn create_private_snapshot(
            &self,
            staged: &StagedFile,
        ) -> anyhow::Result<PrivateSnapshot> {
            let snapshot = self.inner.create_private_snapshot(staged).await?;
            if !self.replaced.swap(true, Ordering::SeqCst) {
                let snapshot_directory = self.book_root.join(".tvserver-book-snapshots");
                let original_directory = self.book_root.join("original-private-snapshots");
                fs::rename(&snapshot_directory, &original_directory)?;
                fs::create_dir(&snapshot_directory)?;
                let name = snapshot.path.file_name().unwrap();
                let decoy = snapshot_directory.join(name);
                fs::write(&decoy, b"decoy must never be ingested or removed")?;
                *self.decoy_path.lock().unwrap() = Some(decoy);
                *self.original_snapshot_path.lock().unwrap() = Some(original_directory.join(name));
            }
            Ok(snapshot)
        }

        async fn seal_private_snapshot(
            &self,
            snapshot: &PrivateSnapshot,
        ) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(
            &self,
            snapshot: &PrivateSnapshot,
            destination: &str,
            expected_seal: &FileSeal,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_private_snapshot_no_replace(snapshot, destination, expected_seal)
                .await
        }

        async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(
            &self,
            staged: &StagedFile,
            destination: &str,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_staged_no_replace(staged, destination)
                .await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    #[async_trait]
    impl FileStore for CleanupAuditStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            let staged = self.inner.stage_no_follow(source).await?;
            *self.staged_path.lock().unwrap() = Some(staged.staged_path.clone());
            Ok(staged)
        }

        async fn create_private_snapshot(
            &self,
            staged: &StagedFile,
        ) -> anyhow::Result<PrivateSnapshot> {
            self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
            let snapshot = self.inner.create_private_snapshot(staged).await?;
            *self.snapshot_path.lock().unwrap() = Some(snapshot.path.clone());
            if self.mutate_staged_after_copy && !self.mutated.swap(true, Ordering::SeqCst) {
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&staged.staged_path)?
                    .write_all(b"mutation")?;
            }
            Ok(snapshot)
        }

        async fn seal_private_snapshot(
            &self,
            snapshot: &PrivateSnapshot,
        ) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(
            &self,
            snapshot: &PrivateSnapshot,
            destination: &str,
            expected_seal: &FileSeal,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_private_snapshot_no_replace(snapshot, destination, expected_seal)
                .await
        }

        async fn remove_private_snapshot(
            &self,
            _snapshot: &PrivateSnapshot,
        ) -> anyhow::Result<()> {
            self.snapshot_cleanup_calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("forced snapshot cleanup failure")
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.thumbnail_cleanup_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_thumbnail_cleanup {
                anyhow::bail!("forced thumbnail cleanup failure")
            }
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(
            &self,
            staged: &StagedFile,
            destination: &str,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_staged_no_replace(staged, destination)
                .await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.restore_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    impl BlockingPhaseStore {
        async fn block(&self, phase: BlockingFilePhase) {
            if self.phase == phase {
                self.started.notify_one();
                self.releases.acquire().await.unwrap().forget();
            }
        }
    }

    #[async_trait]
    impl FileStore for BlockingPhaseStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            self.block(BlockingFilePhase::Stage).await;
            self.inner.stage_no_follow(source).await
        }

        async fn create_private_snapshot(&self, staged: &StagedFile) -> anyhow::Result<PrivateSnapshot> {
            self.inner.create_private_snapshot(staged).await
        }

        async fn seal_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(&self, snapshot: &PrivateSnapshot, destination: &str, expected_seal: &FileSeal) -> anyhow::Result<()> {
            self.block(BlockingFilePhase::Publication).await;
            self.inner.publish_private_snapshot_no_replace(snapshot, destination, expected_seal).await
        }

        async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(&self, staged: &StagedFile, destination: &str) -> anyhow::Result<()> {
            self.inner.publish_staged_no_replace(staged, destination).await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    impl MutatingSnapshotStore {
        async fn mutate_staged_once(&self) -> anyhow::Result<()> {
            if self.mutated.swap(true, Ordering::SeqCst) {
                return Ok(());
            }
            let staged_path = self
                .staged_path
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| anyhow::anyhow!("staged path was not recorded"))?;
            tokio::fs::remove_file(&staged_path).await?;
            tokio::fs::rename(&self.replacement, &staged_path).await?;
            Ok(())
        }
    }

    #[async_trait]
    impl FileStore for MutatingSnapshotStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            let staged = self.inner.stage_no_follow(source).await?;
            *self.staged_path.lock().unwrap() = Some(staged.staged_path.clone());
            Ok(staged)
        }

        async fn create_private_snapshot(&self, staged: &StagedFile) -> anyhow::Result<PrivateSnapshot> {
            self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
            let snapshot = self.inner.create_private_snapshot(staged).await?;
            if self.phase == SnapshotMutationPhase::AfterSnapshotCopy {
                self.mutate_staged_once().await?;
            }
            Ok(snapshot)
        }

        async fn seal_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(&self, snapshot: &PrivateSnapshot, destination: &str, expected_seal: &FileSeal) -> anyhow::Result<()> {
            if self.phase == SnapshotMutationPhase::Publication {
                self.mutate_staged_once().await?;
            }
            self.inner.publish_private_snapshot_no_replace(snapshot, destination, expected_seal).await
        }

        async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(&self, staged: &StagedFile, destination: &str) -> anyhow::Result<()> {
            self.mutate_staged_once().await?;
            self.inner.publish_staged_no_replace(staged, destination).await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    impl SourceSwapStore {
        async fn swap_once(&self, source: &str) -> anyhow::Result<()> {
            if !self.swapped.swap(true, Ordering::SeqCst) {
                tokio::fs::remove_file(source).await?;
                tokio::fs::rename(&self.replacement, source).await?;
            }
            Ok(())
        }
    }

    #[async_trait]
    impl FileStore for SourceSwapStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.swap_once(old_path).await?;
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.swap_once(old_path).await?;
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            self.swap_once(source).await?;
            self.inner.stage_no_follow(source).await
        }

        async fn create_private_snapshot(&self, staged: &StagedFile) -> anyhow::Result<PrivateSnapshot> {
            self.inner.create_private_snapshot(staged).await
        }

        async fn seal_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(&self, snapshot: &PrivateSnapshot, destination: &str, expected_seal: &FileSeal) -> anyhow::Result<()> {
            self.inner.publish_private_snapshot_no_replace(snapshot, destination, expected_seal).await
        }

        async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(
            &self,
            staged: &StagedFile,
            destination: &str,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_staged_no_replace(staged, destination)
                .await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    impl DestinationRaceStore {
        async fn create_competing_destination(&self, path: &str) -> anyhow::Result<()> {
            tokio::fs::write(path, b"external writer bytes").await?;
            Ok(())
        }
    }

    #[async_trait]
    impl FileStore for DestinationRaceStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.create_competing_destination(new_path).await?;
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.create_competing_destination(new_path).await?;
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            self.inner.stage_no_follow(source).await
        }

        async fn create_private_snapshot(&self, staged: &StagedFile) -> anyhow::Result<PrivateSnapshot> {
            self.inner.create_private_snapshot(staged).await
        }

        async fn seal_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(&self, snapshot: &PrivateSnapshot, destination: &str, expected_seal: &FileSeal) -> anyhow::Result<()> {
            self.create_competing_destination(destination).await?;
            self.inner.publish_private_snapshot_no_replace(snapshot, destination, expected_seal).await
        }

        async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(
            &self,
            staged: &StagedFile,
            destination: &str,
        ) -> anyhow::Result<()> {
            self.create_competing_destination(destination).await?;
            self.inner
                .publish_staged_no_replace(staged, destination)
                .await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    #[async_trait]
    impl FileStore for FailingRenameStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, _old_path: &str, _new_path: &str) -> anyhow::Result<()> {
            anyhow::bail!("forced book move failure")
        }

        async fn rename_no_replace(&self, _old_path: &str, _new_path: &str) -> anyhow::Result<()> {
            anyhow::bail!("forced book move failure")
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            self.inner.stage_no_follow(source).await
        }

        async fn create_private_snapshot(&self, staged: &StagedFile) -> anyhow::Result<PrivateSnapshot> {
            self.inner.create_private_snapshot(staged).await
        }

        async fn seal_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(&self, _snapshot: &PrivateSnapshot, _destination: &str, _expected_seal: &FileSeal) -> anyhow::Result<()> {
            anyhow::bail!("forced book move failure")
        }

        async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(
            &self,
            staged: &StagedFile,
            _destination: &str,
        ) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await?;
            anyhow::bail!("forced book move failure")
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    struct FailingSaveRepository {
        inner: Repository,
        barrier: Option<Arc<SaveFailureBarrier>>,
    }

    struct SaveFailureBarrier {
        started: tokio::sync::Notify,
        releases: tokio::sync::Semaphore,
    }

    #[async_trait]
    impl Databaser for FailingSaveRepository {
        async fn save_book(&self, _details: &BookDetails) -> Result<i64, sqlx::Error> {
            if let Some(barrier) = &self.barrier {
                barrier.started.notify_one();
                barrier.releases.acquire().await.unwrap().forget();
            }
            Err(sqlx::Error::Protocol("forced book save failure".to_string()))
        }

        async fn list_book_collections(
            &self,
            collection: &str,
        ) -> Result<Vec<String>, sqlx::Error> {
            self.inner.list_book_collections(collection).await
        }

        async fn list_books(&self, collection: &str) -> Result<Vec<BookDetails>, sqlx::Error> {
            self.inner.list_books(collection).await
        }

        async fn list_all_books(&self) -> Result<Vec<BookDetails>, sqlx::Error> {
            self.inner.list_all_books().await
        }

        async fn retrieve_book(&self, checksum: i64) -> Result<BookDetails, sqlx::Error> {
            self.inner.retrieve_book(checksum).await
        }

        async fn delete_book(&self, checksum: i64) -> Result<u64, sqlx::Error> {
            self.inner.delete_book(checksum).await
        }

        async fn save_video(&self, details: &VideoDetails) -> Result<i64, sqlx::Error> {
            self.inner.save_video(details).await
        }

        async fn list_collection(&self, collection: &str) -> Result<Vec<String>, sqlx::Error> {
            self.inner.list_collection(collection).await
        }

        async fn list_videos(&self, collection: &str) -> Result<Vec<VideoDetails>, sqlx::Error> {
            self.inner.list_videos(collection).await
        }

        async fn list_all_series(&self) -> Result<Vec<CollectionItem>, sqlx::Error> {
            self.inner.list_all_series().await
        }

        async fn list_series_details(
            &self,
            series: &str,
            season: Option<&str>,
        ) -> Result<Vec<VideoDetails>, sqlx::Error> {
            self.inner.list_series_details(series, season).await
        }

        async fn retrieve_video(&self, checksum: i64) -> Result<VideoDetails, sqlx::Error> {
            self.inner.retrieve_video(checksum).await
        }

        async fn delete_video(&self, checksum: i64) -> Result<u64, sqlx::Error> {
            self.inner.delete_video(checksum).await
        }

        async fn update_watched_video(
            &self,
            checksum: i64,
            current_time: f64,
        ) -> Result<(), sqlx::Error> {
            self.inner
                .update_watched_video(checksum, current_time)
                .await
        }

        async fn get_history(
            &self,
            offset: i32,
            limit: i32,
        ) -> Result<Vec<VideoDetails>, sqlx::Error> {
            self.inner.get_history(offset, limit).await
        }

        async fn list_all_videos(&self) -> Result<Vec<VideoDetails>, sqlx::Error> {
            self.inner.list_all_videos().await
        }
    }

    async fn repository_with_book_listener(
    ) -> (Repository, crate::domain::messages::LocalMessageReceiver) {
        let exchange = LocalMessageExchange::new();
        let receiver = exchange
            .listen_for_messages(MessageFilter::Book)
            .await
            .unwrap();
        let repository: Repository = Arc::new(
            SqlRepository::new(":memory:", Some(exchange.new_sender()))
                .await
                .unwrap(),
        );
        (repository, receiver)
    }

    async fn assert_no_book_event(receiver: &mut crate::domain::messages::LocalMessageReceiver) {
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), receiver.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn ingestion_malformed_partial_epub_retains_only_filename_fallback_fields() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("fallback_title.epub");
        write_epub(
            &source,
            "<package><metadata><title>Leaked Title</title><creator>Leaked Author</creator></metadata><manifest>",
            &[],
        );
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
        .unwrap();

        assert_eq!(details.title, "fallback title");
        assert!(details.authors.is_empty());
        assert_eq!(details.thumbnail, DEFAULT_BOOK_THUMBNAIL);
        assert_eq!(details.state, BookState::MetadataError);
        assert!(details.metadata.extraction_error.is_some());
        let saved = repository.retrieve_book(details.checksum).await.unwrap();
        assert_eq!(saved.title, "fallback title");
        assert!(saved.authors.is_empty());
    }

    #[tokio::test]
    async fn ingestion_concurrent_destination_collision_has_one_winner_without_overwrite() {
        let temp = TestDir::new();
        let first_dir = temp.path().join("first");
        let second_dir = temp.path().join("second");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&first_dir).unwrap();
        fs::create_dir_all(&second_dir).unwrap();
        let first = first_dir.join("shared.pdf");
        let second = second_dir.join("shared.pdf");
        write_pdf(&first, Some(dictionary! { "Title" => text_string("First") }), 1);
        write_pdf(
            &second,
            Some(dictionary! { "Title" => text_string("Second") }),
            2,
        );
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let (first_result, second_result) = tokio::join!(
            generate_book_metadata_with_roots(
                first.clone(),
                storer.clone(),
                repository.clone(),
                Some("collision".to_string()),
                book_root.clone(),
                thumbnail_root.clone(),
            ),
            generate_book_metadata_with_roots(
                second.clone(),
                storer,
                repository.clone(),
                Some("collision".to_string()),
                book_root.clone(),
                thumbnail_root,
            )
        );

        let (winner, loser_source) = match (first_result, second_result) {
            (Ok(Some(winner)), Err(_)) => (winner, second),
            (Err(_), Ok(Some(winner))) => (winner, first),
            results => panic!("expected exactly one collision winner, got {results:?}"),
        };
        let destination = book_root.join("Collision/shared.pdf");
        assert!(destination.exists());
        assert!(loser_source.exists());
        assert_eq!(
            super::super::video_metadata::calculate_checksum(&destination)
                .await
                .unwrap(),
            winner.checksum
        );
        let saved = repository.retrieve_book(winner.checksum).await.unwrap();
        assert_eq!(saved.title, winner.title);
        assert_eq!(repository.list_all_books().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn ingestion_external_destination_race_preserves_both_existing_destination_and_source() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("raced.pdf");
        write_pdf(
            &source,
            Some(dictionary! { "Title" => text_string("Incoming") }),
            1,
        );
        let original_source = fs::read(&source).unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let storer: FileStorer = Arc::new(DestinationRaceStore { inner });

        let result = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            Some("collision".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(fs::read(&source).unwrap(), original_source);
        assert_eq!(
            fs::read(book_root.join("Collision/raced.pdf")).unwrap(),
            b"external writer bytes"
        );
        assert!(repository.list_all_books().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn ingestion_same_size_path_replacement_processes_and_publishes_one_identity() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("identity.pdf");
        let replacement = source_dir.join("replacement.pdf");
        write_pdf(
            &source,
            Some(dictionary! { "Title" => text_string("Original Identity") }),
            1,
        );
        write_pdf(
            &replacement,
            Some(dictionary! { "Title" => text_string("Replacement Identity") }),
            2,
        );
        let original_len = fs::metadata(&source).unwrap().len();
        let replacement_len = fs::metadata(&replacement).unwrap().len();
        let target_len = original_len.max(replacement_len);
        for path in [&source, &replacement] {
            let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
            let padding = target_len - fs::metadata(path).unwrap().len();
            file.write_all(&vec![b' '; padding as usize]).unwrap();
        }
        assert_eq!(fs::metadata(&source).unwrap().len(), fs::metadata(&replacement).unwrap().len());
        let replacement_bytes = fs::read(&replacement).unwrap();
        let expected_checksum = super::super::video_metadata::calculate_checksum(&replacement)
            .await
            .unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let storer: FileStorer = Arc::new(SourceSwapStore {
            inner,
            replacement,
            swapped: AtomicBool::new(false),
        });

        let details = generate_book_metadata_with_roots(
            source,
            storer,
            repository.clone(),
            Some("identity".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .unwrap();

        let destination = book_root.join("Identity/identity.pdf");
        assert_eq!(details.title, "Replacement Identity");
        assert_eq!(details.page_count, Some(2));
        assert_eq!(details.checksum, expected_checksum);
        assert_eq!(fs::read(&destination).unwrap(), replacement_bytes);
        assert_eq!(
            super::super::video_metadata::calculate_checksum(&destination)
                .await
                .unwrap(),
            details.checksum
        );
        assert_eq!(repository.retrieve_book(details.checksum).await.unwrap().title, details.title);
    }

    #[tokio::test]
    async fn ingestion_publishes_private_snapshot_when_staged_path_is_replaced_at_commit() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("immutable.pdf");
        let replacement = source_dir.join("replacement.pdf");
        write_pdf(
            &source,
            Some(dictionary! { "Title" => text_string("Snapshot Identity") }),
            1,
        );
        write_pdf(
            &replacement,
            Some(dictionary! { "Title" => text_string("Late Replacement") }),
            2,
        );
        let target_len = fs::metadata(&source)
            .unwrap()
            .len()
            .max(fs::metadata(&replacement).unwrap().len());
        for path in [&source, &replacement] {
            let padding = target_len - fs::metadata(path).unwrap().len();
            std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(&vec![b' '; padding as usize])
                .unwrap();
        }
        let original_bytes = fs::read(&source).unwrap();
        let expected_checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(MutatingSnapshotStore {
            inner,
            replacement,
            phase: SnapshotMutationPhase::Publication,
            staged_path: StdMutex::new(None),
            mutated: AtomicBool::new(false),
            snapshot_calls: AtomicUsize::new(0),
        });
        let storer: FileStorer = store.clone();

        let details = generate_book_metadata_with_roots(
            source,
            storer,
            repository.clone(),
            Some("immutable".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .unwrap();

        let destination = book_root.join("Immutable/immutable.pdf");
        assert_eq!(store.snapshot_calls.load(Ordering::SeqCst), 1);
        assert_eq!(details.title, "Snapshot Identity");
        assert_eq!(details.page_count, Some(1));
        assert_eq!(details.checksum, expected_checksum);
        assert_eq!(fs::read(&destination).unwrap(), original_bytes);
        assert_eq!(repository.retrieve_book(details.checksum).await.unwrap().title, details.title);
    }

    #[tokio::test]
    async fn ingestion_retries_staged_replacement_at_snapshot_copy_boundary_and_cleans_snapshot() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("copy-boundary.epub");
        let replacement = source_dir.join("replacement.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata><dc:title>Discarded Snapshot</dc:title></metadata><manifest/></package>"#,
            &[],
        );
        write_epub(
            &replacement,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata><dc:title>Stable Replacement</dc:title></metadata><manifest/></package>"#,
            &[],
        );
        let replacement_bytes = fs::read(&replacement).unwrap();
        let expected_checksum = super::super::video_metadata::calculate_checksum(&replacement)
            .await
            .unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(MutatingSnapshotStore {
            inner,
            replacement,
            phase: SnapshotMutationPhase::AfterSnapshotCopy,
            staged_path: StdMutex::new(None),
            mutated: AtomicBool::new(false),
            snapshot_calls: AtomicUsize::new(0),
        });

        let details = generate_book_metadata_with_roots(
            source,
            store.clone(),
            repository,
            Some("copy boundary".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(store.snapshot_calls.load(Ordering::SeqCst), 2);
        assert_eq!(details.title, "Stable Replacement");
        assert_eq!(details.checksum, expected_checksum);
        assert_eq!(
            fs::read(book_root.join("Copy Boundary/copy-boundary.epub")).unwrap(),
            replacement_bytes
        );
        let snapshot_dir = book_root.join(".tvserver-book-snapshots");
        assert!(fs::read_dir(snapshot_dir).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn ingestion_rejects_snapshot_mutated_after_post_extraction_seal() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("mutated.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", png_bytes().as_slice())],
        );
        let original_source = fs::read(&source).unwrap();
        let checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(PostSealMutationStore {
            inner,
            seal_calls: AtomicUsize::new(0),
        });

        let result = generate_book_metadata_with_roots(
            source.clone(),
            store.clone(),
            repository.clone(),
            Some("sealed".to_string()),
            book_root.clone(),
            thumbnail_root.clone(),
        )
        .await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains("integrity"), "{error}");
        assert_eq!(store.seal_calls.load(Ordering::SeqCst), 1);
        assert!(!book_root.join("Sealed/mutated.epub").exists());
        assert_eq!(fs::read(&source).unwrap(), original_source);
        assert!(repository.list_all_books().await.unwrap().is_empty());
        assert!(!thumbnail_root.join(format!("{checksum}.jpg")).exists());
        let snapshot_dir = book_root.join(".tvserver-book-snapshots");
        assert!(fs::read_dir(snapshot_dir).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ingestion_rejects_replaced_snapshot_path_and_cleans_original_capability() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("replaced.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest/></package>"#,
            &[],
        );
        let original_source = fs::read(&source).unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(SnapshotDirectoryReplacementStore {
            inner,
            book_root: book_root.clone(),
            replaced: AtomicBool::new(false),
            decoy_path: StdMutex::new(None),
            original_snapshot_path: StdMutex::new(None),
        });

        let result = generate_book_metadata_with_roots(
            source.clone(),
            store.clone(),
            repository.clone(),
            Some("replacement".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(fs::read(&source).unwrap(), original_source);
        assert!(repository.list_all_books().await.unwrap().is_empty());
        assert!(!book_root.join("Replacement/replaced.epub").exists());
        let original = store.original_snapshot_path.lock().unwrap().clone().unwrap();
        let decoy = store.decoy_path.lock().unwrap().clone().unwrap();
        assert!(!original.exists());
        assert_eq!(fs::read(decoy).unwrap(), b"decoy must never be ingested or removed");
    }

    #[tokio::test]
    async fn snapshot_copy_cleanup_failure_attempts_snapshot_cleanup_and_source_restore_once() {
        let temp = TestDir::new();
        let source = temp.path().join("copy-cleanup-failure.epub");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest/></package>"#,
            &[],
        );
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(CleanupAuditStore {
            inner,
            mutate_staged_after_copy: true,
            fail_thumbnail_cleanup: false,
            mutated: AtomicBool::new(false),
            staged_path: StdMutex::new(None),
            snapshot_path: StdMutex::new(None),
            snapshot_calls: AtomicUsize::new(0),
            thumbnail_cleanup_calls: AtomicUsize::new(0),
            snapshot_cleanup_calls: AtomicUsize::new(0),
            restore_calls: AtomicUsize::new(0),
        });

        let result = generate_book_metadata_with_roots(
            source.clone(),
            store.clone(),
            repository.clone(),
            None,
            book_root,
            thumbnail_root,
        )
        .await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains("changed during private snapshot copy"), "{error}");
        assert!(error.contains("forced snapshot cleanup failure"), "{error}");
        assert_eq!(store.snapshot_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.thumbnail_cleanup_calls.load(Ordering::SeqCst), 0);
        assert_eq!(store.snapshot_cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.restore_calls.load(Ordering::SeqCst), 1);
        assert!(source.exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn ingestion_retries_one_transient_size_change_and_then_succeeds() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("growing.pdf");
        write_pdf(
            &source,
            Some(dictionary! { "Title" => text_string("Eventually Stable") }),
            1,
        );
        let mut writer = std::fs::OpenOptions::new().append(true).open(&source).unwrap();
        let mutation = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            writer.write_all(b" ").unwrap();
            writer.flush().unwrap();
        });
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let details = generate_book_metadata_with_roots(
            source,
            storer,
            repository,
            Some("retry".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .expect("a transiently changing source must be retried, not silently completed");
        mutation.await.unwrap();

        let destination = book_root.join("Retry/growing.pdf");
        assert_eq!(details.title, "Eventually Stable");
        assert!(destination.exists());
        assert_eq!(
            super::super::video_metadata::calculate_checksum(&destination)
                .await
                .unwrap(),
            details.checksum
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ingestion_rejects_symlink_replacement_before_any_stability_or_extraction_read() {
        use std::os::unix::fs::symlink;

        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("swapped.epub");
        let target = source_dir.join("secret-target.epub");
        let replacement_link = source_dir.join("replacement-link.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest/></package>"#,
            &[],
        );
        write_epub(
            &target,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata><dc:title>Secret Target</dc:title></metadata><manifest/></package>"#,
            &[],
        );
        let target_bytes = fs::read(&target).unwrap();
        symlink(&target, &replacement_link).unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let storer: FileStorer = Arc::new(SourceSwapStore {
            inner,
            replacement: replacement_link,
            swapped: AtomicBool::new(false),
        });

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(400),
            generate_book_metadata_with_roots(
                source.clone(),
                storer,
                repository.clone(),
                Some("security".to_string()),
                book_root.clone(),
                thumbnail_root,
            ),
        )
        .await
        .expect("source staging must happen before the 500 ms stability wait");

        assert!(result.is_err());
        assert!(source.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&target).unwrap(), target_bytes);
        assert!(!book_root.join("Security/swapped.epub").exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn generated_epub_thumbnail_is_removed_when_move_fails() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("covered.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", png_bytes().as_slice())],
        );
        let checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let storer: FileStorer = Arc::new(FailingRenameStore { inner });

        let result = generate_book_metadata_with_roots(
            source,
            storer,
            repository,
            Some("cleanup".to_string()),
            book_root,
            thumbnail_root.clone(),
        )
        .await;

        assert!(result.is_err());
        assert!(!thumbnail_root.join(format!("{checksum}.jpg")).exists());
    }

    #[tokio::test]
    async fn generated_epub_thumbnail_is_removed_when_save_fails_but_preexisting_is_preserved() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("covered.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", png_bytes().as_slice())],
        );
        let checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        fs::create_dir_all(&thumbnail_root).unwrap();
        let preexisting = thumbnail_root.join(format!("{checksum}.jpg"));
        fs::write(&preexisting, b"preexisting thumbnail").unwrap();
        let (storer, inner) = ingestion_dependencies(&book_root).await;
        let repository: Repository = Arc::new(FailingSaveRepository {
            inner,
            barrier: None,
        });

        let result = generate_book_metadata_with_roots(
            source,
            storer,
            repository,
            Some("cleanup".to_string()),
            book_root,
            thumbnail_root,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(fs::read(&preexisting).unwrap(), b"preexisting thumbnail");
    }

    #[tokio::test]
    async fn generated_epub_thumbnail_is_removed_when_save_fails() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("covered.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", png_bytes().as_slice())],
        );
        let checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        let (storer, inner) = ingestion_dependencies(&book_root).await;
        let repository: Repository = Arc::new(FailingSaveRepository {
            inner,
            barrier: None,
        });

        let result = generate_book_metadata_with_roots(
            source,
            storer,
            repository,
            Some("cleanup".to_string()),
            book_root,
            thumbnail_root.clone(),
        )
        .await;

        assert!(result.is_err());
        assert!(!thumbnail_root.join(format!("{checksum}.jpg")).exists());
    }

    #[tokio::test]
    async fn concurrent_same_checksum_failure_never_deletes_successful_cover() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let failing_source = source_dir.join("failing.epub");
        let successful_source = source_dir.join("successful.epub");
        write_epub(
            &failing_source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", png_bytes().as_slice())],
        );
        fs::copy(&failing_source, &successful_source).unwrap();
        let checksum = super::super::video_metadata::calculate_checksum(&failing_source)
            .await
            .unwrap();
        let (storer, repository) = ingestion_dependencies(&book_root).await;
        let barrier = Arc::new(SaveFailureBarrier {
            started: tokio::sync::Notify::new(),
            releases: tokio::sync::Semaphore::new(0),
        });
        let failing_repository: Repository = Arc::new(FailingSaveRepository {
            inner: repository.clone(),
            barrier: Some(barrier.clone()),
        });
        let failing_started = barrier.started.notified();
        let failing = tokio::spawn(generate_book_metadata_with_roots(
            failing_source,
            storer.clone(),
            failing_repository,
            Some("failed".to_string()),
            book_root.clone(),
            thumbnail_root.clone(),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(3), failing_started)
            .await
            .unwrap();

        let mut successful = tokio::spawn(generate_book_metadata_with_roots(
            successful_source,
            storer,
            repository.clone(),
            Some("successful".to_string()),
            book_root,
            thumbnail_root.clone(),
        ));
        let early_success = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            &mut successful,
        )
        .await
        .ok()
        .map(|joined| joined.unwrap().unwrap().unwrap());

        barrier.releases.add_permits(1);
        assert!(failing.await.unwrap().is_err());
        let details = match early_success {
            Some(details) => details,
            None => successful.await.unwrap().unwrap().unwrap(),
        };

        assert_eq!(details.checksum, checksum);
        assert_eq!(details.thumbnail, format!("{checksum}.jpg"));
        assert!(thumbnail_root.join(&details.thumbnail).exists());
        assert_eq!(repository.retrieve_book(checksum).await.unwrap().thumbnail, details.thumbnail);
    }

    #[tokio::test]
    async fn ingestion_persists_nested_collection_as_portable_identifier() {
        let temp = TestDir::new();
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        let source = book_root.join("Fiction").join("Classics").join("Emma.epub");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest/></package>"#,
            &[],
        );
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let details = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            None,
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(details.collection, "Fiction/Classics");
        assert_eq!(
            repository.retrieve_book(details.checksum).await.unwrap().collection,
            "Fiction/Classics"
        );
        assert!(book_root.join("Fiction").join("Classics").join("Emma.epub").exists());
    }

    #[tokio::test]
    async fn ingestion_persists_nested_suggested_collection_and_publishes_native_path() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Emma.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest/></package>"#,
            &[],
        );
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let details = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            Some("Fiction/Classics".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .unwrap();

        let destination = book_root
            .join("Fiction")
            .join("Classics")
            .join("Emma.epub");
        assert_eq!(details.collection, "Fiction/Classics");
        assert_eq!(
            repository.retrieve_book(details.checksum).await.unwrap().collection,
            "Fiction/Classics"
        );
        assert!(destination.exists());
        assert!(!source.exists());
    }

    #[tokio::test]
    async fn ingestion_rejects_backslash_suggested_collection_before_publication_or_persistence() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("Emma.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest/></package>"#,
            &[],
        );
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let result = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            Some(r"Fiction\Classics".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await;

        assert!(result.is_err());
        assert!(source.exists());
        assert!(!book_root.join(r"Fiction\Classics").exists());
        assert!(!book_root.join("Fiction").exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
    }

    #[test]
    fn ingestion_parentless_relative_source_targets_book_root() {
        let book_root = Path::new("configured-books");
        let source = Path::new("book.pdf");

        let collection = collection_from_source(source, book_root).unwrap();
        let destination = book_root
            .join(&collection)
            .join(source.file_name().unwrap());

        assert_eq!(collection, "");
        assert_eq!(destination, book_root.join("book.pdf"));
    }

    #[tokio::test]
    async fn ingestion_zero_byte_input_creates_no_row_or_event() {
        let temp = TestDir::new();
        let source = temp.path().join("empty.pdf");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::write(&source, []).unwrap();
        let inner_storer: FileStorer = Arc::new(FileSystemStore::new(book_root.to_str().unwrap()));
        let (repository, mut receiver) = repository_with_book_listener().await;

        let result = generate_book_metadata_with_roots(
            source.clone(),
            inner_storer,
            repository.clone(),
            None,
            book_root.clone(),
            thumbnail_root,
        )
        .await;

        assert!(result.unwrap_err().to_string().contains("zero-byte"));
        assert!(source.exists());
        assert!(!book_root.join("empty.pdf").exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
        assert_no_book_event(&mut receiver).await;
    }

    #[tokio::test]
    async fn ingestion_cancelled_before_staging_leaves_source_and_repository_untouched() {
        let temp = TestDir::new();
        let source = temp.path().join("cancelled.pdf");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_pdf(&source, None, 1);
        let original = fs::read(&source).unwrap();
        let (storer, repository) = ingestion_dependencies(&book_root).await;
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();

        let result = generate_book_metadata_with_roots_and_cancellation(
            source.clone(),
            storer,
            repository.clone(),
            None,
            book_root.clone(),
            thumbnail_root,
            cancellation,
        )
        .await;

        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(!book_root.join("cancelled.pdf").exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancellation_during_blocked_stage_waits_then_restores_without_persistence() {
        let temp = TestDir::new();
        let source = temp.path().join("stage-blocked.pdf");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_pdf(&source, None, 1);
        let original = fs::read(&source).unwrap();
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(BlockingPhaseStore {
            inner,
            phase: BlockingFilePhase::Stage,
            started: tokio::sync::Notify::new(),
            releases: tokio::sync::Semaphore::new(0),
        });
        let cancellation = CancellationToken::new();
        let started = store.started.notified();
        let mut ingestion = tokio::spawn(generate_book_metadata_with_roots_and_cancellation(
            source.clone(),
            store.clone(),
            repository.clone(),
            None,
            book_root.clone(),
            thumbnail_root,
            cancellation.clone(),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(2), started)
            .await
            .unwrap();

        cancellation.cancel();
        assert!(tokio::time::timeout(std::time::Duration::from_millis(50), &mut ingestion)
            .await
            .is_err());
        store.releases.add_permits(1);
        let error = ingestion.await.unwrap().unwrap_err();

        assert!(error.to_string().contains("cancelled"));
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(repository.list_all_books().await.unwrap().is_empty());
        let snapshot_dir = book_root.join(".tvserver-book-snapshots");
        assert!(!snapshot_dir.exists() || fs::read_dir(snapshot_dir).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn cancellation_during_blocked_extraction_waits_then_cleans_before_returning() {
        let temp = TestDir::new();
        let source = temp.path().join("extraction-blocked.epub");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", png_bytes().as_slice())],
        );
        let checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        let (storer, repository) = ingestion_dependencies(&book_root).await;
        let barrier = install_extraction_test_barrier(&thumbnail_root);
        let started = barrier.started.notified();
        let cancellation = CancellationToken::new();
        let mut ingestion = tokio::spawn(generate_book_metadata_with_roots_and_cancellation(
            source.clone(),
            storer,
            repository.clone(),
            None,
            book_root.clone(),
            thumbnail_root.clone(),
            cancellation.clone(),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(3), started)
            .await
            .unwrap();

        cancellation.cancel();
        assert!(tokio::time::timeout(std::time::Duration::from_millis(50), &mut ingestion)
            .await
            .is_err());
        barrier.release();
        let error = ingestion.await.unwrap().unwrap_err();

        assert!(error.to_string().contains("cancelled"));
        assert!(source.exists());
        assert!(!thumbnail_root.join(format!("{checksum}.jpg")).exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
        let snapshot_dir = book_root.join(".tvserver-book-snapshots");
        assert!(!snapshot_dir.exists() || fs::read_dir(snapshot_dir).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn retained_downloader_fd_write_during_extraction_cannot_change_snapshot_or_final_bytes() {
        use std::io::{Seek, SeekFrom};

        let temp = TestDir::new();
        let source = temp.path().join("retained-fd.epub");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0"><metadata><dc:title>Immutable Snapshot</dc:title></metadata><manifest/></package>"#,
            &[],
        );
        let original = fs::read(&source).unwrap();
        let expected_checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        let mut retained_fd = std::fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap();
        let (storer, repository) = ingestion_dependencies(&book_root).await;
        let barrier = install_extraction_test_barrier(&thumbnail_root);
        let started = barrier.started.notified();
        let ingestion = tokio::spawn(generate_book_metadata_with_roots(
            source,
            storer,
            repository,
            Some("immutable".to_string()),
            book_root.clone(),
            thumbnail_root,
        ));
        tokio::time::timeout(std::time::Duration::from_secs(3), started)
            .await
            .unwrap();

        retained_fd.seek(SeekFrom::Start(0)).unwrap();
        retained_fd.write_all(b"MUTATED!").unwrap();
        retained_fd.flush().unwrap();
        barrier.release();
        let details = ingestion.await.unwrap().unwrap().unwrap();

        let destination = book_root.join("Immutable/retained-fd.epub");
        assert_eq!(details.title, "Immutable Snapshot");
        assert_eq!(details.checksum, expected_checksum);
        assert_eq!(fs::read(destination).unwrap(), original);
    }

    #[tokio::test]
    async fn post_extraction_cleanup_failures_are_aggregated_and_source_is_restored() {
        let temp = TestDir::new();
        let source = temp.path().join("verification-cleanup-failure.epub");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
                 <metadata/><manifest><item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/></manifest>
               </package>"#,
            &[("OPS/cover.png", png_bytes().as_slice())],
        );
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(CleanupAuditStore {
            inner,
            mutate_staged_after_copy: false,
            fail_thumbnail_cleanup: true,
            mutated: AtomicBool::new(false),
            staged_path: StdMutex::new(None),
            snapshot_path: StdMutex::new(None),
            snapshot_calls: AtomicUsize::new(0),
            thumbnail_cleanup_calls: AtomicUsize::new(0),
            snapshot_cleanup_calls: AtomicUsize::new(0),
            restore_calls: AtomicUsize::new(0),
        });
        let barrier = install_post_extraction_test_barrier(&thumbnail_root);
        let started = barrier.started.notified();
        let ingestion = tokio::spawn(generate_book_metadata_with_roots(
            source.clone(),
            store.clone(),
            repository.clone(),
            None,
            book_root,
            thumbnail_root,
        ));
        tokio::time::timeout(std::time::Duration::from_secs(3), started)
            .await
            .unwrap();
        let snapshot_path = store.snapshot_path.lock().unwrap().clone().unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(snapshot_path)
            .unwrap()
            .write_all(b"mutation")
            .unwrap();
        barrier.release();

        let error = ingestion.await.unwrap().unwrap_err().to_string();
        assert!(
            error.contains("changed during metadata extraction"),
            "{error}"
        );
        assert!(error.contains("forced thumbnail cleanup failure"), "{error}");
        assert!(error.contains("forced snapshot cleanup failure"), "{error}");
        assert_eq!(store.snapshot_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.thumbnail_cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.snapshot_cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.restore_calls.load(Ordering::SeqCst), 1);
        assert!(source.exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancellation_during_blocked_publication_finishes_publish_and_save() {
        let temp = TestDir::new();
        let source = temp.path().join("publication-blocked.pdf");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_pdf(
            &source,
            Some(dictionary! { "Title" => text_string("Committed During Shutdown") }),
            1,
        );
        let (inner, repository) = ingestion_dependencies(&book_root).await;
        let store = Arc::new(BlockingPhaseStore {
            inner,
            phase: BlockingFilePhase::Publication,
            started: tokio::sync::Notify::new(),
            releases: tokio::sync::Semaphore::new(0),
        });
        let cancellation = CancellationToken::new();
        let started = store.started.notified();
        let mut ingestion = tokio::spawn(generate_book_metadata_with_roots_and_cancellation(
            source.clone(),
            store.clone(),
            repository.clone(),
            Some("committed".to_string()),
            book_root.clone(),
            thumbnail_root,
            cancellation.clone(),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(3), started)
            .await
            .unwrap();

        cancellation.cancel();
        assert!(tokio::time::timeout(std::time::Duration::from_millis(50), &mut ingestion)
            .await
            .is_err());
        store.releases.add_permits(1);
        let details = ingestion.await.unwrap().unwrap().unwrap();

        assert!(!source.exists());
        assert!(book_root.join("Committed/publication-blocked.pdf").exists());
        assert_eq!(
            repository.retrieve_book(details.checksum).await.unwrap().title,
            "Committed During Shutdown"
        );
    }

    #[tokio::test]
    async fn ingestion_move_failure_propagates_without_row_or_event() {
        let temp = TestDir::new();
        let source = temp.path().join("move-failure.pdf");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_pdf(&source, None, 1);
        let inner_storer: FileStorer = Arc::new(FileSystemStore::new(book_root.to_str().unwrap()));
        let storer: FileStorer = Arc::new(FailingRenameStore {
            inner: inner_storer,
        });
        let (repository, mut receiver) = repository_with_book_listener().await;

        let result = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            Some("failures".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("forced book move failure"));
        assert!(source.exists());
        assert!(!book_root.join("Failures/move-failure.pdf").exists());
        assert!(repository.list_all_books().await.unwrap().is_empty());
        assert_no_book_event(&mut receiver).await;
    }

    #[tokio::test]
    async fn ingestion_save_failure_leaves_moved_orphan_without_row_or_event() {
        let temp = TestDir::new();
        let source = temp.path().join("save-failure.pdf");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        write_pdf(&source, None, 1);
        let storer: FileStorer = Arc::new(FileSystemStore::new(book_root.to_str().unwrap()));
        let (inner, mut receiver) = repository_with_book_listener().await;
        let repository: Repository = Arc::new(FailingSaveRepository {
            inner: inner.clone(),
            barrier: None,
        });

        let result = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository,
            Some("failures".to_string()),
            book_root.clone(),
            thumbnail_root.clone(),
        )
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("forced book save failure"));
        assert!(!source.exists());
        assert!(book_root.join("Failures/save-failure.pdf").exists());
        assert!(thumbnail_root.join(DEFAULT_BOOK_THUMBNAIL).exists());
        assert!(inner.list_all_books().await.unwrap().is_empty());
        assert_no_book_event(&mut receiver).await;
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
    async fn identical_second_ingestion_keeps_first_file_and_row_canonical() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let first_source = source_dir.join("first.epub");
        let second_source = source_dir.join("second.epub");
        write_epub(
            &first_source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata><dc:title>First Canonical Copy</dc:title></metadata><manifest/>
               </package>"#,
            &[],
        );
        fs::copy(&first_source, &second_source).unwrap();
        let (storer, repository) = ingestion_dependencies(&book_root).await;

        let first = generate_book_metadata_with_roots(
            first_source,
            storer.clone(),
            repository.clone(),
            Some("originals".to_string()),
            book_root.clone(),
            thumbnail_root.clone(),
        )
        .await
        .unwrap()
        .unwrap();
        let canonical = repository.retrieve_book(first.checksum).await.unwrap();
        let second = generate_book_metadata_with_roots(
            second_source.clone(),
            storer,
            repository.clone(),
            Some("reprints".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(second, canonical);
        assert!(book_root.join("Originals/first.epub").exists());
        assert!(!book_root.join("Reprints/second.epub").exists());
        assert!(!second_source.exists());
        let saved = repository.retrieve_book(first.checksum).await.unwrap();
        assert_eq!(saved.collection, "Originals");
        assert_eq!(saved.file_name, "first.epub");
    }

    #[tokio::test]
    async fn identical_ingestion_repairs_checksum_row_when_canonical_file_is_missing() {
        let temp = TestDir::new();
        let source_dir = temp.path().join("downloads");
        let book_root = temp.path().join("books");
        let thumbnail_root = temp.path().join("book-thumbnails");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("repair.epub");
        write_epub(
            &source,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
                 <metadata><dc:title>Repaired Copy</dc:title></metadata><manifest/>
               </package>"#,
            &[],
        );
        let checksum = super::super::video_metadata::calculate_checksum(&source)
            .await
            .unwrap();
        let (storer, repository) = ingestion_dependencies(&book_root).await;
        let mut stale = BookDetails::new(
            "missing.epub".to_string(),
            "Originals".to_string(),
            &book_root.join("Originals/missing.epub"),
            BookFormat::Epub,
        );
        stale.checksum = checksum;
        repository.save_book(&stale).await.unwrap();

        let repaired = generate_book_metadata_with_roots(
            source.clone(),
            storer,
            repository.clone(),
            Some("reprints".to_string()),
            book_root.clone(),
            thumbnail_root,
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(repaired.checksum, checksum);
        assert!(!source.exists());
        assert!(!book_root.join("Originals/missing.epub").exists());
        assert!(book_root.join("Reprints/repair.epub").exists());
        let saved = repository.retrieve_book(checksum).await.unwrap();
        assert_eq!(saved.collection, "Reprints");
        assert_eq!(saved.file_name, "repair.epub");
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
