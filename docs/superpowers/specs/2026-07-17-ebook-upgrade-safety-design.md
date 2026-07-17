# Ebook Upgrade Safety Design

## Context

Review of `spec/ebook-support` found upgrade-safety and availability failures
that remain after the first merge-blocker pass. The existing video scanner can
relocate books found under `MOVIE_DIR`, untrusted PDF parsing is not resource
bounded, one malformed filesystem entry can stop the book scanner, unavailable
book storage can prevent the video server from starting, and some file-identity
checks do not compile on stable Windows.

This change addresses those five blockers, the Docker book-volume mismatch, and
the unsafe default implementation of `FileStore::list_folder_no_follow`. It does
not change the durable checksum identity or broaden the work to the remaining
review suggestions.

## Optional Book Runtime

`Context` owns a `BookRuntime` enum instead of independently exposing book
components that may be only partially initialized.

- `BookRuntime::Available` owns the initialized `BookStore`, book `FileStorer`,
  `BookChecker`, path-lease coordinator, and retained roots needed for static
  book and thumbnail serving.
- `BookRuntime::Unavailable` records a sanitized initialization failure.

Startup attempts to create and open the configured book and thumbnail
directories exactly once. A failure constructs the unavailable state instead of
failing `Context` or HTTP server construction. Video storage, monitoring,
metadata processing, APIs, streaming, and static files remain available.

Book HTTP routes remain registered in both states so the public route shape and
OpenAPI contract do not drift. When books are unavailable, list, get, delete,
download, and thumbnail routes return `503 Service Unavailable` with a stable
generic response. Tauri book commands return the equivalent stable error.

The monitor uses a no-op book checker in the unavailable state. The metadata
manager receives optional book-ingestion state; a book event received while the
subsystem is unavailable is rejected and logged without affecting video
workers.

## Separate Video Scan Admission From Download Routing

The periodic `MOVIE_DIR` scan uses a video-specific admission rule. It queues
known video extensions and preserves the legacy behavior that treats
extensionless files as possible videos. It never queues EPUB or PDF files, so an
upgrade cannot move existing documents out of a movie library.

Completed download events continue using the shared media classifier. Known
book downloads therefore still reach book ingestion, while known video
downloads continue through video metadata processing.

## Resilient No-Follow Scanning

`FileSystemStore::list_folder_no_follow` retains strict capability-based root
opening and component traversal. Failure to open the requested directory still
fails the scan. Once the directory is open, an individual entry that vanishes,
cannot be inspected, or cannot be represented as UTF-8 is logged and skipped;
valid siblings remain visible to the scanner.

`FileStore::list_folder_no_follow` no longer delegates by default to the
symlink-following `list_folder`. Its default implementation returns an
unsupported-operation error, requiring every adapter that claims strict
listing support to implement it explicitly.

Orphan reconciliation isolates failures per recorded book. A confirmed missing
regular file allows the matching database row to be deleted. A directory,
symlink, permission failure, or other suspicious state preserves the row, logs
the error, and allows reconciliation to continue. Deletion remains fail-closed.

## Safe PDF Fallback

PDF ingestion does not invoke lopdf or Pdfium. It derives the title from the
original source filename, records no authors or page count, and assigns the
default book cover. PDF files remain ingestible, downloadable, and servable.
EPUB metadata and cover parsing are unchanged.

This intentionally trades rich PDF metadata for a merge-safe boundary around
untrusted torrent input. A future rich parser must run in a separately
resource-constrained process; a compressed-file size cap alone is not accepted
as decompression-bomb containment.

The release profile uses panic unwinding so worker-level panic containment can
operate in production. Unwinding is defense in depth and is not treated as the
PDF resource-control mechanism.

## Portable File Identity

Private-snapshot ownership checks move behind the `FileStore` boundary. The
filesystem adapter verifies identity through its retained capability and
`cap_std` metadata, which has a stable Windows implementation. Production code
does not call `cap_fs_ext::MetadataExt` on `std::fs::Metadata`.

Other production identity comparisons, including thumbnail publication, use
opened handles with capability metadata or stable platform-specific APIs.
Symlink rejection and creation-time identity binding remain unchanged.

## Container Configuration

The general default remains a lowercase `books` directory beside `MOVIE_DIR`.
Docker Compose explicitly sets `BOOK_DIR=/Books`, matching its existing
`${HOME}/Books:/Books` volume. Container books therefore persist on the mounted
host directory rather than an ephemeral lowercase `/books` path.

## Testing

Each behavior is implemented with a focused test-first red-green cycle:

- the movie scanner ignores EPUB/PDF files without moving them;
- extensionless legacy video files are still queued;
- completed book downloads still route to book ingestion;
- invalid PDF bytes ingest with filename metadata and the default cover without
  invoking a PDF parser;
- non-UTF-8 and disappearing entries do not hide valid siblings, while failure
  to open the requested directory remains fatal to that scan;
- a directory or symlink at one recorded book path preserves that row while
  reconciliation continues for other books;
- unavailable book storage returns `503` for every book HTTP surface while a
  representative video route remains usable;
- Tauri commands report the stable unavailable-books error;
- capability-owned snapshot identity compiles for supported configurations,
  including a stable Windows check when the target is available;
- the default no-follow trait method fails closed; and
- Compose selects `/Books` explicitly.

Final verification runs the full library suite, webserver integration tests,
both feature build configurations, formatting where supported by the repository
toolchain, and `git diff --check`.

## Scope

This pass does not migrate the `i64` checksum identity, change CORS or LAN
authentication, add a PDF parser subprocess, or implement unrelated major and
suggestion items from the review. Those remain separate design decisions.
