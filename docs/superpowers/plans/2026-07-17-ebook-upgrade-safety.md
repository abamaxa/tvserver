# Ebook Upgrade Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent ebook support from relocating legacy movie-library files, exhausting the server through PDF parsing, freezing on isolated filesystem defects, breaking stable Windows builds, or taking down video-only deployments when book storage is unavailable.

**Architecture:** Keep movie scanning and completed-download routing as separate admission paths, replace rich PDF parsing with a safe metadata fallback, and keep strict filesystem failures scoped to the smallest affected entry or database row. Represent initialized book services as one `BookRuntime` value in `Context`; unavailable book storage keeps video services alive and makes every book surface return a stable unavailable response.

**Tech Stack:** Rust 2021, Tokio, Axum, cap-std/cap-fs-ext, SQLx/SQLite, Tauri, Docker Compose, Cargo tests.

## Global Constraints

- `BOOK_DIR` defaults to lowercase `books` beside `MOVIE_DIR`; Docker Compose explicitly overrides it to `/Books`.
- EPUB and PDF files already inside `MOVIE_DIR` must never be queued by the periodic video scanner.
- Extensionless files retain legacy video-scan behavior.
- Completed EPUB/PDF download events must continue to route to book ingestion.
- PDF ingestion must not invoke lopdf or Pdfium in the server process.
- Book-storage initialization failure must not fail `Context`, router, monitor, metadata manager, or any video operation.
- Every unavailable HTTP book surface returns `503 Service Unavailable`; Tauri uses the stable text `book library unavailable`.
- Suspicious filesystem state remains fail-closed for deletion.
- Production code must compile on stable Windows without calling `cap_fs_ext::MetadataExt` on `std::fs::Metadata`.
- Do not change checksum identity, CORS/authentication, EPUB parsing, or unrelated review findings.

---

### Task 1: Separate Movie Scan Admission From Download Routing

**Files:**
- Modify: `src/domain/algorithm/video_utils.rs:41-105`
- Modify: `src/domain/algorithm/mod.rs`
- Modify: `src/domain/services/media_check.rs:50-80`
- Test: `src/domain/algorithm/video_utils.rs`
- Test: `src/services/video_information.rs:694-742`

**Interfaces:**
- Consumes: existing `is_video_extension(&str) -> bool` and `skip_file(&str) -> bool`.
- Produces: `pub fn is_video_scan_candidate(name: &str) -> bool` for `MediaCheck`; shared `classify_media_kind` remains the completed-download router.

- [ ] **Step 1: Write the failing movie-scan admission test**

Add beside `test_skip_file`:

```rust
#[test]
fn video_scan_rejects_books_and_preserves_extensionless_legacy_files() {
    for name in ["manual.pdf", "MANUAL.PDF", "novel.epub", "NOVEL.EPUB"] {
        assert!(!is_video_scan_candidate(name), "movie scan admitted {name}");
    }
    for name in ["movie.mp4", "MOVIE.MKV", "legacy-file"] {
        assert!(is_video_scan_candidate(name), "movie scan rejected {name}");
    }
    for name in [".hidden.mp4", "partial.tmp.mp4", "cover.jpg"] {
        assert!(!is_video_scan_candidate(name), "movie scan admitted {name}");
    }
}
```

- [ ] **Step 2: Run the new test and verify RED**

Run:

```bash
cargo test --lib domain::algorithm::video_utils::tests::video_scan_rejects_books_and_preserves_extensionless_legacy_files -- --exact
```

Expected: compilation fails because `is_video_scan_candidate` does not exist.

- [ ] **Step 3: Implement the dedicated admission function and use it only in `MediaCheck`**

Add to `video_utils.rs`:

```rust
pub fn is_video_scan_candidate(name: &str) -> bool {
    if skip_file(name) {
        return false;
    }
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        None | Some("") => true,
        Some(extension) => is_video_extension(extension),
    }
}
```

Re-export it from `domain/algorithm/mod.rs`. In the file loop in `MediaCheck`, replace `if skip_file(filename)` with `if !is_video_scan_candidate(filename)`. Keep directory filtering on `skip_file`; do not change `DownloadInfo` or `classify_media_kind`.

- [ ] **Step 4: Verify GREEN and preserved download routing**

Run:

```bash
cargo test --lib domain::algorithm::video_utils::tests::video_scan_rejects_books_and_preserves_extensionless_legacy_files -- --exact
cargo test --lib services::video_information::tests::routes_completed_pdf_and_epub_events_to_book_processing -- --exact
```

Expected: both tests pass.

- [ ] **Step 5: Commit the isolated scanner fix**

```bash
git add src/domain/algorithm/video_utils.rs src/domain/algorithm/mod.rs src/domain/services/media_check.rs
git commit -m "fix: keep books out of movie scans"
```

---

### Task 2: Replace In-Process PDF Parsing With Safe Fallback Metadata

**Files:**
- Modify: `Cargo.toml:28,58,76-82`
- Modify: `src/domain/services/book_metadata.rs:10,1077-1220,2233-2535`
- Modify: `src/domain/services/mod.rs`
- Test: `src/domain/services/book_metadata.rs`

**Interfaces:**
- Consumes: `filename_derived_title`, `ensure_default_book_thumbnail`, `DEFAULT_BOOK_THUMBNAIL`.
- Produces: existing `extract_pdf_metadata` and `extract_pdf_metadata_with_renderer` signatures, now guaranteed not to parse PDF bytes or invoke the renderer.

- [ ] **Step 1: Write a renderer-observation regression test**

Add a renderer with an atomic call counter and a test using invalid PDF bytes:

```rust
struct CountingRenderer(AtomicUsize);

impl PdfThumbnailRenderer for CountingRenderer {
    fn render_thumbnail(&self, _pdf_path: &Path) -> Result<image::DynamicImage, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err("renderer must not run".to_string())
    }
}

#[test]
fn invalid_pdf_uses_filename_and_default_cover_without_parsing_or_rendering() {
    let temp = TestDir::new();
    let pdf_path = temp.path().join("Untrusted Manual.pdf");
    fs::write(&pdf_path, b"not a PDF and deliberately unparseable").unwrap();
    let covers = temp.path().join("covers");
    let renderer = CountingRenderer(AtomicUsize::new(0));

    let result = extract_pdf_metadata_with_renderer(&pdf_path, &covers, "unsafe", &renderer)
        .expect("safe PDF fallback should not parse input bytes");

    assert_eq!(result.title.as_deref(), Some("Untrusted Manual"));
    assert!(result.authors.is_empty());
    assert_eq!(result.page_count, None);
    assert_eq!(result.thumbnail, DEFAULT_BOOK_THUMBNAIL);
    assert_eq!(renderer.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        fs::read(covers.join(DEFAULT_BOOK_THUMBNAIL)).unwrap(),
        default_book_thumbnail_bytes()
    );
}
```

Import `AtomicUsize` and `Ordering` in the test module.

- [ ] **Step 2: Run the new test and verify RED**

```bash
cargo test --lib domain::services::book_metadata::tests::invalid_pdf_uses_filename_and_default_cover_without_parsing_or_rendering -- --exact
```

Expected: fails with `could not read PDF` because `Document::load` still parses the invalid bytes.

- [ ] **Step 3: Implement the safe PDF fallback**

Replace the body of `extract_pdf_metadata_with_renderer` with:

```rust
pub fn extract_pdf_metadata_with_renderer<R: PdfThumbnailRenderer + ?Sized>(
    pdf_path: &Path,
    thumbnail_dir: &Path,
    _thumbnail_key: &str,
    _renderer: &R,
) -> Result<BookMetadataExtraction, BookMetadataExtractionError> {
    ensure_default_book_thumbnail(thumbnail_dir)
        .map_err(|error| BookMetadataExtractionError::Pdf(error.to_string()))?;
    Ok(BookMetadataExtraction {
        title: Some(filename_derived_title(pdf_path)),
        authors: Vec::new(),
        description: None,
        page_count: None,
        thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
        metadata: BookMetadata {
            raw: Some(json!({
                "pdf": { "metadataSkipped": "untrusted PDF parsing disabled" }
            })),
            ..BookMetadata::default()
        },
        warnings: vec!["PDF metadata parsing is disabled for untrusted input".to_string()],
    })
}
```

Delete `pdf_info_dictionary`, `pdf_info_text`, `render_pdf_thumbnail`, and the Pdfium-specific renderer implementation. Keep the public renderer trait and `DefaultPdfThumbnailRenderer` as compatibility types, but make its implementation return the existing unavailable-renderer error. Remove the `lopdf` dependency and import. Remove `pdfium-render`; retain `pdf-thumbnails = []` as a compatibility feature. Change the release profile to `panic = "unwind"`.

Update prior PDF extraction tests to assert filename/default-cover fallback and zero renderer calls; remove lopdf-based test construction helpers after no test uses them.

- [ ] **Step 4: Verify GREEN and ingestion behavior**

```bash
cargo test --lib domain::services::book_metadata::tests::invalid_pdf_uses_filename_and_default_cover_without_parsing_or_rendering -- --exact
cargo test --lib domain::services::book_metadata::tests::ingestion_metadata_free_pdf_uses_original_filename_title -- --exact
cargo test --lib domain::services::book_metadata::tests
```

Expected: all book-metadata tests pass; no test invokes lopdf or Pdfium.

- [ ] **Step 5: Commit the parser-boundary fix**

```bash
git add Cargo.toml Cargo.lock src/domain/services/book_metadata.rs src/domain/services/mod.rs
git commit -m "fix: avoid parsing untrusted PDFs in process"
```

---

### Task 3: Make Strict Directory Listing Entry-Resilient and Fail Closed by Default

**Files:**
- Modify: `src/adaptors/object_store.rs:1149-1194,2700-2775`
- Modify: `src/domain/traits.rs:160-180`
- Test: `src/adaptors/object_store.rs`
- Test: `src/domain/traits.rs`

**Interfaces:**
- Consumes: `FileSystemStore::open_root`, `Dir::entries`, `directory_entry_name`.
- Produces: strict listing that fails for an unsafe/unopenable requested directory but skips isolated entry errors; default trait method returns an unsupported-operation error.

- [ ] **Step 1: Write failing entry-isolation tests**

On Unix, create one non-UTF-8 file beside a valid PDF:

```rust
#[cfg(unix)]
#[tokio::test]
async fn strict_listing_skips_non_utf8_entry_and_keeps_valid_sibling() {
    use std::os::unix::ffi::OsStringExt;
    let base = std::env::temp_dir().join(format!(
        "tvserver-list-non-utf8-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("valid.pdf"), b"book").unwrap();
    std::fs::write(base.join(OsString::from_vec(vec![b'b', 0xff])), b"odd").unwrap();
    let store = FileSystemStore::new(base.to_str().unwrap());

    let listing = store.list_folder_no_follow("").await.unwrap();

    assert_eq!(listing, (Vec::new(), vec!["valid.pdf".to_string()]));
    std::fs::remove_dir_all(base).unwrap();
}
```

Extract an entry-error decision helper and test the desired behavior before wiring it into the loop:

```rust
#[test]
fn strict_listing_skips_one_vanished_entry_error() {
    let result = strict_entry_or_skip::<cap_std::fs::DirEntry>(Err(io::Error::new(
        io::ErrorKind::NotFound,
        "entry vanished",
    )));
    assert!(result.is_none());
}
```

Add a helper in `traits.rs` used by the default method and test it:

```rust
#[test]
fn no_follow_listing_default_is_explicitly_unsupported() {
    let error = unsupported_no_follow_listing("Shelf").unwrap_err();
    assert!(error.to_string().contains("strict no-follow listing is not supported"));
}
```

- [ ] **Step 2: Run the tests and verify RED**

```bash
cargo test --lib adaptors::object_store::tests::strict_listing_skips_non_utf8_entry_and_keeps_valid_sibling -- --exact
cargo test --lib adaptors::object_store::tests::strict_listing_skips_one_vanished_entry_error -- --exact
cargo test --lib domain::traits::tests::no_follow_listing_default_is_explicitly_unsupported -- --exact
```

Expected: the integration test returns a UTF-8 conversion error; helper tests fail to compile because the helpers do not exist.

- [ ] **Step 3: Skip only individual entry failures**

Add:

```rust
fn strict_entry_or_skip<T>(entry: io::Result<T>) -> Option<T> {
    match entry {
        Ok(entry) => Some(entry),
        Err(error) => {
            tracing::warn!("Skipping unreadable strict directory entry: {error}");
            None
        }
    }
}
```

In `list_folder_no_follow`, keep `directory.entries()?` as the directory-level failure boundary. Inside the loop, use `strict_entry_or_skip` for iterator errors; match UTF-8 conversion and `entry.file_type()` errors, log, and `continue`. Continue excluding symlinks and special file types.

In `traits.rs`, add:

```rust
fn unsupported_no_follow_listing(
    path: &str,
) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    anyhow::bail!("strict no-follow listing is not supported for {path}")
}
```

and make the default method call this helper instead of `self.list_folder(path).await`.

- [ ] **Step 4: Verify GREEN and existing symlink behavior**

```bash
cargo test --lib adaptors::object_store::tests::strict_listing_skips_non_utf8_entry_and_keeps_valid_sibling -- --exact
cargo test --lib adaptors::object_store::tests::strict_listing_skips_one_vanished_entry_error -- --exact
cargo test --lib adaptors::object_store::tests::strict_list_folder_skips_symlinks_to_files_and_directories -- --exact
cargo test --lib domain::traits::tests::no_follow_listing_default_is_explicitly_unsupported -- --exact
```

Expected: all four pass.

- [ ] **Step 5: Commit strict-listing resilience**

```bash
git add src/adaptors/object_store.rs src/domain/traits.rs
git commit -m "fix: isolate invalid strict directory entries"
```

---

### Task 4: Isolate Orphan-Reconciliation Inspection Failures Per Book

**Files:**
- Modify: `src/domain/services/book_check.rs:90-140,235-380,750-830`
- Test: `src/domain/services/book_check.rs`

**Interfaces:**
- Consumes: `FileStore::regular_file_exists_no_follow` and path leases.
- Produces: per-book reconciliation that deletes only confirmed-missing rows and preserves suspicious rows without aborting sibling processing.

- [ ] **Step 1: Rewrite the existing forced-inspection failure test to require sibling progress**

Create two repository rows. Configure `ControlledListStore` to return an inspection error only for `blocked.pdf`, leave `missing.pdf` absent, and assert:

```rust
checker.check_book_information().await.unwrap();
assert!(repository.retrieve_book(blocked.checksum).await.is_ok());
assert!(matches!(
    repository.retrieve_book(missing.checksum).await,
    Err(sqlx::Error::RowNotFound)
));
```

Change `ControlledListStore.inspection_error: bool` to `inspection_error_path: Option<PathBuf>` and compare it to the requested path.

- [ ] **Step 2: Run the test and verify RED**

```bash
cargo test --lib domain::services::book_check::tests::inspection_error_preserves_row_and_allows_sibling_reconciliation -- --exact
```

Expected: `check_book_information()` returns the forced exact-path inspection error before processing the sibling.

- [ ] **Step 3: Handle inspection errors inside the per-book loop**

Replace `if self.store.regular_file_exists_no_follow(&full_path).await?` with:

```rust
match self.store.regular_file_exists_no_follow(&full_path).await {
    Ok(true) => continue,
    Ok(false) => {}
    Err(error) => {
        tracing::error!(
            book = %book.file_name,
            checksum = book.checksum,
            path = %full_path.display(),
            "cannot safely inspect recorded book path; preserving row: {error}"
        );
        continue;
    }
}
```

Do not change deletion matching or lease acquisition.

- [ ] **Step 4: Verify GREEN and the scanner suite**

```bash
cargo test --lib domain::services::book_check::tests::inspection_error_preserves_row_and_allows_sibling_reconciliation -- --exact
cargo test --lib domain::services::book_check::tests
```

Expected: all book-check tests pass.

- [ ] **Step 5: Commit reconciliation isolation**

```bash
git add src/domain/services/book_check.rs
git commit -m "fix: isolate unsafe book reconciliation paths"
```

---

### Task 5: Move Snapshot and Thumbnail Identity Checks Onto Portable Capabilities

**Files:**
- Modify: `src/domain/traits.rs:130-220`
- Modify: `src/adaptors/object_store.rs:205-280,1450-1535,1800-2100`
- Modify: `src/domain/services/book_metadata.rs:790-845,1980-2075`
- Test: `src/adaptors/object_store.rs`
- Test: `src/domain/services/book_metadata.rs`

**Interfaces:**
- Produces: `PrivateSnapshotFingerprint { len: u64, modified: Option<SystemTime> }` and `FileStore::private_snapshot_fingerprint(&PrivateSnapshot)`.
- Consumes: retained `PrivateSnapshotAuthority`, cap-std `Dir`/`File`, and `cap_fs_ext::MetadataExt` only on cap-std metadata.

- [ ] **Step 1: Write failing capability-fingerprint and opened-thumbnail identity tests**

Add a snapshot test:

```rust
#[tokio::test]
async fn private_snapshot_fingerprint_rejects_replaced_visible_path() {
    let base = test_root("snapshot-fingerprint-replacement");
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let source = base.join("book.epub");
    std::fs::write(&source, b"owned").unwrap();
    let store = FileSystemStore::new(root.to_str().unwrap());
    let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
    let snapshot = store.create_private_snapshot(&staged).await.unwrap();
    std::fs::remove_file(&snapshot.path).unwrap();
    std::fs::write(&snapshot.path, b"replacement").unwrap();

    let error = store.private_snapshot_fingerprint(&snapshot).await.unwrap_err();
    assert!(error.to_string().contains("creation-time identity"));
    store.restore_staged(&staged).await.unwrap();
    std::fs::remove_dir_all(base).unwrap();
}
```

Add a unit test around a new `same_cap_file_identity` helper by hard-linking one file and comparing it with an unrelated file.

- [ ] **Step 2: Run the tests and verify RED**

```bash
cargo test --lib adaptors::object_store::tests::private_snapshot_fingerprint_rejects_replaced_visible_path -- --exact
cargo test --lib domain::services::book_metadata::tests::cap_file_identity_distinguishes_hard_link_from_unrelated_file -- --exact
```

Expected: compilation fails because the new trait method and helper do not exist.

- [ ] **Step 3: Implement capability-owned snapshot fingerprints**

In `traits.rs`, remove `PrivateSnapshot::path_has_creation_identity` and add:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateSnapshotFingerprint {
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
}
```

Add to `FileStore` with a fail-closed default:

```rust
async fn private_snapshot_fingerprint(
    &self,
    _snapshot: &PrivateSnapshot,
) -> anyhow::Result<PrivateSnapshotFingerprint> {
    anyhow::bail!("private snapshot fingerprinting is not supported by this file store")
}
```

The filesystem implementation retrieves `PrivateSnapshotAuthority`, calls `authority.directory.symlink_metadata(&authority.name)`, rejects symlinks/non-files and mismatched `dev`/`ino`, then returns length and modification time from that same cap-std metadata.

Update every test wrapper `impl FileStore` in `book_metadata.rs` and `book_check.rs` to delegate the new method to its inner store.

Change the book-metadata helper to:

```rust
async fn private_snapshot_fingerprint(
    storer: &FileStorer,
    snapshot: &PrivateSnapshot,
) -> anyhow::Result<PrivateSnapshotFingerprint> {
    storer.private_snapshot_fingerprint(snapshot).await
}
```

and pass `&storer` at each call site.

- [ ] **Step 4: Replace thumbnail identity comparisons with cap-std opened files**

Add:

```rust
fn same_cap_file_identity(
    left: &cap_std::fs::File,
    right: &cap_std::fs::File,
) -> Result<bool, String> {
    use cap_fs_ext::MetadataExt;
    let left = left.metadata().map_err(|error| error.to_string())?;
    let right = right.metadata().map_err(|error| error.to_string())?;
    Ok(left.is_file()
        && right.is_file()
        && left.dev() == right.dev()
        && left.ino() == right.ino())
}
```

Convert a clone of the retained `std::fs::File` with `cap_std::fs::File::from_std`. Open the published thumbnail through an ambient parent `Dir` using `FollowSymlinks::No`, then compare the two cap-std files with this helper. Remove all production `cap_fs_ext::MetadataExt` calls whose receiver is `std::fs::Metadata`.

- [ ] **Step 5: Verify GREEN and platform configurations**

```bash
cargo test --lib adaptors::object_store::tests::private_snapshot_fingerprint_rejects_replaced_visible_path -- --exact
cargo test --lib domain::services::book_metadata::tests::cap_file_identity_distinguishes_hard_link_from_unrelated_file -- --exact
cargo test --lib adaptors::object_store::tests
cargo test --lib domain::services::book_metadata::tests
cargo check --no-default-features --features webserver --all-targets
```

On a machine with the target installed, also run:

```bash
cargo check --target x86_64-pc-windows-gnu --no-default-features --features webserver --lib
```

Expected: all local tests/checks pass; the Windows check contains no missing `MetadataExt` implementation errors.

- [ ] **Step 6: Commit portable identity handling**

```bash
git add src/domain/traits.rs src/adaptors/object_store.rs src/domain/services/book_metadata.rs src/domain/services/book_check.rs
git commit -m "fix: use portable book file identities"
```

---

### Task 6: Introduce Explicit Available and Unavailable Book Runtime States

**Files:**
- Create: `src/entrypoints/book_runtime.rs`
- Modify: `src/entrypoints/mod.rs`
- Modify: `src/entrypoints/context.rs:1-270`
- Modify: `src/adaptors/object_store.rs:60-215`
- Modify: `tests/common/context.rs`
- Test: `src/entrypoints/context.rs`

**Interfaces:**
- Produces: `BookRuntime`, `AvailableBookRuntime`, `BookStaticRoots`, `BookIngestionRuntime`, `BOOK_LIBRARY_UNAVAILABLE`.
- Consumes: repository, local sender, configured roots, `FileSystemStore::try_new`, `BookStore`, `BookCheck`, and leases.

- [ ] **Step 1: Write failing runtime-initialization tests**

Add tests that initialize one writable runtime and one runtime whose book path is an existing regular file:

```rust
#[tokio::test]
async fn unavailable_book_root_builds_disabled_runtime_without_error() {
    let base = temp_context_root("unavailable-books");
    std::fs::create_dir_all(&base).unwrap();
    let blocked = base.join("not-a-directory");
    std::fs::write(&blocked, b"file").unwrap();
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    let exchange = LocalMessageExchange::new();

    let runtime = BookRuntime::initialize(
        repository,
        exchange.new_sender(),
        &blocked,
        blocked.join("covers"),
    )
    .await;

    assert!(runtime.available().is_none());
    assert_eq!(runtime.unavailable_message(), Some(BOOK_LIBRARY_UNAVAILABLE));
    std::fs::remove_dir_all(base).unwrap();
}
```

The writable test asserts `available().is_some()`, both retained roots are present, and the default thumbnail exists.

- [ ] **Step 2: Run the tests and verify RED**

```bash
cargo test --lib entrypoints::context::tests::unavailable_book_root_builds_disabled_runtime_without_error -- --exact
cargo test --lib entrypoints::context::tests::writable_book_root_builds_available_runtime -- --exact
```

Expected: compilation fails because `BookRuntime` does not exist.

- [ ] **Step 3: Add fallible filesystem-store construction**

Add:

```rust
pub fn try_new(root: &str) -> Result<Self> {
    let store = Self::new_with_move_filesystem(root, Arc::new(CapabilityMoveFileSystem));
    store.open_root()?;
    Ok(store)
}

pub(crate) fn retained_root(&self) -> Result<Arc<Dir>> {
    Ok(Arc::new(self.open_root()?.try_clone()?))
}
```

Keep `new` for existing callers; runtime initialization must use `try_new`.

- [ ] **Step 4: Implement `BookRuntime` as one cohesive bundle**

Create `book_runtime.rs` with this shape:

```rust
pub const BOOK_LIBRARY_UNAVAILABLE: &str = "book library unavailable";

#[derive(Clone)]
pub struct BookStaticRoots {
    pub(crate) downloads: Arc<Dir>,
    pub(crate) thumbnails: Arc<Dir>,
}

#[derive(Clone)]
pub struct BookIngestionRuntime {
    pub storer: FileStorer,
    pub leases: BookPathLeaseCoordinator,
}

pub struct AvailableBookRuntime {
    pub store: Arc<BookStore>,
    pub checker: BookCheckerHandle,
    pub ingestion: BookIngestionRuntime,
    pub static_roots: BookStaticRoots,
}

#[derive(Clone)]
pub enum BookRuntime {
    Available(Arc<AvailableBookRuntime>),
    Unavailable { message: Arc<str> },
}
```

`initialize` creates both directories with Tokio, materializes the default thumbnail inside `spawn_blocking`, creates fallible filesystem stores, retains roots, constructs one shared lease coordinator, and logs the detailed initialization failure before returning `Unavailable` with only the stable message.

Provide `available() -> Option<Arc<AvailableBookRuntime>>`, `ingestion() -> Option<BookIngestionRuntime>`, `static_roots() -> Option<BookStaticRoots>`, and `unavailable_message() -> Option<&str>`.

- [ ] **Step 5: Store only `BookRuntime` in `Context`**

Replace `book_store`, `book_file_storer`, and `book_path_leases` fields with `book_runtime`. Constructors take `BookRuntime`. Expose:

```rust
pub fn get_book_runtime(&self) -> BookRuntime { self.book_runtime.clone() }
pub fn get_available_book_runtime(&self) -> Option<Arc<AvailableBookRuntime>> {
    self.book_runtime.available()
}
```

`get_book_checker` returns the available checker or a shared `UnavailableBookChecker` whose `check_book_information` returns `Ok(())`. Update `create_context` to call `BookRuntime::initialize`; do not propagate book initialization errors.

Update `tests/common/context.rs` so `get_book_services_at` constructs an available runtime and test contexts pass that single value.

- [ ] **Step 6: Verify GREEN**

```bash
cargo test --lib entrypoints::context::tests::unavailable_book_root_builds_disabled_runtime_without_error -- --exact
cargo test --lib entrypoints::context::tests::writable_book_root_builds_available_runtime -- --exact
cargo test --lib entrypoints::context::tests
```

Expected: all context tests pass.

- [ ] **Step 7: Commit the runtime boundary**

```bash
git add src/entrypoints/book_runtime.rs src/entrypoints/mod.rs src/entrypoints/context.rs src/adaptors/object_store.rs tests/common/context.rs
git commit -m "feat: isolate optional book runtime"
```

---

### Task 7: Wire Unavailable Book Runtime Through HTTP, Tauri, and Metadata Workers

**Files:**
- Modify: `src/entrypoints/api.rs:30-250`
- Modify: `src/entrypoints/webserver.rs:65-290`
- Modify: `src/entrypoints/tauri_api.rs:90-180,390-520`
- Modify: `src/entrypoints/tvserver.rs:20-75`
- Modify: `src/services/video_information.rs:1-430`
- Modify: `tests/book_router_test.rs`
- Modify: `tests/book_api_test.rs`
- Test: `src/entrypoints/tauri_api.rs`
- Test: `src/services/video_information.rs`

**Interfaces:**
- Consumes: `Context::get_available_book_runtime`, `BookRuntime::static_roots`, `BookRuntime::ingestion`, and `BOOK_LIBRARY_UNAVAILABLE`.
- Produces: stable `503` HTTP responses and stable Tauri errors; metadata manager accepts `Option<BookIngestionRuntime>`.

- [ ] **Step 1: Write failing HTTP availability tests**

Build a context with `BookRuntime::Unavailable` and assert each method/path returns `503`:

```rust
for (method, path) in [
    (reqwest::Method::GET, "/api/books"),
    (reqwest::Method::GET, "/api/book/1"),
    (reqwest::Method::DELETE, "/api/book/1"),
    (reqwest::Method::GET, "/api/books/download/Shelf/book.epub"),
    (reqwest::Method::GET, "/api/book-thumbnails/default-book.jpg"),
] {
    let response = client.request(method, server.url(path)).send().await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
}
let video = client.get(server.url("/api/media/root")).send().await?;
assert_ne!(video.status(), StatusCode::SERVICE_UNAVAILABLE);
```

- [ ] **Step 2: Write failing Tauri and metadata-manager tests**

Add a command-core helper test asserting `require_available_books(&BookRuntime::Unavailable { ... })` returns exactly `BOOK_LIBRARY_UNAVAILABLE`. Add a metadata-manager test that sends a book event with `book_ingestion: None`, then a video event, and asserts the video processor runs once while the book processor never runs.

- [ ] **Step 3: Run the tests and verify RED**

```bash
cargo test --features webserver --test book_router_test unavailable_book_runtime_returns_503_without_disabling_video_routes -- --exact
cargo test --lib entrypoints::tauri_api::tests::unavailable_book_runtime_returns_stable_command_error -- --exact
cargo test --lib services::video_information::tests::disabled_books_do_not_block_video_workers -- --exact
```

Expected: router construction still errors or book routes do not return `503`; helper/manager tests fail to compile against the old interfaces.

- [ ] **Step 4: Gate every HTTP book handler and static route**

Add `SERVICE_UNAVAILABLE` and a single JSON response helper in `api.rs`. At the start of list/get/delete book handlers, require `state.get_available_book_runtime()`; return the stable `503` response when absent. Even `get_book` must gate before repository access.

Replace `RetainedRoot` construction from ambient paths with `BookStaticRoots` from the runtime. Register the same static routes in both states. Available handlers receive retained roots; unavailable handlers return `StatusCode::SERVICE_UNAVAILABLE` without touching disk.

- [ ] **Step 5: Gate Tauri book commands**

Add:

```rust
fn require_available_books(runtime: &BookRuntime) -> Result<Arc<AvailableBookRuntime>, String> {
    runtime
        .available()
        .ok_or_else(|| BOOK_LIBRARY_UNAVAILABLE.to_string())
}
```

Use it in `list_root_books`, `list_books`, `get_book`, and `delete_book` with `&state.get_book_runtime()` before repository or store access. Keep checksum parsing and command-core helpers unchanged after availability is established.

- [ ] **Step 6: Make metadata ingestion optional without affecting video workers**

Replace `book_storer` and `book_path_leases` fields with `book_ingestion: Option<BookIngestionRuntime>`. Update consume constructors accordingly. In `handle_media_event`, for `MediaKind::Book`, clone the optional ingestion runtime before spawning; if absent, log `book library unavailable` and return without reserving or spawning. Video handling remains byte-for-byte equivalent after constructor plumbing.

`TVServer::new` passes `context.get_book_runtime().ingestion()` and continues passing the no-op checker to `Monitor` when unavailable.

- [ ] **Step 7: Verify GREEN and route contract**

```bash
cargo test --features webserver --test book_router_test unavailable_book_runtime_returns_503_without_disabling_video_routes -- --exact
cargo test --lib entrypoints::tauri_api::tests::unavailable_book_runtime_returns_stable_command_error -- --exact
cargo test --lib services::video_information::tests::disabled_books_do_not_block_video_workers -- --exact
cargo test --features webserver --test book_api_test
cargo test --features webserver --test book_router_test
cargo test --features webserver --test openapi_contract_test
```

Expected: all tests pass and route/OpenAPI shape remains unchanged.

- [ ] **Step 8: Commit unavailable-runtime wiring**

```bash
git add src/entrypoints/api.rs src/entrypoints/webserver.rs src/entrypoints/tauri_api.rs src/entrypoints/tvserver.rs src/services/video_information.rs tests/book_router_test.rs tests/book_api_test.rs
git commit -m "fix: keep video server alive without book storage"
```

---

### Task 8: Align Container Storage and Run Final Verification

**Files:**
- Modify: `docker-compose.yml:5-18`
- Modify: `env.sample:1-12`
- Test: `docker-compose.yml`

**Interfaces:**
- Consumes: existing `${HOME}/Books:/Books` volume and lowercase general default.
- Produces: container-specific `BOOK_DIR=/Books` selection.

- [ ] **Step 1: Add a failing configuration assertion**

Run before editing:

```bash
rg -n '^\s+- BOOK_DIR=/Books$' docker-compose.yml
```

Expected: exit 1 because Compose does not set `BOOK_DIR`.

- [ ] **Step 2: Set the explicit container path**

Add under the `tvserver` service:

```yaml
    environment:
      - BOOK_DIR=/Books
```

Keep the existing volume unchanged. Update `env.sample` comments to state that Compose explicitly selects `/Books`, while non-container deployments retain the lowercase sibling default.

- [ ] **Step 3: Verify the configuration assertion**

```bash
rg -n '^\s+- BOOK_DIR=/Books$' docker-compose.yml
```

Expected: one matching line.

- [ ] **Step 4: Run focused and full verification**

```bash
cargo test --lib domain::algorithm::video_utils::tests
cargo test --lib domain::services::book_check::tests
cargo test --lib domain::services::book_metadata::tests
cargo test --lib adaptors::object_store::tests
cargo test --lib services::video_information::tests
cargo test --lib entrypoints::context::tests
cargo test --lib
cargo test --features webserver --test book_api_test
cargo test --features webserver --test book_router_test
cargo test --features webserver --test openapi_contract_test
cargo test --all-targets --no-run
cargo test --features webserver --all-targets --no-run
git diff --check
```

Expected: all tests and compilation checks pass; `git diff --check` prints nothing. Run `cargo fmt --all -- --check`; if the repository's existing nightly-only rustfmt configuration prevents a stable check, record that exact baseline limitation without applying a repository-wide mechanical rewrite.

- [ ] **Step 5: Commit configuration and documentation**

```bash
git add docker-compose.yml env.sample
git commit -m "fix: persist container book storage"
```

- [ ] **Step 6: Review the complete branch diff**

```bash
git status --short --branch
git log --oneline origin/spec/ebook-support..HEAD
git diff --stat 7a5f748..HEAD
git diff --check 7a5f748..HEAD
```

Expected: clean worktree, the task commits are present, and the completed implementation contains no whitespace errors.
