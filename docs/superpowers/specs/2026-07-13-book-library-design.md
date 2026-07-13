# Book Library Backend Design

## Summary

Extend the backend so completed downloads can be processed as books as well as videos. The first book release supports PDF and EPUB files, stores them in a separate `BOOK_DIR`, extracts title/author and other practical metadata, assigns a thumbnail, exposes a new REST API under `/api/books`, and adds matching Tauri commands for desktop and Android clients.

Existing video download sources, video processing, `/api/media`, and video response contracts remain unchanged.

## Goals

- Support `.pdf` and `.epub` files downloaded through the existing download sources.
- Store books separately from videos using a new `BOOK_DIR` configuration value.
- Persist books in a new SQLite table.
- Extract book title, author, and relevant metadata where available.
- Use an extracted cover or first-page thumbnail when possible.
- Assign a bundled default book cover image whenever thumbnail extraction fails or no cover exists.
- Expose books through dedicated REST routes and matching Tauri commands.
- Keep Android/Tauri viability by avoiding desktop-only external binaries.

## Non-Goals

- No MOBI, AZW, CBZ, or CBR support in the first pass.
- No frontend UI redesign in this backend-focused spec.
- No migration of video data into a generalized media table.
- No changes to search providers or download source behavior.
- No dependency on desktop-only tools such as `pdftoppm`, `mutool`, or `exiftool`.

## Architecture

Books are a sibling domain to videos. The implementation adds book-specific models, repository methods, processing, events, and API handlers while leaving the existing video model intact.

New backend pieces:

- `BOOK_DIR` config for stored ebook files.
- `BOOK_THUMBNAIL_DIR` config for book covers, defaulting to `<BOOK_DIR>/.thumbnails`.
- `books` SQLite table.
- `BookDetails` domain model.
- `BookMetadata` domain model for extracted metadata.
- `BookState` enum for ingestion state.
- Repository methods beside the existing video methods.
- `BookEvent` and `LocalMessage::Book` for client refresh events.
- `MetaDataManager` routing by file extension.
- REST routes under `/api/books` and `/api/book`.
- Tauri commands that mirror the REST read/delete operations.

The current `MediaEvent::MediaAvailable` remains the download-completion event. Completed files are classified by extension inside `MetaDataManager`:

- Known video extensions call the existing `generate_video_metadatas` path.
- `.pdf` and `.epub` call the new `generate_book_metadata` path.
- Unsupported extensions are skipped with a log entry.

## Data Model

Create a new `books` table rather than widening `video_details`.

Proposed columns:

- `checksum INTEGER PRIMARY KEY NOT NULL`
- `file_name TEXT NOT NULL`
- `collection TEXT NOT NULL`
- `title TEXT NOT NULL`
- `authors TEXT`
- `description TEXT`
- `publisher TEXT`
- `published_date TEXT`
- `language TEXT`
- `isbn TEXT`
- `format TEXT NOT NULL`
- `page_count INTEGER`
- `thumbnail TEXT NOT NULL`
- `metadata TEXT`
- `search_phrase TEXT`
- `state INTEGER DEFAULT 0`
- `created_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP`
- `updated_on TIMESTAMP DEFAULT CURRENT_TIMESTAMP`

Indexes:

- Unique index on `(collection, file_name)`.
- Index on `title`.
- Index on `authors`.

`authors` is stored as JSON text to preserve multiple authors while keeping the schema simple. `metadata` stores raw or semi-structured extractor output as JSON text where useful.

`thumbnail` stores the generated thumbnail filename or the default thumbnail filename. Serialized API responses convert that filename into a `/api/book-thumbnails/...` URL for webserver builds and a local path for Tauri builds, matching the existing video thumbnail pattern.

## Storage

Books are moved into `BOOK_DIR/<collection>/<file_name>`.

Collection selection:

- If a download request provides `series`, use its title-cased value as the book collection.
- Otherwise derive the collection from the source parent directory, equivalent to the existing video behavior.
- If no collection can be derived, store at the `BOOK_DIR` root.

Book thumbnails are stored separately from video thumbnails in `BOOK_THUMBNAIL_DIR`. The default cover image is treated as a stable thumbnail value and served from the same route as generated book thumbnails, such as `/api/book-thumbnails/default-book.jpg`.

## Metadata Extraction

### EPUB

Use Rust-native ZIP/XML parsing rather than the GPL-licensed `epub` crate.

Extraction steps:

- Open the EPUB as a ZIP archive.
- Read `META-INF/container.xml`.
- Resolve the package document path.
- Parse package metadata for `title`, `creator`, `description`, `publisher`, `date`, `language`, and identifiers such as ISBN.
- Resolve the cover item from package metadata or manifest properties.
- Copy or transcode the cover image into the book thumbnail directory.
- Use the default book cover if no cover is present or extraction fails.

### PDF

Use Rust-native PDF parsing for metadata. A suitable parser is `lopdf`, which is MIT licensed and focuses on PDF document manipulation.

Extraction steps:

- Open the PDF.
- Read document info metadata such as title, author, subject, keywords, creation date, and page count where available.
- Fall back to filename-derived title and empty authors when metadata is weak or absent.

PDF thumbnail generation uses a pluggable renderer boundary:

- Primary implementation: Pdfium-backed renderer through `pdfium-render` when a Pdfium library is packaged or otherwise available for the target.
- Android requirement: the renderer must use an Android-compatible packaged library or a path that is straightforward to compile for Android.
- Fallback implementation: assign `default-book.jpg` without failing ingestion.

The renderer is optional from the ingestion perspective. A missing Pdfium library is a warning, not a failed book import.

## Data Flow

1. Existing download source completes a file and emits `MediaEvent::MediaAvailable(path, search)`.
2. `MetaDataManager` receives the event and prevents duplicate processing for the same path.
3. `MetaDataManager` classifies the file:
   - video -> existing video processing
   - PDF/EPUB -> book processing
   - unsupported -> skip
4. Book processing checks file stability and zero-byte status.
5. Book processing computes the checksum.
6. Book processing extracts metadata and thumbnail.
7. Book processing moves the file into `BOOK_DIR`.
8. Repository upserts the `BookDetails` row.
9. Repository emits `BookEventAdded` or `BookEventChanged`.
10. REST, Tauri, and websocket clients can refresh book views from the new book API.

Idle scanning also needs to cover `BOOK_DIR`. Add a book-specific scan that detects new books, retries records in `NeedMetadata` or `MetadataError` state, and removes orphaned book rows when files are gone. Keep this separate from the existing video scan in the first pass to avoid renaming video-specific methods across the codebase.

## Repository API

Add methods to the repository trait and `SqlRepository` implementation:

- `save_book(&self, details: &BookDetails) -> Result<i64, sqlx::Error>`
- `list_book_collections(&self, parent_collection: &str) -> Result<Vec<String>, sqlx::Error>`
- `list_books(&self, collection: &str) -> Result<Vec<BookDetails>, sqlx::Error>`
- `retrieve_book(&self, checksum: i64) -> Result<BookDetails, sqlx::Error>`
- `delete_book(&self, checksum: i64) -> Result<u64, sqlx::Error>`
- `list_all_books(&self) -> Result<Vec<BookDetails>, sqlx::Error>`

The book upsert behavior mirrors video upsert behavior:

- Conflict on `(collection, file_name)` updates the existing row.
- Conflict on `checksum` updates path and metadata fields.
- Added rows emit `BookEventAdded`.
- Updated rows emit `BookEventChanged`.
- Deleted rows emit `BookEventDeleted`.

## API

Add dedicated REST routes:

- `GET /api/books`
  - Lists root book collections.
- `GET /api/books/{*collection}`
  - Lists nested book collections and books for the collection.
- `GET /api/book/{checksum}`
  - Returns one book record.
- `DELETE /api/book/{checksum}`
  - Deletes the book file, thumbnail when generated, and database row.
- `GET /api/books/download/{*path}`
  - Serves stored book files from `BOOK_DIR`.
- `GET /api/book-thumbnails/{file}`
  - Serves generated book thumbnails and the default book cover.

Add matching Tauri commands:

- `list_root_books()`
- `list_books(collection: String)`
- `get_book(checksum: String)`
- `delete_book(checksum: String)`

Response types:

- `BookItem` for a single book.
- `BookCollectionDetails` for collection listings.
- `BookEvent` for realtime updates.

Do not widen `MediaItem` or alter `/api/media` response payloads in this first pass.

## Error Handling

Unsupported file extension:

- Skip and log.
- Do not create a database row.

Zero-byte file:

- Fail ingestion.
- Do not create a database row.

Metadata parse failure:

- Fall back to filename-derived title.
- Use empty authors.
- Save the book with a metadata-error state.
- Record the parse error in structured metadata if useful.

PDF thumbnail failure, EPUB cover missing, or Pdfium unavailable:

- Assign `default-book.jpg`.
- Save the book normally.
- Log the original thumbnail problem as a warning.

File move failure or repository save failure:

- Fail ingestion.
- Do not emit a `BookEvent`.

Generated thumbnail delete failure:

- Delete the book row and file.
- Log the thumbnail cleanup failure.
- Never delete the default cover image.

## Android and Dependency Policy

External processing tools must be Python-based or easy to compile/package for Android. The book design avoids desktop-only binaries.

Allowed dependency direction:

- Rust crates for EPUB ZIP/XML parsing.
- Rust crates for PDF metadata parsing.
- Optional Pdfium-backed rendering through an Android-compatible packaged Pdfium library.
- Python helper scripts only if Rust-native extraction proves insufficient.

Disallowed for this feature:

- Desktop-only command-line PDF renderers.
- System package assumptions that do not hold on Android.
- Failing book ingestion solely because a PDF renderer is unavailable.

Research notes:

- `pdfium-render` provides Rust bindings to Pdfium and documents rendering PDF pages to bitmap images plus dynamic/static Pdfium binding options.
- `lopdf` is an MIT-licensed Rust library for PDF document manipulation.

## Testing

Repository tests:

- `save_book` inserts and retrieves PDF records.
- `save_book` inserts and retrieves EPUB records.
- `save_book` updates existing rows on `(collection, file_name)` conflict.
- `save_book` updates existing rows on `checksum` conflict.
- `list_book_collections` returns nested collections.
- `delete_book` removes the row and emits delete events when a sender exists.

Metadata tests:

- EPUB extraction reads title, author, language, publisher, and date from package metadata.
- EPUB cover extraction writes a thumbnail.
- EPUB without a cover assigns `default-book.jpg`.
- PDF metadata extraction reads title and author when present.
- PDF metadata extraction falls back to filename title when document metadata is absent.
- PDF thumbnail failure assigns `default-book.jpg`.
- Unsupported extension is skipped before video or book processing.

API tests:

- `GET /api/books` returns root book collections.
- `GET /api/books/{*collection}` returns nested collections and books.
- `GET /api/book/{checksum}` returns one book.
- `DELETE /api/book/{checksum}` deletes database and file state.
- Book download route serves files from `BOOK_DIR`.
- Book thumbnail route serves generated thumbnails and `default-book.jpg`.

Regression tests:

- Existing video ingestion still routes video extensions to `generate_video_metadatas`.
- Existing `/api/media` responses remain unchanged.
- Existing video delete/list behavior remains unchanged.

Android/dependency guard:

- Unit tests do not require desktop-only external binaries.
- PDF renderer tests use a mock or feature-gated renderer.
- Default-thumbnail fallback is tested unconditionally.

## Implementation Order

1. Add `BOOK_DIR`, `BOOK_THUMBNAIL_DIR`, and book domain models.
2. Add migration and repository methods.
3. Add book thumbnail/default-cover helpers.
4. Add EPUB metadata extraction.
5. Add PDF metadata extraction and renderer boundary.
6. Add book ingestion service.
7. Route `MetaDataManager` by file extension.
8. Add book scan for `BOOK_DIR`.
9. Add REST routes and static serving.
10. Add Tauri commands.
11. Add websocket/local book events.
12. Add focused tests and run existing video regression tests.
