# Ebook Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add backend support for PDF and EPUB books while preserving existing video download, processing, and API behavior.

**Architecture:** Books are a sibling domain to videos: separate `BOOK_DIR`, `books` table, book metadata pipeline, book repository methods, `/api/books` routes, and Tauri commands. `MetaDataManager` remains the download-completion choke point and routes files by extension to either existing video processing or new book processing.

**Tech Stack:** Rust 2021, Tokio, Axum, Tauri, SQLite/sqlx, Rust-native EPUB ZIP/XML parsing, Rust-native PDF metadata parsing, optional Android-compatible Pdfium-backed PDF thumbnail rendering, existing file-store, repository, local-message, and task-monitor patterns.

---

## Plan Style

This plan intentionally contains no implementation code. Each implementation worker is responsible for writing the tests and production code that satisfy the deliverables and acceptance criteria below, following the approved spec and existing codebase patterns.

## Spec Branch Discipline

This plan is committed on `spec/ebook-support`. Treat `spec/ebook-support` as the base branch for all implementation work. Do not create implementation branches directly from `main`.

Acceptance criteria:

- Every implementation branch starts from `spec/ebook-support`.
- Every implementation branch contains the approved spec and this plan in its history.
- Implementation work is split by the tasks below unless the maintainer explicitly chooses a different breakdown.
- Pull requests or integration branches call out which task or tasks they complete.

## File Responsibility Map

- `Cargo.toml`: dependency and feature declarations for EPUB/PDF/image handling.
- `src/domain/config.rs`: `BOOK_DIR`, `BOOK_THUMBNAIL_DIR`, and path helpers.
- `src/domain/algorithm/media_kind.rs`: file extension classification for video, book, and unsupported files.
- `src/domain/algorithm/naming.rs`: URL helpers and root-aware collection path helpers.
- `src/domain/models/book.rs`: book domain models, states, formats, collection response types, and serialization.
- `src/domain/messages/book_event.rs`: book add/change/delete events.
- `src/domain/messages/local.rs`: local message variant for book events.
- `src/domain/messagebus/local_message_exchange.rs`: local routing for book messages.
- `src/domain/messagebus/message_exchange.rs`: websocket broadcast behavior for book events.
- `migrations/20260713000001_books.sql`: SQLite schema for persisted books.
- `src/adaptors/repository.rs`: SQL repository implementation for book CRUD/list/upsert behavior.
- `src/domain/traits.rs`: repository and book-scanner trait additions.
- `src/services/book_store.rs`: book listing, movement, and deletion across disk and repository state.
- `src/domain/services/book_metadata.rs`: EPUB/PDF metadata extraction, thumbnail assignment, and book ingestion.
- `src/domain/services/book_check.rs`: idle scanner for `BOOK_DIR`.
- `src/services/video_information.rs`: event routing from completed downloads to video or book processing.
- `src/services/monitor.rs`: scheduled scan of both videos and books.
- `src/entrypoints/context.rs`: construction and exposure of book store, book checker, and book file store.
- `src/entrypoints/tvserver.rs`: wiring of book store/checker into runtime services.
- `src/entrypoints/api.rs`: REST book handlers.
- `src/entrypoints/webserver.rs`: static serving for book downloads and book thumbnails.
- `src/entrypoints/tauri_api.rs`: Tauri commands for book list/get/delete.
- `tests/common/context.rs`: test context construction with book services.
- `tests/common/server.rs`: test static serving for book routes.
- `tests/book_api_test.rs`: REST book API integration coverage.
- `tests/fixtures/book_dir/`: test book library fixture root.

## Task 1: Config, Dependencies, and Media Classification

**Deliverables**

- Add required `BOOK_DIR` configuration and optional `BOOK_THUMBNAIL_DIR` configuration.
- Add helper functions for the book root and book thumbnail directory.
- Add a media-kind classifier that recognizes existing video extensions, `.pdf`, `.epub`, and unsupported files.
- Add root-aware collection helpers so book collection derivation strips `BOOK_DIR` rather than `MOVIE_DIR`.
- Add dependencies for ZIP, XML, image, PDF metadata, and optional PDF thumbnail rendering without adding desktop-only binary dependencies.

**Files**

- Modify: `Cargo.toml`
- Modify: `src/domain/config.rs`
- Create: `src/domain/algorithm/media_kind.rs`
- Modify: `src/domain/algorithm/naming.rs`
- Modify: `src/domain/algorithm/mod.rs`

**Acceptance Criteria**

- `.pdf` and `.epub` classify as books, case-insensitively.
- Current video extensions still classify as video.
- Hidden files and unrelated extensions classify as unsupported.
- Book collection helpers return paths relative to `BOOK_DIR`.
- Video collection helpers continue to return paths relative to `MOVIE_DIR`.
- `BOOK_DIR` is required when book services are constructed.
- `BOOK_THUMBNAIL_DIR` defaults to `<BOOK_DIR>/.thumbnails`.
- New dependencies are Rust-native or optional Android-compatible native bindings.
- No desktop-only tools such as `pdftoppm`, `mutool`, or `exiftool` are introduced.

**Verification**

- Unit tests cover media classification.
- Unit tests cover book config behavior.
- Unit tests cover root-aware collection helpers.
- Existing `skip_file` tests still pass before book-specific behavior is changed later.

## Task 2: Book Domain Models, URLs, and Default Thumbnail Contract

**Deliverables**

- Add `BookFormat`, `BookState`, `BookMetadata`, `BookDetails`, `BookCollectionItem`, and `BookCollectionDetails`.
- Serialize book checksums as strings, matching video API behavior.
- Add book download URL and book thumbnail URL helpers.
- Define the stable default thumbnail filename, `default-book.jpg`.
- Define how the default thumbnail is made available at runtime and in tests.

**Files**

- Create: `src/domain/models/book.rs`
- Modify: `src/domain/models/mod.rs`
- Modify: `src/domain/algorithm/naming.rs`
- Modify: `src/domain/algorithm/mod.rs`
- Add fixture or asset location for `default-book.jpg`

**Acceptance Criteria**

- Book JSON includes title, authors, format, checksum string, URL, thumbnail URL, state, and timestamps.
- Optional fields are omitted or serialized consistently with existing API style.
- `BookDetails::get_full_path` resolves against `BOOK_DIR` when no transient source directory is present.
- `BookDetails::get_download_path` returns a path relative to `BOOK_DIR`.
- Default thumbnail handling is explicit and does not depend on thumbnail extraction success.
- The default cover image is never deleted during book deletion.

**Verification**

- Model serialization tests assert checksum string behavior.
- Model serialization tests assert URL and thumbnail URL generation.
- Tests cover default thumbnail naming.

## Task 3: Book Events and Local Message Routing

**Deliverables**

- Add `BookEventType` and `BookEvent`.
- Add `LocalMessage::Book`.
- Add a book-specific `MessageFilter`.
- Route book messages through `LocalMessageExchange`.
- Broadcast book events to websocket clients without changing existing video event payloads.

**Files**

- Create: `src/domain/messages/book_event.rs`
- Modify: `src/domain/messages/local.rs`
- Modify: `src/domain/messages/mod.rs`
- Modify: `src/domain/messagebus/local_message_exchange.rs`
- Modify: `src/domain/messagebus/message_exchange.rs`

**Acceptance Criteria**

- Book add, change, and delete events carry checksum strings.
- Add/change events include a book payload.
- Delete events do not require a book payload.
- Book event routing reaches `Book` and `All` subscribers.
- Book event routing does not reach `Video` subscribers.
- Existing media, task, video, and player-state routing behavior is unchanged.

**Verification**

- Local message exchange tests cover book message routing.
- Existing local message exchange tests continue to pass.

## Task 4: Books Migration and Repository Methods

**Deliverables**

- Add a `books` SQLite table.
- Add indexes for collection/file uniqueness, title lookup, and author lookup.
- Extend the repository trait with book CRUD/list methods.
- Implement book row mapping and upsert behavior in `SqlRepository`.
- Emit book events from repository insert/update/delete operations when a sender is configured.

**Files**

- Create: `migrations/20260713000001_books.sql`
- Modify: `src/domain/traits.rs`
- Modify: `src/adaptors/repository.rs`

**Acceptance Criteria**

- `save_book` inserts new records.
- `save_book` updates existing records on `(collection, file_name)` conflict.
- `save_book` updates existing records on `checksum` conflict.
- `retrieve_book` returns a single book by checksum.
- `list_books` returns books for one collection in stable title/file order.
- `list_book_collections` returns nested collection names using the existing collection-listing semantics.
- `list_all_books` returns all books in stable collection/title/file order.
- `delete_book` removes a row and emits a delete event when applicable.
- Authors and metadata are stored as structured JSON text and restored into domain types.
- Existing video repository behavior remains unchanged.

**Verification**

- Repository tests cover insert, retrieve, upsert, list, collection listing, and delete.
- Event-emission tests cover add/change/delete where practical.
- Existing repository tests continue to pass.

## Task 5: Book Store Service

**Deliverables**

- Add a `BookStore` service that lists book collections and books.
- Move downloaded book files into `BOOK_DIR/<collection>/`.
- Delete book files and generated thumbnails.
- Preserve the default thumbnail during deletion.
- Expose `BookStore` and the underlying `BOOK_DIR` file store through `Context`.

**Files**

- Create: `src/services/book_store.rs`
- Modify: `src/services/mod.rs`
- Modify: `src/entrypoints/context.rs`
- Modify: `tests/common/context.rs`

**Acceptance Criteria**

- Listing a collection returns child collections and books.
- Moving a book uses `BOOK_DIR`, not `MOVIE_DIR`.
- Suggested download series/search text determines collection when present.
- Without suggested collection, collection is derived from the source path relative to the appropriate root.
- Delete removes the book file, removes generated thumbnail files, preserves `default-book.jpg`, and deletes the repository row.
- File-store root protections are preserved.

**Verification**

- Service tests cover listing.
- Service tests cover file movement into `BOOK_DIR`.
- Service tests cover delete behavior for generated thumbnail and default thumbnail cases.

## Task 6: EPUB Metadata and Cover Extraction

**Deliverables**

- Extract EPUB package path from `META-INF/container.xml`.
- Parse EPUB package metadata for title, creator/author, description, publisher, date, language, and ISBN-like identifiers.
- Resolve cover image through EPUB metadata and manifest conventions.
- Copy or normalize cover images into `BOOK_THUMBNAIL_DIR`.
- Assign `default-book.jpg` when no cover exists or cover extraction fails.
- Avoid the GPL-licensed `epub` crate.

**Files**

- Create or modify: `src/domain/services/book_metadata.rs`
- Modify: `src/domain/services/mod.rs`
- Add EPUB fixtures under `tests/fixtures/book_dir` or generate them in tests.

**Acceptance Criteria**

- EPUB metadata extraction succeeds on a valid minimal EPUB.
- Multiple creators are preserved as multiple authors.
- Missing optional metadata does not fail ingestion.
- Missing cover image returns the default thumbnail.
- Invalid cover path returns the default thumbnail and records/logs the underlying problem.
- ZIP/XML parsing is Rust-native and Android-safe.

**Verification**

- Unit tests cover EPUB metadata extraction.
- Unit tests cover EPUB cover extraction.
- Unit tests cover missing cover fallback.
- Unit tests cover malformed or incomplete EPUB metadata fallback.

## Task 7: PDF Metadata and Thumbnail Renderer Boundary

**Deliverables**

- Extract PDF title, author, subject/description, and page count where available.
- Fall back to filename-derived title and empty authors when PDF metadata is absent.
- Add a PDF thumbnail renderer boundary.
- Add a default renderer that uses Pdfium only behind an optional feature.
- Ensure missing Pdfium or disabled PDF rendering assigns `default-book.jpg`.

**Files**

- Modify: `src/domain/services/book_metadata.rs`
- Modify: `src/domain/services/mod.rs`
- Modify: `Cargo.toml` if the optional renderer feature needs adjustment.

**Acceptance Criteria**

- PDF metadata extraction works without desktop command-line tools.
- PDF metadata extraction does not require Pdfium.
- PDF thumbnail fallback works without Pdfium.
- Optional Pdfium rendering can be enabled without changing ingestion semantics.
- The renderer boundary can be mocked or replaced in tests.
- Android builds are not forced to link against unavailable desktop libraries.

**Verification**

- Unit tests cover PDF metadata extraction.
- Unit tests cover metadata fallback when PDF info fields are absent.
- Unit tests cover renderer failure returning the default thumbnail.
- Optional renderer tests are feature-gated and not required for normal CI.

## Task 8: Book Ingestion and Download Event Routing

**Deliverables**

- Update `skip_file` and download completion handling so PDF and EPUB files are not skipped.
- Route completed files by media kind in `MetaDataManager`.
- Keep video files on the existing `generate_video_metadatas` path.
- Send PDF and EPUB files through the new book ingestion path.
- Give `MetaDataManager` access to the `BOOK_DIR` file store, separate from the video `Storer`.
- Save book records after metadata extraction, thumbnail assignment, and file movement.

**Files**

- Modify: `src/domain/algorithm/video_utils.rs`
- Modify: `src/domain/services/book_metadata.rs`
- Modify: `src/domain/services/mod.rs`
- Modify: `src/services/video_information.rs`
- Modify: `src/entrypoints/tvserver.rs`

**Acceptance Criteria**

- Completed video files continue through existing video processing.
- Completed PDF and EPUB files go through book processing.
- Unsupported completed files are logged and skipped.
- Book files are moved into `BOOK_DIR`, not `MOVIE_DIR`.
- Book processing assigns a default thumbnail when extraction fails.
- Book processing saves a record even when metadata is weak, unless the file is zero bytes or cannot be moved/saved.
- Duplicate path processing prevention still works.
- Concurrency limits for metadata processing still apply.

**Verification**

- Unit tests cover media-kind routing.
- Unit tests cover `skip_file` accepting PDF/EPUB and rejecting unrelated files.
- Book ingestion tests cover successful EPUB and PDF ingestion.
- Regression tests cover existing video event routing.

## Task 9: Book Idle Scanner

**Deliverables**

- Add a book scanner service for `BOOK_DIR`.
- Queue new PDF/EPUB files for metadata processing.
- Retry books in `NeedMetadata` or `MetadataError` state.
- Delete orphaned book rows when files disappear.
- Run book scanning alongside the existing video scan from the monitor.

**Files**

- Create: `src/domain/services/book_check.rs`
- Modify: `src/domain/services/mod.rs`
- Modify: `src/domain/traits.rs`
- Modify: `src/services/monitor.rs`
- Modify: `src/entrypoints/context.rs`
- Modify: `src/entrypoints/tvserver.rs`
- Modify: `tests/common/context.rs`

**Acceptance Criteria**

- Scanner walks `BOOK_DIR` recursively.
- Scanner only queues PDF and EPUB files.
- Scanner does not queue generated thumbnails.
- Scanner uses collections relative to `BOOK_DIR`.
- Scanner removes database rows for missing files.
- Scanner does not affect video scan behavior.
- Monitor logs book scan failures without stopping task-state broadcasting.

**Verification**

- Scanner tests cover new book detection.
- Scanner tests cover unsupported-file skip behavior.
- Scanner tests cover retry-state behavior.
- Scanner tests cover orphan removal.
- Existing monitor-related tests continue to pass.

## Task 10: REST API and Static Book Serving

**Deliverables**

- Add dedicated REST routes for book collections, individual book lookup, book deletion, book download serving, and book thumbnail serving.
- Serve stored books from `BOOK_DIR`.
- Serve generated thumbnails and `default-book.jpg` from the book thumbnail route.
- Keep `/api/media` unchanged.
- Update integration test server setup.

**Files**

- Modify: `src/entrypoints/api.rs`
- Modify: `src/entrypoints/webserver.rs`
- Modify: `tests/common/server.rs`
- Create: `tests/book_api_test.rs`
- Add fixtures under `tests/fixtures/book_dir`

**Acceptance Criteria**

- `GET /api/books` lists root book collections.
- `GET /api/books/{collection}` lists nested book collections and books.
- `GET /api/book/{checksum}` returns one book record.
- `DELETE /api/book/{checksum}` deletes the book file, generated thumbnail when applicable, and database row.
- Book download route serves stored PDF/EPUB files from `BOOK_DIR`.
- Book thumbnail route serves generated thumbnails and the default image.
- Existing `/api/media` routes and payloads are unchanged.
- Existing stream and video thumbnail routes are unchanged.

**Verification**

- Integration tests cover collection listing.
- Integration tests cover single book lookup.
- Integration tests cover delete behavior.
- Integration tests cover static book download serving.
- Integration tests cover default thumbnail serving.
- Existing media/search/download API tests continue to pass.

## Task 11: Tauri Commands

**Deliverables**

- Add Tauri commands that mirror the REST book read/delete operations.
- Keep commands gated to non-webserver builds, consistent with existing Tauri API structure.
- Ensure Android/Tauri clients can list root books, list a collection, retrieve one book, and delete one book.

**Files**

- Modify: `src/entrypoints/tauri_api.rs`

**Acceptance Criteria**

- `list_root_books` returns root book collection details.
- `list_books` returns collection details for a requested collection.
- `get_book` parses checksum strings and returns one book.
- `delete_book` parses checksum strings and deletes through `BookStore`.
- Invalid checksum inputs return useful errors.
- Webserver builds are unaffected.
- Default Tauri build compiles.

**Verification**

- Default build check passes.
- Webserver build check passes.
- Command-level tests are added where the current test harness supports them.

## Task 12: Documentation and Runtime Configuration

**Deliverables**

- Document `BOOK_DIR` and `BOOK_THUMBNAIL_DIR`.
- Document that implementation branches must be based on `spec/ebook-support`.
- Document optional PDF thumbnail rendering behavior and Android constraints.
- Update environment samples if they list required runtime variables.

**Files**

- Modify: `README.md`
- Modify: `env.sample`
- Modify: `docs/superpowers/specs/2026-07-13-book-library-design.md` only if implementation decisions materially change the approved spec.

**Acceptance Criteria**

- Required book configuration is visible to operators.
- Default thumbnail behavior is documented.
- Optional Pdfium behavior is documented as optional.
- Documentation does not imply desktop-only PDF tools are required.
- Spec branch discipline remains explicit.

**Verification**

- Documentation review confirms no stale references to video-only library behavior where book behavior is now relevant.

## Task 13: Final Verification

**Deliverables**

- Confirm all focused book tests pass.
- Confirm affected video regression tests pass.
- Confirm full webserver test suite passes.
- Confirm default Tauri build check passes.
- Confirm no desktop-only external tools are required for normal tests.
- Confirm all implementation branches remain based on `spec/ebook-support`.

**Acceptance Criteria**

- Book unit tests pass.
- Book integration tests pass.
- Existing video tests pass.
- Existing search/download tests pass.
- Full webserver test suite passes.
- Default Tauri build check passes.
- Any optional Pdfium-specific tests are clearly feature-gated.
- Worktree is clean after final commits.

## Spec Coverage Map

- PDF and EPUB support: Tasks 1, 6, 7, and 8.
- Separate `BOOK_DIR`: Tasks 1, 5, 8, 9, and 10.
- New `books` table and repository: Task 4.
- Metadata extraction: Tasks 6 and 7.
- Default thumbnail fallback: Tasks 2, 6, 7, 8, and 10.
- REST API: Task 10.
- Tauri commands: Task 11.
- Android dependency policy: Tasks 1, 7, and 12.
- Existing video behavior preserved: Tasks 1, 3, 4, 8, 9, 10, and 13.
- Spec branch discipline: this plan and `docs/superpowers/specs/2026-07-13-book-library-design.md`.

