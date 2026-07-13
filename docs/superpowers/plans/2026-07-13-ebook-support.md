# Ebook Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add backend support for PDF and EPUB books while preserving existing video download, processing, and API behavior.

**Architecture:** Books are a sibling domain to videos: separate `BOOK_DIR`, `books` table, book metadata pipeline, book repository methods, `/api/books` routes, and Tauri commands. `MetaDataManager` remains the download-completion choke point and routes files by extension to either existing video processing or new book processing.

**Tech Stack:** Rust 2021, Tokio, Axum, Tauri, SQLite/sqlx, `zip` for EPUB archives, `quick-xml` for EPUB metadata XML, `lopdf` for PDF metadata, `image` for cover normalization, optional `pdfium-render` for PDF first-page thumbnails, existing `FileSystemStore`, `LocalMessageExchange`, and repository patterns.

---

## Spec Branch Discipline

This plan is committed on `spec/ebook-support`. Treat `spec/ebook-support` as the base branch for all implementation work. Do not create implementation branches directly from `main`.

Before starting any implementation task, run:

```bash
git switch spec/ebook-support
git pull --ff-only
git switch -c codex/ebook-support-<task-name>
```

Each implementation PR should target the integration branch chosen by the maintainer, but its local branch must start from `spec/ebook-support` so it contains the approved spec and this plan.

## File Structure

Create or modify these files:

- Create: `src/domain/algorithm/media_kind.rs`
  - Classifies file extensions as video, book, or unsupported.
- Modify: `src/domain/algorithm/mod.rs`
  - Re-export media classification helpers.
- Modify: `src/domain/config.rs`
  - Add `BOOK_DIR`, `BOOK_THUMBNAIL_DIR`, and default path helpers.
- Modify: `Cargo.toml`
  - Add EPUB/PDF/image dependencies and optional PDF renderer feature.
- Create: `src/domain/models/book.rs`
  - Defines `BookDetails`, `BookMetadata`, `BookState`, `BookFormat`, `BookCollectionDetails`, and serialization.
- Modify: `src/domain/models/mod.rs`
  - Export book models.
- Create: `src/domain/messages/book_event.rs`
  - Defines `BookEvent` and `BookEventType`.
- Modify: `src/domain/messages/local.rs`
  - Add `LocalMessage::Book`.
- Modify: `src/domain/messages/mod.rs`
  - Export book events.
- Modify: `src/domain/messagebus/local_message_exchange.rs`
  - Add `MessageFilter::Book` and routing for book events.
- Modify: `src/domain/messagebus/message_exchange.rs`
  - Broadcast book events to websocket clients.
- Create: `migrations/20260713000001_books.sql`
  - Adds `books` table and indexes.
- Modify: `src/domain/traits.rs`
  - Add repository book methods and a book scanner trait.
- Modify: `src/adaptors/repository.rs`
  - Implement book persistence methods and event emission.
- Create: `src/services/book_store.rs`
  - Lists, moves, and deletes books on disk and in the database.
- Create: `src/domain/services/book_metadata.rs`
  - Extracts PDF/EPUB metadata, assigns thumbnails, and saves books.
- Create: `src/domain/services/book_check.rs`
  - Scans `BOOK_DIR` for new/orphaned books.
- Modify: `src/domain/services/mod.rs`
  - Export book metadata and book scanner services.
- Modify: `src/services/mod.rs`
  - Export `BookStore`.
- Modify: `src/services/video_information.rs`
  - Route completed file events by media kind.
- Modify: `src/services/monitor.rs`
  - Run the book scanner alongside the existing video scanner.
- Modify: `src/entrypoints/context.rs`
  - Construct and expose `BookStore`, `BookCheck`, and the `BOOK_DIR` file store used by ingestion.
- Modify: `src/entrypoints/tvserver.rs`
  - Pass the book checker into the monitor.
- Modify: `src/entrypoints/api.rs`
  - Add REST handlers for books.
- Modify: `src/entrypoints/webserver.rs`
  - Serve book downloads and book thumbnails.
- Modify: `src/entrypoints/tauri_api.rs`
  - Add matching Tauri book commands.
- Modify: `tests/common/context.rs`
  - Support constructing contexts with the new book services.
- Modify: `tests/common/server.rs`
  - Serve book static routes in API tests.
- Create: `tests/book_api_test.rs`
  - REST coverage.
- Create: `tests/fixtures/book_dir/.thumbnails/.keep`
  - Book fixture directories.

### Task 1: Config, Dependencies, and Media Classification

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/domain/config.rs`
- Create: `src/domain/algorithm/media_kind.rs`
- Modify: `src/domain/algorithm/naming.rs`
- Modify: `src/domain/algorithm/mod.rs`

- [ ] **Step 1: Write failing tests for book config and media classification**

Add these tests to `src/domain/algorithm/media_kind.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn classifies_pdf_and_epub_as_books() {
        assert_eq!(classify_media_path(Path::new("Dune.pdf")), MediaKind::Book(BookFormat::Pdf));
        assert_eq!(classify_media_path(Path::new("Dune.epub")), MediaKind::Book(BookFormat::Epub));
        assert_eq!(classify_media_path(Path::new("Dune.EPUB")), MediaKind::Book(BookFormat::Epub));
    }

    #[test]
    fn classifies_existing_video_extensions_as_video() {
        assert_eq!(classify_media_path(Path::new("movie.mp4")), MediaKind::Video);
        assert_eq!(classify_media_path(Path::new("movie.mkv")), MediaKind::Video);
        assert_eq!(classify_media_path(Path::new("movie.webm")), MediaKind::Video);
    }

    #[test]
    fn rejects_unrelated_extensions() {
        assert_eq!(classify_media_path(Path::new("cover.jpg")), MediaKind::Unsupported);
        assert_eq!(classify_media_path(Path::new(".hidden.epub")), MediaKind::Unsupported);
    }
}
```

Add these tests to the existing test module in `src/domain/algorithm/naming.rs`:

```rust
#[test]
fn test_collection_helpers_support_explicit_roots() {
    let root = "/library/books";
    let path = Path::new("/library/books/Sci-Fi/Dune.pdf");

    assert_eq!(get_collection_from_root(path, root), "Sci-Fi");
    assert_eq!(
        get_collection_and_file_from_root(path, root),
        ("Sci-Fi".to_string(), "Dune.pdf".to_string())
    );
}
```

Add this test to the existing test module in `src/domain/config.rs` or create a new `#[cfg(test)]` module at the bottom:

```rust
#[cfg(test)]
mod book_config_tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    #[test]
    fn book_dir_requires_explicit_env_value() {
        env::remove_var(BOOK_DIR);
        let result = std::panic::catch_unwind(get_book_dir);
        assert!(result.is_err());
    }

    #[test]
    fn book_thumbnail_dir_defaults_inside_book_dir() {
        env::set_var(BOOK_DIR, "/tmp/tvserver-books");
        env::remove_var(BOOK_THUMBNAIL_DIR);
        assert_eq!(
            get_book_thumbnail_dir(&get_book_dir()),
            PathBuf::from("/tmp/tvserver-books/.thumbnails")
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --no-default-features --features webserver media_kind
cargo test --no-default-features --features webserver book_config_tests
```

Expected: FAIL because `media_kind`, `BookFormat`, `MediaKind`, `BOOK_DIR`, `BOOK_THUMBNAIL_DIR`, `get_book_dir`, `get_book_thumbnail_dir`, `get_collection_from_root`, and `get_collection_and_file_from_root` are not defined.

- [ ] **Step 3: Add dependencies and feature flags**

In `Cargo.toml`, add dependencies under `[dependencies]`:

```toml
image = { version = "0.25", default-features = false, features = ["jpeg", "png"] }
lopdf = { version = "0.44", default-features = false }
quick-xml = "0.41"
zip = { version = "8.6", default-features = false, features = ["deflate"] }
pdfium-render = { version = "0.9", default-features = false, features = ["image_025", "thread_safe"], optional = true }
```

In `[features]`, add:

```toml
pdf-thumbnails = ["dep:pdfium-render"]
```

- [ ] **Step 4: Add book config functions**

In `src/domain/config.rs`, add constants near the other environment variables:

```rust
pub const BOOK_DIR: &str = "BOOK_DIR";
const BOOK_THUMBNAIL_DIR: &str = "BOOK_THUMBNAIL_DIR";
```

Add functions near `get_movie_dir()` and `get_thumbnail_dir()`:

```rust
pub fn get_book_dir() -> String {
    env::var(BOOK_DIR).expect("BOOK_DIR environment variable is not set")
}

pub fn get_book_thumbnail_dir(book_dir: &str) -> PathBuf {
    match env::var(BOOK_THUMBNAIL_DIR) {
        Ok(dir) => PathBuf::from(dir),
        _ => PathBuf::from(book_dir).join(".thumbnails"),
    }
}
```

- [ ] **Step 5: Add media kind helper**

Create `src/domain/algorithm/media_kind.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookFormat {
    Pdf,
    Epub,
}

impl BookFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            BookFormat::Pdf => "pdf",
            BookFormat::Epub => "epub",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Book(BookFormat),
    Unsupported,
}

pub fn classify_media_path(path: &Path) -> MediaKind {
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    if file_name.starts_with('.') {
        return MediaKind::Unsupported;
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "pdf" => MediaKind::Book(BookFormat::Pdf),
        "epub" => MediaKind::Book(BookFormat::Epub),
        "mp4" | "mkv" | "avi" | "mov" | "flv" | "wmv" | "webm" | "m4v" | "mpg" | "mpeg"
        | "3gp" | "3g2" | "ts" | "vob" | "m2ts" | "mts" | "f4v" | "f4p" | "f4a"
        | "f4b" | "ogv" | "ogg" | "drc" | "gif" | "gifv" | "mng" | "qt" | "yuv"
        | "rm" | "rmvb" | "asf" | "amv" | "m4p" | "mp2" | "mpe" | "mpv" | "m2v"
        | "svi" | "mxf" | "roq" | "nsv" => MediaKind::Video,
        "" => MediaKind::Video,
        _ => MediaKind::Unsupported,
    }
}

pub fn is_supported_media_path(path: &Path) -> bool {
    !matches!(classify_media_path(path), MediaKind::Unsupported)
}
```

In `src/domain/algorithm/naming.rs`, add root-aware helpers and update the existing video helpers to delegate to them:

```rust
pub fn get_collection_from_root(path: &Path, root: &str) -> String {
    let short_path = match path.strip_prefix(root) {
        Ok(p) => PathBuf::from(p),
        _ => PathBuf::from(path),
    };

    if path.is_dir() {
        return short_path.to_str().unwrap_or_default().to_string();
    }

    match short_path.parent() {
        Some(parent) => parent.to_str().unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

pub fn get_collection_and_file_from_root(path: &Path, root: &str) -> (String, String) {
    let short_path = match path.strip_prefix(root) {
        Ok(p) => PathBuf::from(p),
        _ => PathBuf::from(path),
    };

    let parent = match short_path.parent() {
        Some(parent) => parent.to_str().unwrap_or_default().to_string(),
        _ => String::new(),
    };

    (
        parent,
        short_path
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
            .to_string(),
    )
}

pub fn get_collection_from_path(path: &Path) -> String {
    get_collection_from_root(path, &get_movie_dir())
}

pub fn get_collection_and_video_from_path(path: &Path) -> (String, String) {
    get_collection_and_file_from_root(path, &get_movie_dir())
}
```

Update `src/domain/algorithm/mod.rs`:

```rust
mod media_kind;

pub use media_kind::{classify_media_path, is_supported_media_path, BookFormat, MediaKind};
```

- [ ] **Step 6: Run tests to verify they pass**

Run:

```bash
cargo test --no-default-features --features webserver media_kind
cargo test --no-default-features --features webserver book_config_tests
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/domain/config.rs src/domain/algorithm/media_kind.rs src/domain/algorithm/naming.rs src/domain/algorithm/mod.rs
git commit -m "feat: add book config and media classification"
```

### Task 2: Book Domain Models and URL Helpers

**Files:**
- Create: `src/domain/models/book.rs`
- Modify: `src/domain/models/mod.rs`
- Modify: `src/domain/algorithm/naming.rs`
- Modify: `src/domain/algorithm/mod.rs`

- [ ] **Step 1: Write failing model serialization tests**

Create `src/domain/models/book.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    #[test]
    fn serializes_checksum_as_string_and_adds_urls() {
        let now = Local::now().naive_local();
        let book = BookDetails {
            file_name: "Dune.pdf".to_string(),
            collection: "Sci-Fi".to_string(),
            title: "Dune".to_string(),
            authors: vec!["Frank Herbert".to_string()],
            description: Some("A desert planet.".to_string()),
            publisher: Some("Chilton".to_string()),
            published_date: Some("1965".to_string()),
            language: Some("en".to_string()),
            isbn: Some("9780441172719".to_string()),
            format: BookFormat::Pdf,
            page_count: Some(412),
            thumbnail: "dune.jpg".to_string(),
            metadata: BookMetadata::default(),
            checksum: 1234,
            search_phrase: Some("dune".to_string()),
            state: BookState::Ready,
            created_on: now,
            updated_on: now,
            dir_path: None,
        };

        let json = serde_json::to_value(book).unwrap();

        assert_eq!(json["checksum"], "1234");
        assert_eq!(json["title"], "Dune");
        assert_eq!(json["authors"][0], "Frank Herbert");
        assert!(json["url"].as_str().unwrap().contains("Dune.pdf"));
        assert!(json["thumbnail"].as_str().unwrap().contains("dune.jpg"));
    }
}
```

- [ ] **Step 2: Run model test to verify it fails**

Run:

```bash
cargo test --no-default-features --features webserver serializes_checksum_as_string_and_adds_urls
```

Expected: FAIL because book model types are not defined.

- [ ] **Step 3: Add book URL helpers**

In `src/domain/algorithm/naming.rs`, add:

```rust
#[cfg(feature = "webserver")]
pub fn get_book_url(collection: &str, file_name: &str) -> String {
    if collection.is_empty() {
        format!("/api/books/download/{}", file_name)
    } else {
        format!("/api/books/download/{}/{}", collection, file_name)
    }
}

#[cfg(not(feature = "webserver"))]
pub fn get_book_url(collection: &str, file_name: &str) -> String {
    if collection.is_empty() {
        format!("{}/{}", crate::domain::config::get_book_dir(), file_name)
    } else {
        format!("{}/{}/{}", crate::domain::config::get_book_dir(), collection, file_name)
    }
}

#[cfg(feature = "webserver")]
pub fn get_book_thumbnail_url(thumbnail: &str) -> String {
    format!("/api/book-thumbnails/{}", thumbnail)
}

#[cfg(not(feature = "webserver"))]
pub fn get_book_thumbnail_url(thumbnail: &str) -> String {
    let book_dir = crate::domain::config::get_book_dir();
    let thumbnail_dir = crate::domain::config::get_book_thumbnail_dir(&book_dir)
        .to_string_lossy()
        .to_string();
    format!("{}/{}", thumbnail_dir, thumbnail)
}
```

Update `src/domain/algorithm/mod.rs` to re-export the new helpers:

```rust
pub use naming::{
    generate_display_name,
    get_book_thumbnail_url,
    get_book_url,
    get_collection_and_file_from_root,
    get_collection_and_video_from_path,
    get_collection_from_path,
    get_collection_from_root,
    get_next_version_name,
    get_thumbnails_url,
    get_video_url,
    replace_extension,
    title_case,
};
```

- [ ] **Step 4: Add book model code**

Replace `src/domain/models/book.rs` with:

```rust
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize, Serializer};
use serde::ser::SerializeStruct;
use std::path::{Path, PathBuf};

use crate::domain::algorithm::{get_book_thumbnail_url, get_book_url, BookFormat};
use crate::domain::config::get_book_dir;

pub const DEFAULT_BOOK_THUMBNAIL: &str = "default-book.jpg";

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_error: Option<String>,
}

#[derive(Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum BookState {
    #[default]
    Ready = 0,
    NewFile = 1,
    ZeroFileSize = 2,
    NeedMetadata = 3,
    MetadataError = 4,
    NeedThumbnail = 5,
    Exception = 10,
}

impl From<i32> for BookState {
    fn from(value: i32) -> Self {
        match value {
            0 => BookState::Ready,
            1 => BookState::NewFile,
            2 => BookState::ZeroFileSize,
            3 => BookState::NeedMetadata,
            4 => BookState::MetadataError,
            5 => BookState::NeedThumbnail,
            10 => BookState::Exception,
            _ => BookState::Ready,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BookDetails {
    pub file_name: String,
    pub collection: String,
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub format: BookFormat,
    pub page_count: Option<i32>,
    pub thumbnail: String,
    pub metadata: BookMetadata,
    pub checksum: i64,
    pub search_phrase: Option<String>,
    pub state: BookState,
    pub created_on: NaiveDateTime,
    pub updated_on: NaiveDateTime,
    #[serde(skip)]
    pub dir_path: Option<PathBuf>,
}

impl Default for BookDetails {
    fn default() -> Self {
        let now = Local::now().naive_local();
        Self {
            file_name: String::new(),
            collection: String::new(),
            title: String::new(),
            authors: Vec::new(),
            description: None,
            publisher: None,
            published_date: None,
            language: None,
            isbn: None,
            format: BookFormat::Pdf,
            page_count: None,
            thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            metadata: BookMetadata::default(),
            checksum: 0,
            search_phrase: None,
            state: BookState::NewFile,
            created_on: now,
            updated_on: now,
            dir_path: None,
        }
    }
}

impl BookDetails {
    pub fn new(file_name: String, collection: String, path: &Path, format: BookFormat) -> Self {
        let now = Local::now().naive_local();
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name)
            .replace('.', " ");

        Self {
            file_name,
            collection,
            title,
            format,
            created_on: now,
            updated_on: now,
            dir_path: path.parent().map(|p| p.to_path_buf()),
            ..Default::default()
        }
    }

    pub fn get_full_path(&self) -> PathBuf {
        if let Some(dir_path) = &self.dir_path {
            dir_path.join(&self.file_name)
        } else if self.collection.is_empty() {
            Path::new(&get_book_dir()).join(&self.file_name)
        } else {
            Path::new(&get_book_dir()).join(&self.collection).join(&self.file_name)
        }
    }

    pub fn get_download_path(&self) -> String {
        if self.collection.is_empty() {
            self.file_name.clone()
        } else {
            format!("{}/{}", self.collection, self.file_name)
        }
    }

    pub fn should_retry_metadata(&self) -> bool {
        matches!(self.state, BookState::NeedMetadata | BookState::MetadataError)
    }
}

impl Serialize for BookDetails {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut field_count = 19;
        if self.search_phrase.is_none() {
            field_count -= 1;
        }

        let mut state = serializer.serialize_struct("BookDetails", field_count)?;
        state.serialize_field("fileName", &self.file_name)?;
        state.serialize_field("collection", &self.collection)?;
        state.serialize_field("title", &self.title)?;
        state.serialize_field("authors", &self.authors)?;
        state.serialize_field("description", &self.description)?;
        state.serialize_field("publisher", &self.publisher)?;
        state.serialize_field("publishedDate", &self.published_date)?;
        state.serialize_field("language", &self.language)?;
        state.serialize_field("isbn", &self.isbn)?;
        state.serialize_field("format", &self.format)?;
        state.serialize_field("pageCount", &self.page_count)?;
        state.serialize_field("thumbnail", &get_book_thumbnail_url(&self.thumbnail))?;
        state.serialize_field("metadata", &self.metadata)?;
        state.serialize_field("checksum", &self.checksum.to_string())?;
        if let Some(search_phrase) = &self.search_phrase {
            state.serialize_field("searchPhrase", search_phrase)?;
        }
        state.serialize_field("state", &self.state)?;
        state.serialize_field("createdOn", &self.created_on)?;
        state.serialize_field("updatedOn", &self.updated_on)?;
        state.serialize_field("url", &get_book_url(&self.collection, &self.file_name))?;
        state.end()
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BookCollectionItem {
    pub collection: String,
    pub thumbnail: String,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BookCollectionDetails {
    pub collection: String,
    pub parent_collection: String,
    pub child_collections: Vec<BookCollectionItem>,
    pub books: Vec<BookDetails>,
    pub errors: Vec<String>,
}
```

- [ ] **Step 5: Export book models**

Update `src/domain/models/mod.rs`:

```rust
mod book;
mod conversion;
mod search_results;
mod video;
mod youtube;

pub use book::{
    BookCollectionDetails, BookCollectionItem, BookDetails, BookMetadata, BookState,
    DEFAULT_BOOK_THUMBNAIL,
};
pub use conversion::{Conversion, AVAILABLE_CONVERSIONS};
pub use search_results::{DownloadableItem, SearchResults, TaskListResults};
pub use video::{CollectionItem, CollectionDetails, SeriesDetails, VideoDetails, VideoState, VideoMetadata};
pub use youtube::{Id, Item, Snippet, YoutubeResponse};
```

- [ ] **Step 6: Run model tests**

Run:

```bash
cargo test --no-default-features --features webserver serializes_checksum_as_string_and_adds_urls
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/domain/models/book.rs src/domain/models/mod.rs src/domain/algorithm/naming.rs src/domain/algorithm/mod.rs
git commit -m "feat: add book domain models"
```

### Task 3: Book Events and Local Message Routing

**Files:**
- Create: `src/domain/messages/book_event.rs`
- Modify: `src/domain/messages/local.rs`
- Modify: `src/domain/messages/mod.rs`
- Modify: `src/domain/messagebus/local_message_exchange.rs`
- Modify: `src/domain/messagebus/message_exchange.rs`

- [ ] **Step 1: Write failing local exchange test**

In `src/domain/messagebus/local_message_exchange.rs`, add this test to the existing test module:

```rust
#[tokio::test]
async fn test_sending_book_messages() {
    use crate::domain::messages::{BookEvent, LocalMessage};
    use crate::domain::models::BookDetails;
    use tokio::time::{timeout, Duration};

    let exchange = LocalMessageExchange::new();
    let sender = exchange.new_sender();
    let mut book_receiver = exchange.listen_for_messages(MessageFilter::Book).await.unwrap();
    let mut video_receiver = exchange.listen_for_messages(MessageFilter::Video).await.unwrap();
    let mut all_receiver = exchange.listen_for_messages(MessageFilter::All).await.unwrap();

    let book = BookDetails {
        checksum: 42,
        title: "Dune".to_string(),
        file_name: "Dune.pdf".to_string(),
        ..Default::default()
    };
    let message = LocalMessage::Book(BookEvent::new_book_added_event(book));

    sender.send(message).await.unwrap();

    assert!(timeout(Duration::from_millis(100), book_receiver.recv()).await.is_ok());
    assert!(timeout(Duration::from_millis(100), all_receiver.recv()).await.is_ok());
    assert!(timeout(Duration::from_millis(100), video_receiver.recv()).await.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --no-default-features --features webserver test_sending_book_messages
```

Expected: FAIL because `BookEvent`, `LocalMessage::Book`, and `MessageFilter::Book` do not exist.

- [ ] **Step 3: Add `BookEvent`**

Create `src/domain/messages/book_event.rs`:

```rust
use crate::domain::models::BookDetails;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum BookEventType {
    BookEventAdded = 0,
    BookEventChanged = 1,
    BookEventDeleted = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookEvent {
    #[serde(rename = "type")]
    pub event_type: BookEventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book: Option<BookDetails>,
    pub checksum: String,
}

impl BookEvent {
    pub fn new_book_added_event(book: BookDetails) -> Self {
        Self {
            event_type: BookEventType::BookEventAdded,
            checksum: book.checksum.to_string(),
            book: Some(book),
        }
    }

    pub fn new_book_changed_event(book: BookDetails) -> Self {
        Self {
            event_type: BookEventType::BookEventChanged,
            checksum: book.checksum.to_string(),
            book: Some(book),
        }
    }

    pub fn new_book_deleted_event(checksum: i64) -> Self {
        Self {
            event_type: BookEventType::BookEventDeleted,
            book: None,
            checksum: checksum.to_string(),
        }
    }
}
```

- [ ] **Step 4: Wire local message types**

In `src/domain/messages/local.rs`, import and add the variant:

```rust
use super::book_event::BookEvent;

pub enum LocalMessage {
    Media(MediaEvent),
    Task(Vec<TaskState>),
    Video(VideoEvent),
    Book(BookEvent),
    PlayerState(RemotePlayerState),
    SendToRemote(SocketAddr, RemoteMessage),
    LastStateRequest(SocketAddr),
}
```

In `src/domain/messages/mod.rs`, add:

```rust
mod book_event;
pub use book_event::*;
```

- [ ] **Step 5: Add book message filter and websocket broadcast**

In `src/domain/messagebus/local_message_exchange.rs`, extend `MessageFilter` and the initial broadcasters:

```rust
Book,
```

Add it to the initial broadcaster vector:

```rust
(MessageFilter::Book, LocalMessageSenderReceiver::new()),
```

Add routing in `broadcast()`:

```rust
LocalMessage::Book(_) => Some(MessageFilter::Book),
```

In `src/domain/messagebus/message_exchange.rs`, add a match arm in `on_local_message()`:

```rust
LocalMessage::Book(book_event) => {
    let remote_message = RemoteMessage::Command {
        command: format!("book_event:{:?}", book_event),
    };
    MessageExchange::broadcast_to_all(client_map, remote_message).await;
}
```

- [ ] **Step 6: Run test**

Run:

```bash
cargo test --no-default-features --features webserver test_sending_book_messages
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/domain/messages/book_event.rs src/domain/messages/local.rs src/domain/messages/mod.rs src/domain/messagebus/local_message_exchange.rs src/domain/messagebus/message_exchange.rs
git commit -m "feat: add book events"
```

### Task 4: Books Migration and Repository Methods

**Files:**
- Create: `migrations/20260713000001_books.sql`
- Modify: `src/domain/traits.rs`
- Modify: `src/adaptors/repository.rs`

- [ ] **Step 1: Write failing repository tests**

In the existing test module in `src/adaptors/repository.rs`, add:

```rust
#[tokio::test]
async fn test_save_and_retrieve_book_details() {
    use crate::domain::algorithm::BookFormat;
    use crate::domain::models::{BookDetails, BookState};

    let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
    let mut book = BookDetails {
        checksum: 991,
        file_name: "Dune.pdf".to_string(),
        collection: "Sci-Fi".to_string(),
        title: "Dune".to_string(),
        authors: vec!["Frank Herbert".to_string()],
        format: BookFormat::Pdf,
        state: BookState::Ready,
        ..Default::default()
    };

    assert_eq!(db.save_book(&book).await.unwrap(), 991);
    let saved = db.retrieve_book(991).await.unwrap();
    assert_eq!(saved.title, "Dune");
    assert_eq!(saved.authors, vec!["Frank Herbert".to_string()]);

    book.title = "Dune Revised".to_string();
    db.save_book(&book).await.unwrap();
    let updated = db.retrieve_book(991).await.unwrap();
    assert_eq!(updated.title, "Dune Revised");
}

#[tokio::test]
async fn test_list_book_collections_and_delete_book() {
    use crate::domain::algorithm::BookFormat;
    use crate::domain::models::{BookDetails, BookState};

    let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
    db.save_book(&BookDetails {
        checksum: 111,
        file_name: "Dune.pdf".to_string(),
        collection: "Sci-Fi/Classics".to_string(),
        title: "Dune".to_string(),
        format: BookFormat::Pdf,
        state: BookState::Ready,
        ..Default::default()
    }).await.unwrap();

    assert_eq!(db.list_book_collections("").await.unwrap(), vec!["Sci-Fi".to_string()]);
    assert_eq!(db.list_book_collections("Sci-Fi").await.unwrap(), vec!["Classics".to_string()]);
    assert_eq!(db.list_books("Sci-Fi/Classics").await.unwrap().len(), 1);
    assert_eq!(db.delete_book(111).await.unwrap(), 1);
    assert!(db.retrieve_book(111).await.is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --no-default-features --features webserver test_save_and_retrieve_book_details
cargo test --no-default-features --features webserver test_list_book_collections_and_delete_book
```

Expected: FAIL because repository methods and table are missing.

- [ ] **Step 3: Add migration**

Create `migrations/20260713000001_books.sql`:

```sql
CREATE TABLE IF NOT EXISTS books (
    checksum INTEGER PRIMARY KEY NOT NULL,
    file_name TEXT NOT NULL,
    collection TEXT NOT NULL,
    title TEXT NOT NULL,
    authors TEXT,
    description TEXT,
    publisher TEXT,
    published_date TEXT,
    language TEXT,
    isbn TEXT,
    format TEXT NOT NULL,
    page_count INTEGER,
    thumbnail TEXT NOT NULL,
    metadata TEXT,
    search_phrase TEXT,
    state INTEGER DEFAULT 0,
    created_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_books_collection_file
    ON books(collection, file_name);

CREATE INDEX IF NOT EXISTS idx_books_title
    ON books(title);

CREATE INDEX IF NOT EXISTS idx_books_authors
    ON books(authors);
```

- [ ] **Step 4: Add trait methods**

In `src/domain/traits.rs`, import book models:

```rust
use super::models::{BookCollectionItem, BookDetails, CollectionItem, DownloadableItem, SearchResults, VideoDetails};
```

Add methods to `Databaser`:

```rust
async fn save_book(&self, details: &BookDetails) -> Result<i64, sqlx::Error>;
async fn list_book_collections(&self, collection: &str) -> Result<Vec<String>, sqlx::Error>;
async fn list_books(&self, collection: &str) -> Result<Vec<BookDetails>, sqlx::Error>;
async fn retrieve_book(&self, checksum: i64) -> Result<BookDetails, sqlx::Error>;
async fn delete_book(&self, checksum: i64) -> Result<u64, sqlx::Error>;
async fn list_all_books(&self) -> Result<Vec<BookDetails>, sqlx::Error>;
```

- [ ] **Step 5: Implement repository conversion helpers**

In `src/adaptors/repository.rs`, add imports:

```rust
use crate::domain::algorithm::BookFormat;
use crate::domain::messages::BookEvent;
use crate::domain::models::{BookDetails, BookMetadata, BookState};
```

Add helper methods to `impl SqlRepository`:

```rust
fn book_from_record(row: &SqliteRow) -> BookDetails {
    let authors_str = row.get::<Option<String>, _>("authors").unwrap_or_default();
    let authors: Vec<String> = serde_json::from_str(&authors_str).unwrap_or_default();
    let metadata_str = row.get::<Option<String>, _>("metadata").unwrap_or_default();
    let metadata: BookMetadata = serde_json::from_str(&metadata_str).unwrap_or_default();
    let format = match row.get::<String, _>("format").as_str() {
        "epub" => BookFormat::Epub,
        _ => BookFormat::Pdf,
    };

    BookDetails {
        file_name: row.get("file_name"),
        collection: row.get("collection"),
        title: row.get("title"),
        authors,
        description: row.get("description"),
        publisher: row.get("publisher"),
        published_date: row.get("published_date"),
        language: row.get("language"),
        isbn: row.get("isbn"),
        format,
        page_count: row.get("page_count"),
        thumbnail: row.get("thumbnail"),
        metadata,
        checksum: row.get("checksum"),
        search_phrase: row.get("search_phrase"),
        state: row.get::<i32, _>("state").into(),
        created_on: row.get("created_on"),
        updated_on: row.get("updated_on"),
        dir_path: None,
    }
}
```

- [ ] **Step 6: Implement repository methods**

Add implementations inside `impl Databaser for SqlRepository`:

```rust
async fn save_book(&self, details: &BookDetails) -> Result<i64, sqlx::Error> {
    let mut tx = self.pool.begin().await?;
    let existing = sqlx::query("SELECT checksum FROM books WHERE checksum = ?")
        .bind(details.checksum)
        .fetch_optional(&mut *tx)
        .await?;
    let is_update = existing.is_some();

    let authors = serde_json::to_string(&details.authors).unwrap_or_default();
    let metadata = serde_json::to_string(&details.metadata).unwrap_or_default();
    let state: i32 = details.state as i32;

    let query = r#"
        INSERT INTO books (
            checksum, file_name, collection, title, authors, description,
            publisher, published_date, language, isbn, format, page_count,
            thumbnail, metadata, search_phrase, state
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(collection, file_name) DO UPDATE SET
            checksum = ?,
            title = ?,
            authors = ?,
            description = ?,
            publisher = ?,
            published_date = ?,
            language = ?,
            isbn = ?,
            format = ?,
            page_count = ?,
            thumbnail = ?,
            metadata = ?,
            search_phrase = ?,
            state = ?,
            updated_on = CURRENT_TIMESTAMP
        ON CONFLICT(checksum) DO UPDATE SET
            file_name = ?,
            collection = ?,
            title = ?,
            authors = ?,
            description = ?,
            publisher = ?,
            published_date = ?,
            language = ?,
            isbn = ?,
            format = ?,
            page_count = ?,
            thumbnail = ?,
            metadata = ?,
            search_phrase = ?,
            state = ?,
            updated_on = CURRENT_TIMESTAMP
    "#;

    sqlx::query(query)
        .bind(details.checksum)
        .bind(&details.file_name)
        .bind(&details.collection)
        .bind(&details.title)
        .bind(&authors)
        .bind(&details.description)
        .bind(&details.publisher)
        .bind(&details.published_date)
        .bind(&details.language)
        .bind(&details.isbn)
        .bind(details.format.as_str())
        .bind(details.page_count)
        .bind(&details.thumbnail)
        .bind(&metadata)
        .bind(&details.search_phrase)
        .bind(state)
        .bind(details.checksum)
        .bind(&details.title)
        .bind(&authors)
        .bind(&details.description)
        .bind(&details.publisher)
        .bind(&details.published_date)
        .bind(&details.language)
        .bind(&details.isbn)
        .bind(details.format.as_str())
        .bind(details.page_count)
        .bind(&details.thumbnail)
        .bind(&metadata)
        .bind(&details.search_phrase)
        .bind(state)
        .bind(&details.file_name)
        .bind(&details.collection)
        .bind(&details.title)
        .bind(&authors)
        .bind(&details.description)
        .bind(&details.publisher)
        .bind(&details.published_date)
        .bind(&details.language)
        .bind(&details.isbn)
        .bind(details.format.as_str())
        .bind(details.page_count)
        .bind(&details.thumbnail)
        .bind(&metadata)
        .bind(&details.search_phrase)
        .bind(state)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    if let Some(sender) = &self.sender {
        let message = if is_update {
            LocalMessage::Book(BookEvent::new_book_changed_event(details.clone()))
        } else {
            LocalMessage::Book(BookEvent::new_book_added_event(details.clone()))
        };
        if let Err(e) = sender.send(message).await {
            tracing::error!("Error sending book event {}", e);
        }
    }

    Ok(details.checksum)
}
```

Add list/retrieve/delete methods using raw `sqlx::query()`:

```rust
async fn list_book_collections(&self, parent_collection: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows = if parent_collection.is_empty() {
        sqlx::query("SELECT DISTINCT collection FROM books WHERE collection <> ''")
            .fetch_all(&self.pool)
            .await?
    } else {
        let collection = format!("{}%", parent_collection);
        sqlx::query("SELECT DISTINCT collection FROM books WHERE collection LIKE ?")
            .bind(collection)
            .fetch_all(&self.pool)
            .await?
    };

    let pick_part = if parent_collection.is_empty() {
        0
    } else {
        parent_collection.matches('/').count() + 1
    };

    Ok(rows
        .into_iter()
        .map(|row| row.get::<String, _>("collection"))
        .filter_map(|s| s.split('/').nth(pick_part).map(str::to_string))
        .unique()
        .sorted()
        .collect())
}

async fn list_books(&self, collection: &str) -> Result<Vec<BookDetails>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM books WHERE collection = ? ORDER BY title, file_name")
        .bind(collection)
        .fetch_all(&self.pool)
        .await?;
    Ok(rows.iter().map(Self::book_from_record).collect())
}

async fn retrieve_book(&self, checksum: i64) -> Result<BookDetails, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM books WHERE checksum = ?")
        .bind(checksum)
        .fetch_one(&self.pool)
        .await?;
    Ok(Self::book_from_record(&row))
}

async fn delete_book(&self, checksum: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM books WHERE checksum = ?")
        .bind(checksum)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected());

    if let Ok(rows) = result {
        if rows > 0 {
            if let Some(sender) = &self.sender {
                if let Err(e) = sender.send(LocalMessage::Book(BookEvent::new_book_deleted_event(checksum))).await {
                    tracing::error!("Error sending book deleted event {} {}", checksum, e);
                }
            }
        }
    }

    result
}

async fn list_all_books(&self) -> Result<Vec<BookDetails>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM books ORDER BY collection, title, file_name")
        .fetch_all(&self.pool)
        .await?;
    Ok(rows.iter().map(Self::book_from_record).collect())
}
```

- [ ] **Step 7: Run repository tests**

Run:

```bash
cargo test --no-default-features --features webserver test_save_and_retrieve_book_details
cargo test --no-default-features --features webserver test_list_book_collections_and_delete_book
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add migrations/20260713000001_books.sql src/domain/traits.rs src/adaptors/repository.rs
git commit -m "feat: persist book details"
```

### Task 5: BookStore Service

**Files:**
- Create: `src/services/book_store.rs`
- Modify: `src/services/mod.rs`
- Modify: `src/entrypoints/context.rs`

- [ ] **Step 1: Write failing service tests**

Create `src/services/book_store.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::{FileSystemStore, SqlRepository};
    use crate::domain::algorithm::BookFormat;
    use crate::domain::config::{BOOK_DIR, MOVIE_DIR};
    use crate::domain::models::{BookDetails, BookState, DEFAULT_BOOK_THUMBNAIL};
    use crate::domain::traits::{FileStorer, Repository};
    use std::env;
    use std::sync::Arc;

    #[tokio::test]
    async fn list_returns_book_collection_details() {
        env::set_var(BOOK_DIR, "tests/fixtures/book_dir");
        env::set_var(MOVIE_DIR, "tests/fixtures/media_dir");
        let repo: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        repo.save_book(&BookDetails {
            checksum: 31,
            file_name: "Dune.pdf".to_string(),
            collection: "Sci-Fi".to_string(),
            title: "Dune".to_string(),
            format: BookFormat::Pdf,
            thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            state: BookState::Ready,
            ..Default::default()
        }).await.unwrap();

        let store: FileStorer = Arc::new(FileSystemStore::new("tests/fixtures/book_dir"));
        let book_store = BookStore::new(store, repo);
        let result = book_store.list("Sci-Fi").await.unwrap();

        assert_eq!(result.collection, "Sci-Fi");
        assert_eq!(result.books.len(), 1);
        assert_eq!(result.books[0].title, "Dune");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --no-default-features --features webserver list_returns_book_collection_details
```

Expected: FAIL because `BookStore` is not implemented or exported.

- [ ] **Step 3: Implement `BookStore`**

Create `src/services/book_store.rs`:

```rust
use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::domain::algorithm::{get_collection_from_root, title_case};
use crate::domain::config::{get_book_dir, get_book_thumbnail_dir};
use crate::domain::models::{BookCollectionDetails, BookCollectionItem, DEFAULT_BOOK_THUMBNAIL};
use crate::domain::traits::{FileStorer, Repository};

#[derive(Clone)]
pub struct BookStore {
    store: FileStorer,
    repo: Repository,
}

impl BookStore {
    pub fn new(store: FileStorer, repo: Repository) -> Self {
        Self { store, repo }
    }

    pub async fn list(&self, collection: &str) -> Result<BookCollectionDetails> {
        let child_collections = self
            .repo
            .list_book_collections(collection)
            .await?
            .into_iter()
            .map(|collection| BookCollectionItem {
                collection,
                thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            })
            .collect();

        let books = self.repo.list_books(collection).await?;

        Ok(BookCollectionDetails {
            collection: collection.to_string(),
            parent_collection: parent_collection(collection),
            child_collections,
            books,
            errors: Vec::new(),
        })
    }

    pub async fn add_file(&self, full_path: &Path, suggested_collection: Option<String>) -> Result<PathBuf> {
        let collection = match suggested_collection {
            Some(collection) => title_case(&collection),
            None => get_collection_from_root(full_path, &get_book_dir()),
        };
        let dest_dir = Path::new(&get_book_dir()).join(&collection);
        self.store.create_folder(&dest_dir).await?;
        let dest_path = dest_dir.join(full_path.file_name().unwrap_or_default());

        if full_path == dest_path {
            return Ok(dest_path);
        }

        self.store
            .rename(
                full_path.to_str().unwrap_or_default(),
                dest_path.to_str().unwrap_or_default(),
            )
            .await?;
        Ok(dest_path)
    }

    pub async fn delete(&self, checksum: i64) -> Result<()> {
        let book = self.repo.retrieve_book(checksum).await?;
        let book_path = book.get_full_path();
        self.store.delete(book_path.to_str().unwrap_or_default()).await?;

        if book.thumbnail != DEFAULT_BOOK_THUMBNAIL {
            let thumbnail_path = get_book_thumbnail_dir(&get_book_dir()).join(&book.thumbnail);
            if let Err(err) = tokio::fs::remove_file(&thumbnail_path).await {
                tracing::warn!("failed to delete book thumbnail {}: {}", book.thumbnail, err);
            }
        }

        self.repo.delete_book(checksum).await?;
        Ok(())
    }
}

fn parent_collection(collection: &str) -> String {
    if let Some(pos) = collection.find('/') {
        collection[..pos].to_string()
    } else {
        String::new()
    }
}
```

- [ ] **Step 4: Export and expose store in context**

In `src/services/mod.rs`, add:

```rust
mod book_store;
pub use book_store::BookStore;
```

In `src/entrypoints/context.rs`, add a field to `Context`:

```rust
book_store: Arc<BookStore>,
book_file_storer: FileStorer,
```

Update `Context::new()` signature to accept `book_store: Arc<BookStore>` and `book_file_storer: FileStorer`, assign both, and add:

```rust
pub fn get_book_store(&self) -> Arc<BookStore> {
    self.book_store.clone()
}

pub fn get_book_file_storer(&self) -> FileStorer {
    self.book_file_storer.clone()
}
```

In `create_context()`, construct:

```rust
let book_file_storer: FileStorer = Arc::new(FileSystemStore::new(&get_book_dir()));
let book_store = Arc::new(BookStore::new(book_file_storer.clone(), repository.clone()));
```

Pass `book_store` and `book_file_storer` into `Context::new()`.

- [ ] **Step 5: Update test context helper**

In `tests/common/context.rs`, create a `BookStore` for tests:

```rust
use app_lib::adaptors::FileSystemStore;
use app_lib::services::BookStore;
use app_lib::domain::traits::FileStorer;

let book_file_storer: FileStorer = Arc::new(FileSystemStore::new("tests/fixtures/book_dir"));
let book_store = Arc::new(BookStore::new(book_file_storer.clone(), repository.clone()));
```

Pass `book_store` and `book_file_storer` into `Context::new()`.

- [ ] **Step 6: Run service test**

Run:

```bash
cargo test --no-default-features --features webserver list_returns_book_collection_details
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/services/book_store.rs src/services/mod.rs src/entrypoints/context.rs tests/common/context.rs
git commit -m "feat: add book store service"
```

### Task 6: EPUB Metadata Extraction

**Files:**
- Create: `src/domain/services/book_metadata.rs`
- Modify: `src/domain/services/mod.rs`

- [ ] **Step 1: Write failing EPUB metadata tests**

Create `src/domain/services/book_metadata.rs` with these tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::DEFAULT_BOOK_THUMBNAIL;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    fn create_epub(path: &std::path::Path, with_cover: bool) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#).unwrap();

        zip.start_file("OEBPS/content.opf", options).unwrap();
        let cover_manifest = if with_cover {
            r#"<meta name="cover" content="cover-image"/>
               <manifest><item id="cover-image" href="cover.jpg" media-type="image/jpeg"/></manifest>"#
        } else {
            "<manifest></manifest>"
        };
        let opf = format!(r#"<?xml version="1.0"?>
<package xmlns:dc="http://purl.org/dc/elements/1.1/">
  <metadata>
    <dc:title>Dune</dc:title>
    <dc:creator>Frank Herbert</dc:creator>
    <dc:language>en</dc:language>
    <dc:publisher>Chilton</dc:publisher>
    <dc:date>1965</dc:date>
    {}
  </metadata>
</package>"#, cover_manifest);
        zip.write_all(opf.as_bytes()).unwrap();

        if with_cover {
            zip.start_file("OEBPS/cover.jpg", options).unwrap();
            zip.write_all(include_bytes!("../../../tests/fixtures/media_dir/test.jpg")).unwrap();
        }

        zip.finish().unwrap();
    }

    #[test]
    fn extracts_epub_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dune.epub");
        create_epub(&path, false);

        let extracted = extract_epub_metadata(&path).unwrap();

        assert_eq!(extracted.title, Some("Dune".to_string()));
        assert_eq!(extracted.authors, vec!["Frank Herbert".to_string()]);
        assert_eq!(extracted.language, Some("en".to_string()));
        assert_eq!(extracted.publisher, Some("Chilton".to_string()));
        assert_eq!(extracted.published_date, Some("1965".to_string()));
    }

    #[test]
    fn missing_epub_cover_uses_default_thumbnail() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dune.epub");
        create_epub(&path, false);

        let thumbnail = extract_epub_cover(&path, dir.path()).unwrap();

        assert_eq!(thumbnail, DEFAULT_BOOK_THUMBNAIL);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --no-default-features --features webserver extracts_epub_metadata
cargo test --no-default-features --features webserver missing_epub_cover_uses_default_thumbnail
```

Expected: FAIL because EPUB extraction functions are missing.

- [ ] **Step 3: Add EPUB extraction structs and functions**

In `src/domain/services/book_metadata.rs`, add production code above the tests:

```rust
use anyhow::{anyhow, Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

use crate::domain::models::DEFAULT_BOOK_THUMBNAIL;

#[derive(Default, Debug, Clone, PartialEq)]
pub struct ExtractedBookMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub page_count: Option<i32>,
    pub raw: Option<serde_json::Value>,
}

pub fn extract_epub_metadata(path: &Path) -> Result<ExtractedBookMetadata> {
    let mut archive = open_epub(path)?;
    let package_path = find_epub_package_path(&mut archive)?;
    let mut package = String::new();
    archive.by_name(&package_path)?.read_to_string(&mut package)?;
    parse_epub_package_metadata(&package)
}

pub fn extract_epub_cover(path: &Path, thumbnail_dir: &Path) -> Result<String> {
    let mut archive = open_epub(path)?;
    let package_path = find_epub_package_path(&mut archive)?;
    let mut package = String::new();
    archive.by_name(&package_path)?.read_to_string(&mut package)?;

    let Some(cover_href) = find_epub_cover_href(&package)? else {
        return Ok(DEFAULT_BOOK_THUMBNAIL.to_string());
    };

    let package_dir = Path::new(&package_path).parent().unwrap_or_else(|| Path::new(""));
    let cover_path = package_dir.join(cover_href);
    let mut cover_file = archive.by_name(cover_path.to_str().unwrap_or_default())?;
    std::fs::create_dir_all(thumbnail_dir)?;

    let output_name = format!(
        "{}_cover.jpg",
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("book").replace(' ', "_")
    );
    let output_path = thumbnail_dir.join(&output_name);
    let mut output = File::create(&output_path)?;
    std::io::copy(&mut cover_file, &mut output)?;
    Ok(output_name)
}

fn open_epub(path: &Path) -> Result<ZipArchive<File>> {
    let file = File::open(path).with_context(|| format!("opening epub {}", path.display()))?;
    ZipArchive::new(file).context("opening epub zip archive")
}

fn find_epub_package_path(archive: &mut ZipArchive<File>) -> Result<String> {
    let mut container = String::new();
    archive
        .by_name("META-INF/container.xml")?
        .read_to_string(&mut container)?;

    let mut reader = Reader::from_str(&container);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event()? {
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"rootfile" => {
                for attr in e.attributes() {
                    let attr = attr?;
                    if attr.key.as_ref() == b"full-path" {
                        return Ok(attr.unescape_value()?.to_string());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Err(anyhow!("epub package rootfile not found"))
}

fn parse_epub_package_metadata(package: &str) -> Result<ExtractedBookMetadata> {
    let mut reader = Reader::from_str(package);
    reader.config_mut().trim_text(true);
    let mut current = String::new();
    let mut metadata = ExtractedBookMetadata::default();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                current = String::from_utf8_lossy(e.name().as_ref()).to_string();
            }
            Event::Text(e) => {
                let value = e.unescape()?.to_string();
                match current.as_str() {
                    "dc:title" | "title" => metadata.title = Some(value),
                    "dc:creator" | "creator" => metadata.authors.push(value),
                    "dc:description" | "description" => metadata.description = Some(value),
                    "dc:publisher" | "publisher" => metadata.publisher = Some(value),
                    "dc:date" | "date" => metadata.published_date = Some(value),
                    "dc:language" | "language" => metadata.language = Some(value),
                    "dc:identifier" | "identifier" if value.to_ascii_lowercase().contains("isbn") => {
                        metadata.isbn = Some(value)
                    }
                    _ => {}
                }
            }
            Event::End(_) => current.clear(),
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(metadata)
}

fn find_epub_cover_href(package: &str) -> Result<Option<String>> {
    let mut reader = Reader::from_str(package);
    reader.config_mut().trim_text(true);
    let mut cover_id: Option<String> = None;
    let mut manifest_items: Vec<(String, String, String)> = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"meta" => {
                let mut name = None;
                let mut content = None;
                for attr in e.attributes() {
                    let attr = attr?;
                    match attr.key.as_ref() {
                        b"name" => name = Some(attr.unescape_value()?.to_string()),
                        b"content" => content = Some(attr.unescape_value()?.to_string()),
                        _ => {}
                    }
                }
                if name.as_deref() == Some("cover") {
                    cover_id = content;
                }
            }
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"item" => {
                let mut id = String::new();
                let mut href = String::new();
                let mut media_type = String::new();
                for attr in e.attributes() {
                    let attr = attr?;
                    match attr.key.as_ref() {
                        b"id" => id = attr.unescape_value()?.to_string(),
                        b"href" => href = attr.unescape_value()?.to_string(),
                        b"media-type" => media_type = attr.unescape_value()?.to_string(),
                        _ => {}
                    }
                }
                manifest_items.push((id, href, media_type));
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if let Some(cover_id) = cover_id {
        if let Some((_, href, _)) = manifest_items.iter().find(|(id, _, _)| id == &cover_id) {
            return Ok(Some(href.clone()));
        }
    }

    Ok(manifest_items
        .into_iter()
        .find(|(_, href, media_type)| {
            href.to_ascii_lowercase().contains("cover") && media_type.starts_with("image/")
        })
        .map(|(_, href, _)| href))
}
```

- [ ] **Step 4: Export service functions**

In `src/domain/services/mod.rs`, add:

```rust
mod book_metadata;
pub use book_metadata::{
    extract_epub_cover, extract_epub_metadata, ExtractedBookMetadata,
};
```

- [ ] **Step 5: Add `tempfile` dev dependency if missing**

If `tempfile` is not already present in `Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3.27"
```

- [ ] **Step 6: Run EPUB tests**

Run:

```bash
cargo test --no-default-features --features webserver extracts_epub_metadata
cargo test --no-default-features --features webserver missing_epub_cover_uses_default_thumbnail
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/domain/services/book_metadata.rs src/domain/services/mod.rs
git commit -m "feat: extract epub book metadata"
```

### Task 7: PDF Metadata and Thumbnail Renderer Boundary

**Files:**
- Modify: `src/domain/services/book_metadata.rs`
- Modify: `src/domain/services/mod.rs`

- [ ] **Step 1: Write failing PDF tests**

Add tests to `src/domain/services/book_metadata.rs`:

```rust
#[test]
fn extracts_pdf_metadata() {
    use lopdf::{dictionary, Document, Object};
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let path = dir.path().join("dune.pdf");
    let mut doc = Document::with_version("1.5");
    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Dune"),
        "Author" => Object::string_literal("Frank Herbert"),
    });
    doc.trailer.set("Info", info_id);
    doc.save(&path).unwrap();

    let extracted = extract_pdf_metadata(&path).unwrap();

    assert_eq!(extracted.title, Some("Dune".to_string()));
    assert_eq!(extracted.authors, vec!["Frank Herbert".to_string()]);
}

#[test]
fn pdf_thumbnail_failure_uses_default_thumbnail() {
    struct FailingRenderer;

    impl PdfThumbnailRenderer for FailingRenderer {
        fn render_first_page(&self, _path: &std::path::Path, _thumbnail_dir: &std::path::Path) -> anyhow::Result<String> {
            anyhow::bail!("renderer unavailable")
        }
    }

    let thumbnail = render_pdf_thumbnail_or_default(
        std::path::Path::new("missing.pdf"),
        std::path::Path::new("/tmp"),
        &FailingRenderer,
    );

    assert_eq!(thumbnail, DEFAULT_BOOK_THUMBNAIL);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --no-default-features --features webserver extracts_pdf_metadata
cargo test --no-default-features --features webserver pdf_thumbnail_failure_uses_default_thumbnail
```

Expected: FAIL because PDF extraction and renderer trait are missing.

- [ ] **Step 3: Add PDF metadata extraction and renderer trait**

In `src/domain/services/book_metadata.rs`, add:

```rust
use lopdf::{Document, Object};

pub trait PdfThumbnailRenderer: Send + Sync {
    fn render_first_page(&self, path: &Path, thumbnail_dir: &Path) -> Result<String>;
}

pub struct DefaultPdfThumbnailRenderer;

impl PdfThumbnailRenderer for DefaultPdfThumbnailRenderer {
    fn render_first_page(&self, path: &Path, thumbnail_dir: &Path) -> Result<String> {
        render_pdf_first_page(path, thumbnail_dir)
    }
}

pub fn extract_pdf_metadata(path: &Path) -> Result<ExtractedBookMetadata> {
    let doc = Document::load(path).with_context(|| format!("loading pdf {}", path.display()))?;
    let mut metadata = ExtractedBookMetadata {
        page_count: Some(doc.get_pages().len() as i32),
        ..Default::default()
    };

    if let Ok(info_ref) = doc.trailer.get(b"Info") {
        if let Ok(info_id) = info_ref.as_reference() {
            if let Ok(info) = doc.get_object(info_id).and_then(Object::as_dict) {
                metadata.title = pdf_dict_string(info.get(b"Title").ok());
                if let Some(author) = pdf_dict_string(info.get(b"Author").ok()) {
                    metadata.authors.push(author);
                }
                metadata.description = pdf_dict_string(info.get(b"Subject").ok());
            }
        }
    }

    Ok(metadata)
}

fn pdf_dict_string(value: Option<&Object>) -> Option<String> {
    value.and_then(|object| match object {
        Object::String(bytes, _) => Some(String::from_utf8_lossy(bytes).trim().to_string()),
        Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).trim().to_string()),
        _ => None,
    }).filter(|s| !s.is_empty())
}

pub fn render_pdf_thumbnail_or_default(
    path: &Path,
    thumbnail_dir: &Path,
    renderer: &dyn PdfThumbnailRenderer,
) -> String {
    match renderer.render_first_page(path, thumbnail_dir) {
        Ok(thumbnail) => thumbnail,
        Err(err) => {
            tracing::warn!("using default PDF thumbnail for {}: {}", path.display(), err);
            DEFAULT_BOOK_THUMBNAIL.to_string()
        }
    }
}
```

Add a feature-gated Pdfium implementation:

```rust
#[cfg(feature = "pdf-thumbnails")]
fn render_pdf_first_page(path: &Path, thumbnail_dir: &Path) -> Result<String> {
    use pdfium_render::prelude::*;

    std::fs::create_dir_all(thumbnail_dir)?;
    let pdfium = Pdfium::default();
    let document = pdfium.load_pdf_from_file(path, None)?;
    let page = document.pages().first()?;
    let image = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(480).set_maximum_height(720))?
        .as_image()?;
    let output_name = format!(
        "{}_pdf_cover.jpg",
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("book").replace(' ', "_")
    );
    image
        .into_rgb8()
        .save_with_format(thumbnail_dir.join(&output_name), image::ImageFormat::Jpeg)
        .map_err(|_| PdfiumError::ImageError)?;
    Ok(output_name)
}

#[cfg(not(feature = "pdf-thumbnails"))]
fn render_pdf_first_page(_path: &Path, _thumbnail_dir: &Path) -> Result<String> {
    anyhow::bail!("pdf thumbnail rendering feature is disabled")
}
```

- [ ] **Step 4: Export PDF functions**

In `src/domain/services/mod.rs`, extend the book metadata export:

```rust
pub use book_metadata::{
    extract_epub_cover, extract_epub_metadata, extract_pdf_metadata,
    render_pdf_thumbnail_or_default, DefaultPdfThumbnailRenderer, ExtractedBookMetadata,
    PdfThumbnailRenderer,
};
```

- [ ] **Step 5: Run PDF tests without Pdfium**

Run:

```bash
cargo test --no-default-features --features webserver extracts_pdf_metadata
cargo test --no-default-features --features webserver pdf_thumbnail_failure_uses_default_thumbnail
```

Expected: PASS. These tests must not require a Pdfium library.

- [ ] **Step 6: Commit**

```bash
git add src/domain/services/book_metadata.rs src/domain/services/mod.rs
git commit -m "feat: extract pdf book metadata"
```

### Task 8: Book Ingestion and MetadataManager Routing

**Files:**
- Modify: `src/domain/services/book_metadata.rs`
- Modify: `src/domain/services/mod.rs`
- Modify: `src/services/video_information.rs`
- Modify: `src/entrypoints/tvserver.rs`
- Modify: `src/domain/algorithm/video_utils.rs`

- [ ] **Step 1: Write failing routing tests**

In `src/domain/algorithm/video_utils.rs`, update `test_skip_file` expectations and add book cases:

```rust
#[test]
fn test_skip_file_accepts_books() {
    assert!(!skip_file("Dune.pdf"));
    assert!(!skip_file("Dune.epub"));
    assert!(skip_file("cover.jpg"));
}
```

In `src/services/video_information.rs`, add a unit-testable helper and test:

```rust
#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::domain::algorithm::{BookFormat, MediaKind};
    use std::path::Path;

    #[test]
    fn routes_completed_files_by_extension() {
        assert_eq!(media_kind_for_completed_path(Path::new("movie.mp4")), MediaKind::Video);
        assert_eq!(media_kind_for_completed_path(Path::new("book.pdf")), MediaKind::Book(BookFormat::Pdf));
        assert_eq!(media_kind_for_completed_path(Path::new("book.epub")), MediaKind::Book(BookFormat::Epub));
        assert_eq!(media_kind_for_completed_path(Path::new("cover.jpg")), MediaKind::Unsupported);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test --no-default-features --features webserver test_skip_file_accepts_books
cargo test --no-default-features --features webserver routes_completed_files_by_extension
```

Expected: FAIL until `skip_file` and `media_kind_for_completed_path()` are updated.

- [ ] **Step 3: Update `skip_file` to use media classifier**

In `src/domain/algorithm/video_utils.rs`, replace extension acceptance logic inside `skip_file()` with:

```rust
let path = Path::new(name);
!is_supported_media_path(path)
```

Add import:

```rust
use crate::domain::algorithm::is_supported_media_path;
```

Keep the existing early skip checks for dot files, `.tmp.mp4`, `TV`, `downloads`, and `Downloads`.

- [ ] **Step 4: Add book ingestion function**

In `src/domain/services/book_metadata.rs`, add:

```rust
use crate::domain::algorithm::{get_collection_and_file_from_root, BookFormat};
use crate::domain::config::{get_book_dir, get_book_thumbnail_dir};
use crate::domain::models::{BookDetails, BookMetadata, BookState, DEFAULT_BOOK_THUMBNAIL};
use crate::domain::services::calculate_checksum;
use crate::domain::traits::{FileStorer, Repository};
use tokio::fs;

pub async fn generate_book_metadata(
    path: PathBuf,
    book_file_storer: FileStorer,
    repo: Repository,
    suggested_collection: Option<String>,
    format: BookFormat,
) -> Result<Option<BookDetails>> {
    if is_file_being_written(&path).await.unwrap_or(false) {
        tracing::info!("Skipping book still being written: {}", path.display());
        return Ok(None);
    }

    let metadata = fs::metadata(&path).await?;
    if metadata.len() == 0 {
        anyhow::bail!("zero-byte book file: {}", path.display());
    }

    let checksum = calculate_checksum(&path).await?;
    let (collection, file_name) = get_collection_and_file_from_root(&path, &get_book_dir());
    let mut details = BookDetails::new(file_name, collection, &path, format);
    details.checksum = checksum;
    details.search_phrase = suggested_collection.clone();

    let extracted = match format {
        BookFormat::Epub => extract_epub_metadata(&path),
        BookFormat::Pdf => extract_pdf_metadata(&path),
    };

    match extracted {
        Ok(extracted) => apply_extracted_metadata(&mut details, extracted),
        Err(err) => {
            details.state = BookState::MetadataError;
            details.metadata = BookMetadata {
                extraction_error: Some(err.to_string()),
                raw: None,
            };
        }
    }

    let thumbnail_dir = get_book_thumbnail_dir(&get_book_dir());
    details.thumbnail = match format {
        BookFormat::Epub => extract_epub_cover(&path, &thumbnail_dir).unwrap_or_else(|err| {
            tracing::warn!("using default EPUB thumbnail for {}: {}", path.display(), err);
            DEFAULT_BOOK_THUMBNAIL.to_string()
        }),
        BookFormat::Pdf => render_pdf_thumbnail_or_default(
            &path,
            &thumbnail_dir,
            &DefaultPdfThumbnailRenderer,
        ),
    };

    let new_path = move_book_file(book_file_storer, &details.get_full_path(), suggested_collection).await?;
    let (collection, file_name) = get_collection_and_file_from_root(&new_path, &get_book_dir());
    details.collection = collection;
    details.file_name = file_name;
    details.dir_path = None;

    if details.state != BookState::MetadataError {
        details.state = BookState::Ready;
    }

    repo.save_book(&details).await?;
    Ok(Some(details))
}

async fn move_book_file(
    store: FileStorer,
    full_path: &Path,
    suggested_collection: Option<String>,
) -> Result<PathBuf> {
    let collection = match suggested_collection {
        Some(collection) => crate::domain::algorithm::title_case(&collection),
        None => crate::domain::algorithm::get_collection_from_root(full_path, &get_book_dir()),
    };
    let dest_dir = Path::new(&get_book_dir()).join(collection);
    store.create_folder(&dest_dir).await?;
    let dest_path = dest_dir.join(full_path.file_name().unwrap_or_default());

    if full_path == dest_path {
        return Ok(dest_path);
    }

    store
        .rename(
            full_path.to_str().unwrap_or_default(),
            dest_path.to_str().unwrap_or_default(),
        )
        .await?;
    Ok(dest_path)
}

fn apply_extracted_metadata(details: &mut BookDetails, extracted: ExtractedBookMetadata) {
    if let Some(title) = extracted.title {
        details.title = title;
    }
    details.authors = extracted.authors;
    details.description = extracted.description;
    details.publisher = extracted.publisher;
    details.published_date = extracted.published_date;
    details.language = extracted.language;
    details.isbn = extracted.isbn;
    details.page_count = extracted.page_count;
    details.metadata.raw = extracted.raw;
}

async fn is_file_being_written(path: &Path) -> std::io::Result<bool> {
    let initial_size = fs::metadata(path).await?.len();
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    let current_size = fs::metadata(path).await?.len();
    Ok(initial_size != current_size)
}
```

- [ ] **Step 5: Export ingestion function**

In `src/domain/services/mod.rs`, export:

```rust
generate_book_metadata,
```

- [ ] **Step 6: Route in `MetaDataManager`**

In `src/services/video_information.rs`, add imports:

```rust
use crate::domain::algorithm::{classify_media_path, BookFormat, MediaKind};
use crate::domain::services::generate_book_metadata;
use crate::domain::traits::FileStorer;
```

Add helper:

```rust
fn media_kind_for_completed_path(path: &std::path::Path) -> MediaKind {
    classify_media_path(path)
}
```

Add a `book_file_storer: FileStorer` field to `MetaDataManager`. Update `MetaDataManager::new()` and `MetaDataManager::consume()` to accept that field, and clone it into the spawned task:

```rust
let book_file_storer = self.book_file_storer.clone();
```

Replace the processing call in the spawned task:

```rust
let result = match media_kind_for_completed_path(&full_path) {
    MediaKind::Video => {
        generate_video_metadatas(full_path, storer, repo, search, spawner)
            .await
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!(err.to_string()))
    }
    MediaKind::Book(format) => {
        generate_book_metadata(full_path, book_file_storer, repo, search, format)
            .await
            .map(|_| ())
    }
    MediaKind::Unsupported => {
        tracing::info!("Skipping unsupported media file: {:?}", full_path);
        Ok(())
    }
};
```

In `src/entrypoints/tvserver.rs`, update the `MetaDataManager::consume()` call to pass the book file store after `context.get_store()`:

```rust
context.get_book_file_storer(),
```

- [ ] **Step 7: Run routing tests**

Run:

```bash
cargo test --no-default-features --features webserver test_skip_file_accepts_books
cargo test --no-default-features --features webserver routes_completed_files_by_extension
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/domain/services/book_metadata.rs src/domain/services/mod.rs src/services/video_information.rs src/entrypoints/tvserver.rs src/domain/algorithm/video_utils.rs
git commit -m "feat: route book downloads to book metadata processing"
```

### Task 9: Book Idle Scanner

**Files:**
- Create: `src/domain/services/book_check.rs`
- Modify: `src/domain/services/mod.rs`
- Modify: `src/domain/traits.rs`
- Modify: `src/services/monitor.rs`
- Modify: `src/entrypoints/context.rs`
- Modify: `src/entrypoints/tvserver.rs`

- [ ] **Step 1: Write failing scanner trait and monitor tests**

In `src/domain/services/book_check.rs`, add tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::{FileSystemStore, SqlRepository};
    use crate::domain::config::BOOK_DIR;
    use crate::domain::messagebus::LocalMessageExchange;
    use crate::domain::traits::{FileStorer, Repository};
    use std::env;
    use std::sync::Arc;

    #[tokio::test]
    async fn book_check_queues_new_pdf_files() {
        env::set_var(BOOK_DIR, "tests/fixtures/book_dir");
        let repo: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let file_store: FileStorer = Arc::new(FileSystemStore::new("tests/fixtures/book_dir"));
        let exchange = LocalMessageExchange::new();
        let checker = BookCheck::new(file_store, repo, exchange.new_sender());

        checker.check_book_information().await.unwrap();
    }
}
```

- [ ] **Step 2: Run scanner test to verify it fails**

Run:

```bash
cargo test --no-default-features --features webserver book_check_queues_new_pdf_files
```

Expected: FAIL because `BookCheck` and `BookChecker` are missing.

- [ ] **Step 3: Add scanner trait**

In `src/domain/traits.rs`, add:

```rust
#[automock]
#[async_trait]
pub trait BookChecker: Send + Sync {
    async fn check_book_information(&self) -> anyhow::Result<()>;
}

pub type BookScanner = Arc<dyn BookChecker>;
```

- [ ] **Step 4: Implement `BookCheck`**

Create `src/domain/services/book_check.rs`:

```rust
use async_recursion::async_recursion;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::domain::algorithm::{classify_media_path, get_collection_from_root, MediaKind};
use crate::domain::config::get_book_dir;
use crate::domain::messages::{LocalMessage, LocalMessageSender, MediaEvent};
use crate::domain::models::BookDetails;
use crate::domain::services::calculate_checksum;
use crate::domain::traits::{BookChecker, FileStorer, Repository};

#[derive(Clone)]
pub struct BookCheck {
    store: FileStorer,
    repo: Repository,
    sender: LocalMessageSender,
}

impl BookCheck {
    pub fn new(store: FileStorer, repo: Repository, sender: LocalMessageSender) -> Self {
        Self { store, repo, sender }
    }

    async fn queue_book_info(&self, path: &Path) {
        let event = MediaEvent::new_media(path, None);
        if let Err(e) = self.sender.send(LocalMessage::Media(event)).await {
            tracing::error!("could not queue book Media event: {}", e);
        }
    }

    #[async_recursion]
    async fn process_directory(&self, dir_path: PathBuf) -> anyhow::Result<()> {
        let collection = get_collection_from_root(&dir_path, &get_book_dir());
        let mut current_books = self.repo.list_books(&collection).await?;
        let (directories, files) = self.store.list_folder(&collection).await?;

        for directory in directories {
            self.process_directory(dir_path.join(directory)).await?;
        }

        for file in files {
            let full_path = dir_path.join(&file);
            if !matches!(classify_media_path(&full_path), MediaKind::Book(_)) {
                continue;
            }

            let mut existing = current_books
                .iter()
                .filter_map(|item| if item.file_name == file { Some((item.checksum, item)) } else { None })
                .collect::<HashMap<_, _>>();

            if existing.is_empty() {
                self.queue_book_info(&full_path).await;
                continue;
            }

            if existing.len() > 1 {
                let checksum = calculate_checksum(&full_path).await?;
                if let Some(item) = existing.get(&checksum) {
                    existing = HashMap::from([(item.checksum, *item)]);
                } else {
                    self.queue_book_info(&full_path).await;
                    continue;
                }
            }

            let (_, current) = existing.iter().next().unwrap();
            if current.should_retry_metadata() {
                self.queue_book_info(&full_path).await;
            }

            let checksum = current.checksum;
            current_books = current_books
                .into_iter()
                .filter(|item| item.checksum != checksum || item.file_name != file)
                .collect();
        }

        self.delete_orphaned_records(current_books).await;
        Ok(())
    }

    async fn find_orphaned_records(&self) -> anyhow::Result<()> {
        let collections = self.repo.list_book_collections("").await?;
        for collection in collections {
            let books = self.repo.list_books(&collection).await?;
            if books.is_empty() {
                continue;
            }
            let disk_files: HashSet<String> = match self.store.list_folder(&collection).await {
                Ok((_dirs, files)) => files.into_iter().collect(),
                Err(_) => HashSet::new(),
            };
            let orphans: Vec<BookDetails> = books
                .into_iter()
                .filter(|book| !disk_files.contains(&book.file_name))
                .collect();
            self.delete_orphaned_records(orphans).await;
        }
        Ok(())
    }

    async fn delete_orphaned_records(&self, books: Vec<BookDetails>) {
        for book in books {
            if let Err(err) = self.repo.delete_book(book.checksum).await {
                tracing::error!("error deleting book record {}: {}", book.file_name, err);
            }
        }
    }
}

#[async_trait]
impl BookChecker for BookCheck {
    async fn check_book_information(&self) -> anyhow::Result<()> {
        self.process_directory(PathBuf::from(&get_book_dir())).await?;
        self.find_orphaned_records().await
    }
}
```

- [ ] **Step 5: Export scanner**

In `src/domain/services/mod.rs`, add:

```rust
mod book_check;
pub use book_check::BookCheck;
```

- [ ] **Step 6: Run book scanner from monitor**

In `src/services/monitor.rs`, update imports:

```rust
use crate::domain::traits::{BookScanner, Checker, Storer};
```

Add field:

```rust
book_checker: BookScanner,
```

Update `Monitor::start()` to accept `book_checker: BookScanner` and assign it. In the loop after video check:

```rust
if let Err(err) = &monitor.book_checker.check_book_information().await {
    tracing::error!("error checking book info: {}", err);
}
```

In `src/entrypoints/context.rs`, add `book_checker: BookScanner` to `Context`, constructor, and getter:

```rust
pub fn get_book_checker(&self) -> BookScanner {
    self.book_checker.clone()
}
```

In `create_context()`, construct:

```rust
let book_checker = Arc::new(BookCheck::new(
    Arc::new(FileSystemStore::new(&get_book_dir())),
    repository.clone(),
    local_message_exchange.new_sender(),
));
```

Pass it into `Context::new()`.

In `src/entrypoints/tvserver.rs`, pass it to `Monitor::start()`:

```rust
context.get_book_checker(),
```

Update `tests/common/context.rs` to construct a `BookCheck` for tests and pass it into `Context::new()`.

- [ ] **Step 7: Run scanner test**

Run:

```bash
cargo test --no-default-features --features webserver book_check_queues_new_pdf_files
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/domain/services/book_check.rs src/domain/services/mod.rs src/domain/traits.rs src/services/monitor.rs src/entrypoints/context.rs src/entrypoints/tvserver.rs tests/common/context.rs
git commit -m "feat: scan book library"
```

### Task 10: REST API and Static Book Serving

**Files:**
- Modify: `src/entrypoints/api.rs`
- Modify: `src/entrypoints/webserver.rs`
- Modify: `tests/common/server.rs`
- Create: `tests/book_api_test.rs`
- Create: `tests/fixtures/book_dir/.thumbnails/.keep`

- [ ] **Step 1: Write failing REST API test**

Create `tests/book_api_test.rs`:

```rust
mod common;

use anyhow::Result;
use app_lib::domain::algorithm::BookFormat;
use app_lib::domain::config::{BOOK_DIR, MOVIE_DIR};
use app_lib::domain::models::{BookDetails, BookState};
use crate::common::{get_checker, get_context, get_media_store, get_pirate_search, get_repository, get_task_manager};
use std::env;

#[tokio::test]
async fn test_book_api_lists_and_retrieves_books() -> Result<()> {
    env::set_var(MOVIE_DIR, "tests/fixtures/media_dir");
    env::set_var(BOOK_DIR, "tests/fixtures/book_dir");

    let repo = get_repository().await;
    repo.save_book(&BookDetails {
        checksum: 777,
        file_name: "Dune.pdf".to_string(),
        collection: "Sci-Fi".to_string(),
        title: "Dune".to_string(),
        authors: vec!["Frank Herbert".to_string()],
        format: BookFormat::Pdf,
        state: BookState::Ready,
        ..Default::default()
    }).await?;

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;
    let context = get_context(get_media_store(), searcher, get_task_manager(), repo, get_checker()).await?;
    let server = common::create_server(context, 57191).await;
    let client = reqwest::Client::new();

    let collection: serde_json::Value = client
        .get("http://localhost:57191/api/books/Sci-Fi")
        .send()
        .await?
        .json()
        .await?;

    assert_eq!(collection["books"][0]["title"], "Dune");

    let book: serde_json::Value = client
        .get("http://localhost:57191/api/book/777")
        .send()
        .await?
        .json()
        .await?;

    assert_eq!(book["title"], "Dune");

    server.abort();
    Ok(())
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --no-default-features --features webserver test_book_api_lists_and_retrieves_books
```

Expected: FAIL with 404 or missing handlers.

- [ ] **Step 3: Add REST routes and handlers**

In `src/entrypoints/api.rs`, add routes in `register()`:

```rust
.route("/api/books", get(list_root_books))
.route("/api/books/{*collection}", get(list_books))
.route("/api/book/{checksum}", get(get_book))
.route("/api/book/{checksum}", delete(delete_book))
```

Add handlers:

```rust
#[debug_handler]
async fn list_root_books(state: State<SharedState>) -> impl IntoResponse {
    list_book_collection(&state, "").await
}

#[debug_handler]
async fn list_books(state: State<SharedState>, collection: Path<String>) -> impl IntoResponse {
    list_book_collection(&state, &collection).await
}

async fn list_book_collection(
    state: &SharedState,
    collection: &str,
) -> (StatusCode, Json<crate::domain::models::BookCollectionDetails>) {
    match state.get_book_store().list(collection).await {
        Ok(result) => (OK, Json(result)),
        Err(e) => (
            NOT_FOUND,
            Json(crate::domain::models::BookCollectionDetails {
                errors: vec![e.to_string()],
                ..Default::default()
            }),
        ),
    }
}

#[debug_handler]
async fn get_book(state: State<SharedState>, checksum: Path<i64>) -> impl IntoResponse {
    match state.get_repository().retrieve_book(checksum.0).await {
        Ok(book) => (OK, Json(book)).into_response(),
        Err(e) => std_error(NOT_FOUND, e.to_string()).into_response(),
    }
}

#[debug_handler]
async fn delete_book(state: State<SharedState>, checksum: Path<i64>) -> StdResponse {
    match state.get_book_store().delete(checksum.0).await {
        Ok(()) => (OK, Json(Response::success("success".to_string()))),
        Err(e) => std_error(INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
```

- [ ] **Step 4: Serve book files and thumbnails**

In `src/entrypoints/webserver.rs`, import:

```rust
use crate::domain::config::{get_book_dir, get_book_thumbnail_dir, get_client_path, get_movie_dir, get_thumbnail_dir};
```

Add to `unprotected_routes`:

```rust
.nest_service("/api/books/download", ServeDir::new(get_book_dir()))
.nest_service(
    "/api/book-thumbnails",
    ServeDir::new(get_book_thumbnail_dir(&get_book_dir())),
)
```

In `tests/common/server.rs`, import book config and add the same static routes to the test router:

```rust
use app_lib::domain::config::{get_book_dir, get_book_thumbnail_dir, get_client_path, get_movie_dir};

.nest_service("/api/books/download", ServeDir::new(get_book_dir()))
.nest_service("/api/book-thumbnails", ServeDir::new(get_book_thumbnail_dir(&get_book_dir())))
```

Create fixture directories:

```bash
mkdir -p tests/fixtures/book_dir/.thumbnails
touch tests/fixtures/book_dir/.thumbnails/.keep
```

- [ ] **Step 5: Run REST API test**

Run:

```bash
cargo test --no-default-features --features webserver test_book_api_lists_and_retrieves_books
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/entrypoints/api.rs src/entrypoints/webserver.rs tests/common/server.rs tests/book_api_test.rs tests/fixtures/book_dir/.thumbnails/.keep
git commit -m "feat: expose book REST API"
```

### Task 11: Tauri Commands

**Files:**
- Modify: `src/entrypoints/tauri_api.rs`

- [ ] **Step 1: Add command functions**

In `src/entrypoints/tauri_api.rs`, add imports under `#[cfg(not(feature = "webserver"))]`:

```rust
use crate::domain::models::{BookCollectionDetails, BookDetails};
```

Add commands:

```rust
#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_root_books(
    state: tauri::State<'_, SharedState>
) -> Result<BookCollectionDetails, String> {
    state.get_book_store().list("").await.map_err(|e| e.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_books(
    state: tauri::State<'_, SharedState>,
    collection: String
) -> Result<BookCollectionDetails, String> {
    state.get_book_store().list(&collection).await.map_err(|e| e.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn get_book(
    state: tauri::State<'_, SharedState>,
    checksum: String
) -> Result<BookDetails, String> {
    let checksum = checksum.parse::<i64>().map_err(|e| format!("Invalid book ID: {}", e))?;
    state.get_repository().retrieve_book(checksum).await.map_err(|e| e.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn delete_book(
    state: tauri::State<'_, SharedState>,
    checksum: String
) -> Result<Response, String> {
    let checksum = checksum.parse::<i64>().map_err(|e| format!("Invalid book ID: {}", e))?;
    state
        .get_book_store()
        .delete(checksum)
        .await
        .map(|_| Response::success("success".to_string()))
        .map_err(|e| e.to_string())
}
```

Add these functions to `tauri::generate_handler!`:

```rust
list_root_books,
list_books,
get_book,
delete_book,
```

- [ ] **Step 2: Build non-webserver target**

Run:

```bash
cargo check
```

Expected: PASS. This checks Tauri command compilation under default features.

- [ ] **Step 3: Build webserver target**

Run:

```bash
cargo check --no-default-features --features webserver
```

Expected: PASS. This ensures the `#[cfg(not(feature = "webserver"))]` command changes did not affect the webserver build.

- [ ] **Step 4: Commit**

```bash
git add src/entrypoints/tauri_api.rs
git commit -m "feat: add Tauri book commands"
```

### Task 12: Verification Pass

**Files:**
- Modify only if verification finds defects in files touched by earlier tasks.

- [ ] **Step 1: Run focused book tests**

Run:

```bash
cargo test --no-default-features --features webserver book
```

Expected: PASS.

- [ ] **Step 2: Run existing regression tests most likely affected by routing/API changes**

Run:

```bash
cargo test --no-default-features --features webserver test_skip_file
cargo test --no-default-features --features webserver test_sending_messages
cargo test --no-default-features --features webserver test_video_stream
cargo test --no-default-features --features webserver test_pirate_download
```

Expected: PASS.

- [ ] **Step 3: Run full webserver test suite**

Run:

```bash
cargo test --no-default-features --features webserver
```

Expected: PASS.

- [ ] **Step 4: Run default build check for Tauri**

Run:

```bash
cargo check
```

Expected: PASS.

- [ ] **Step 5: Commit verification fixes if any were needed**

If no files changed, do not create an empty commit. If fixes were needed:

```bash
git add <fixed-files>
git commit -m "fix: stabilize ebook support"
```

## Spec Coverage Map

- PDF and EPUB support: Tasks 1, 6, 7, and 8.
- Separate `BOOK_DIR`: Tasks 1, 5, 9, and 10.
- New `books` table and repository: Task 4.
- Metadata extraction: Tasks 6 and 7.
- Default thumbnail fallback: Tasks 2, 6, 7, and 10.
- REST API: Task 10.
- Tauri commands: Task 11.
- Android dependency policy: Tasks 1 and 7 avoid desktop-only binaries and make Pdfium optional.
- Existing video behavior preserved: Tasks 8 and 12.
- Spec branch discipline: top section of this plan and `docs/superpowers/specs/2026-07-13-book-library-design.md`.
