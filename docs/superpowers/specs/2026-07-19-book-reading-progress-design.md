# Book Reading Progress Design

## Summary

Add one server-side reading-progress record per book so web, Tauri desktop, and Android clients can resume from the most recently saved position. Progress is format-neutral, stored independently from book metadata, and exposed through matching REST and Tauri operations.

## Scope

The feature includes:

- persistent progress keyed by book checksum;
- EPUB CFI and PDF page locators;
- an optional normalized progression value;
- list, get, save, and delete operations over REST and Tauri;
- automatic cleanup when a book is deleted;
- OpenAPI schemas and operations;
- model, repository, API, Tauri, routing, lifecycle, and contract tests.

The feature does not include per-user progress, history, bookmarks, highlights, annotations, or live synchronization between open readers.

## API Models

Clients save only fields they own:

```json
{
  "locator": {
    "type": "epub-cfi",
    "value": "epubcfi(/6/4!/4/2/8)"
  },
  "progression": 0.42
}
```

The server returns the complete record:

```json
{
  "checksum": "9223372036854775807",
  "locator": {
    "type": "epub-cfi",
    "value": "epubcfi(/6/4!/4/2/8)"
  },
  "progression": 0.42,
  "updatedOn": "2026-07-19T12:00:00"
}
```

The internal checksum remains an `i64`, but API serialization emits it as a decimal string. `updatedOn` is always assigned by the server. The save request cannot set either field.

The locator type is a closed enum with two initial values:

- `epub-cfi`
- `pdf-page`

The locator value is opaque to the backend and must contain at least one non-whitespace character. A PDF page is represented as a 1-based page number encoded as a string, but the backend does not interpret or normalize that string. `progression` is optional and, when present, must be finite and within the inclusive range `0..=1`.

## Architecture

Add a focused `book_progress` domain model and a small stateless `BookProgressService`. The service is shared by REST and Tauri and owns:

- checksum parsing;
- input validation;
- book-existence checks;
- repository orchestration;
- stable domain errors.

This boundary prevents REST and Tauri validation or error behavior from drifting. It uses the repository already available through the application context and does not require new runtime state.

Extend the repository trait and SQLite implementation with list, get, upsert, and delete progress operations. Repository methods own SQL concerns and return persisted records with server-generated timestamps. They do not expose transport-specific status codes or response wrappers.

## Persistence

Add a migration for a separate `book_progress` table:

- `checksum INTEGER PRIMARY KEY`, referencing `books(checksum)` with `ON DELETE CASCADE`;
- `locator_type TEXT NOT NULL`;
- `locator_value TEXT NOT NULL`;
- `progression REAL`;
- `updated_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP`.

SQL constraints enforce the two supported locator types, a nonblank locator value, and progression within `0..=1`. Application validation provides useful client errors before a database constraint is reached.

The SQLite connection must explicitly enforce foreign keys. Tests must verify that both ordinary repository book deletion and the conditional deletion used by `BookStore` cascade to progress.

Saving uses a single upsert keyed by checksum. A later save replaces the locator and progression and sets `updated_on = CURRENT_TIMESTAMP`. Listing orders records by checksum for deterministic output.

Re-ingestion with the same checksum preserves progress. When the existing book upsert encounters the same path with a different checksum, it must delete the old book row before inserting the replacement. This allows the old progress to cascade and prevents progress from transferring to different content. Updating a book checksum through `ON UPDATE CASCADE` is explicitly forbidden.

## Operations

### REST

- `GET /api/book-progress`
  - Returns `200 OK` with all saved records as a JSON array.
- `GET /api/book/{checksum}/progress`
  - Returns `200 OK` with the saved record.
  - Returns `204 No Content` when the book exists but has no progress.
- `PUT /api/book/{checksum}/progress`
  - Accepts `SaveBookProgress`, creates or replaces the record, and returns `200 OK` with the persisted `BookProgress`.
- `DELETE /api/book/{checksum}/progress`
  - Removes any saved record and returns `204 No Content`.
  - The operation is idempotent for an existing book with no saved progress.

All per-book operations validate that the referenced book exists. A valid but unknown checksum returns `404 Not Found`; a malformed or overflowing checksum returns `400 Bad Request`. Invalid locator type, blank locator value, non-finite progression, or progression outside `0..=1` returns `400 Bad Request`. REST errors use the existing JSON error response shape. Unexpected repository failures are logged and returned as sanitized `500 Internal Server Error` responses.

When the book runtime is unavailable, every progress route returns the existing stable `503 Service Unavailable` book-library response. Runtime availability is checked before request-body extraction so this behavior is consistent for all progress operations.

### Tauri

Add and register:

- `list_book_progress() -> Result<Vec<BookProgress>, String>`
- `get_book_progress(checksum: String) -> Result<Option<BookProgress>, String>`
- `save_book_progress(checksum: String, progress: SaveBookProgress) -> Result<BookProgress, String>`
- `delete_book_progress(checksum: String) -> Result<(), String>`

The commands delegate to testable core functions backed by the shared service. Their successful results match REST semantics, and validation or not-found failures use the same stable domain messages.

## Data Flow

1. A client submits a checksum and, for saves, a locator with optional progression.
2. REST or Tauri verifies that the book runtime is available.
3. `BookProgressService` parses the checksum and validates the request.
4. For per-book operations, the service verifies that the book exists.
5. The repository lists, retrieves, upserts, or deletes the progress row.
6. The transport maps the typed result or domain error to its response shape.

The list operation reads all progress rows directly and performs no per-book lookups, avoiding an N+1 query pattern.

## Error Model

Use a dedicated error type with stable variants for:

- invalid checksum;
- book not found;
- invalid locator type;
- blank locator value;
- invalid progression;
- repository failure.

Messages derived from these variants are shared by REST and Tauri. The underlying database error is retained for logging but is not exposed in REST responses.

## OpenAPI

Update `docs/api/openapi.yaml` with both routes and reusable schemas for:

- `BookProgress`;
- `SaveBookProgress`;
- `BookLocator` and its EPUB CFI and PDF page variants;
- the list response.

The contract marks checksum as a signed-decimal string, `updatedOn` as a required server-authored date-time, locator type as exactly one supported variant, locator value as non-empty, and progression as optional with minimum `0` and maximum `1`. PUT documents only the client-owned request fields. GET-without-progress and DELETE document empty `204` responses.

## Testing

Follow test-driven development for every behavior.

Model and validation tests cover exact EPUB/PDF serialization, checksum string serialization, optional progression, inclusive bounds, non-finite values, unsupported locator types, blank values, and preservation of opaque locator values.

Repository tests cover migration structure, foreign-key enforcement, EPUB and PDF round trips, deterministic listing, last-write-wins replacement, timestamp assignment, unknown books, idempotent reset, same-checksum preservation, different-checksum replacement cleanup, and cascades through both book deletion methods.

REST integration tests cover list/get/save/delete, `204` behavior, full-width `i64` checksum strings, replacement, malformed and unknown checksums, invalid payloads, runtime unavailability, sanitized repository failures, and progress cleanup after full book deletion.

Tauri tests exercise the core functions and command registration with the same success, validation, not-found, replacement, reset, and lifecycle cases.

OpenAPI tests require the new paths and schemas, validate representative EPUB and PDF payloads, reject numeric checksums and invalid locators or progression, and check request-versus-response field ownership. The project OpenAPI parser and semantic validation must continue to pass.

## Worktree Safety

The existing generated frontend changes under `client/newapp` are unrelated user work. Implementation must not modify, stage, or commit those files.
