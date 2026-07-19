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
  "updatedOn": "2026-07-19T12:00:00Z"
}
```

The internal checksum remains an `i64`, but API serialization emits it as a decimal string. Integration tests use `9223372036854775807` (`i64::MAX`) to exercise a value above JavaScript's safe-integer range while remaining valid for the backend. The frontend fixture value `18446744073709551615` is outside signed `i64` and is not a valid backend checksum.

`updatedOn` is always assigned by the server and serialized as RFC 3339 UTC with a `Z` suffix. The save request cannot set either `checksum` or `updatedOn`.

The transport input and validated domain model are separate. REST and Tauri deserialize a raw `SaveBookProgressRequest` whose locator type is a `String`. `BookProgressService` converts that string into the closed `BookLocatorType` domain enum, so unsupported types produce the same stable domain validation error instead of an Axum or Tauri deserialization rejection.

The validated locator type is a closed enum with two initial values:

- `epub-cfi`
- `pdf-page`

The locator value is opaque to the backend and must contain at least one non-whitespace character. A PDF page is represented by frontend clients as a positive, 1-based page number encoded as a string, but issue #53 deliberately makes the value opaque. The backend therefore does not parse, normalize, or restrict PDF locator values beyond the nonblank rule. `progression` is optional and, when present, must be finite and within the inclusive range `0..=1`.

## Architecture

Add focused raw-request and validated `book_progress` models plus a small stateless `BookProgressService`. The service is shared by REST and Tauri and owns:

- checksum parsing;
- input validation;
- conversion from the raw transport DTO to the closed domain enum;
- interpretation of atomic repository outcomes as book-not-found, no-progress, or saved-progress results;
- repository orchestration;
- stable domain errors.

This boundary prevents REST and Tauri validation or error behavior from drifting. It uses the repository already available through the application context and does not require new runtime state.

Extend the repository trait and SQLite implementation with list, get, upsert, and delete progress operations. Repository methods own SQL concerns, combine per-book existence with progress operations atomically, and return typed outcomes with persisted records and server-generated timestamps. They do not expose transport-specific status codes or response wrappers.

## Persistence

Add a migration for a separate `book_progress` table:

- `checksum INTEGER PRIMARY KEY`, referencing `books(checksum)` with `ON DELETE CASCADE`;
- `locator_type TEXT NOT NULL`;
- `locator_value TEXT NOT NULL`;
- `progression REAL`;
- `updated_on TEXT NOT NULL`, populated with an RFC 3339 UTC value such as `2026-07-19T12:00:00.000Z`.

SQL constraints enforce the two supported locator types, a nonblank locator value, and progression within `0..=1`. Application validation provides useful client errors before a database constraint is reached.

Foreign keys must be enabled for every pooled SQLite connection. `SqlRepository::new` will build `SqliteConnectOptions` with `foreign_keys(true)` and create the pool with `SqlitePoolOptions::connect_with`, or use an equivalent per-connection hook. Running `PRAGMA foreign_keys = ON` once after pool creation is insufficient and is not an acceptable implementation. A file-backed, multi-connection repository test must acquire multiple pooled connections and verify enforcement on each one. Tests must also verify that both ordinary repository book deletion and the conditional deletion used by `BookStore` cascade to progress.

Saving uses one conditional upsert statement (or one transaction with equivalent locking) that selects the referenced checksum from `books`, inserts or replaces progress, assigns the UTC timestamp, and returns the persisted row. If the book does not exist, no progress row is inserted and the repository returns a typed missing-book outcome that the service maps to `BookNotFound`. A service-level existence query followed by a separate upsert is forbidden because book deletion could race between those operations. Foreign-key violations are also mapped to `BookNotFound` rather than exposed as repository failures.

Per-book reads use a single query that distinguishes an unknown book from an existing book with no progress. Deletes similarly return an atomic typed outcome distinguishing an unknown book from an idempotent reset. A later save replaces the locator and progression and assigns a fresh RFC 3339 UTC `updated_on`. Listing orders records by checksum for deterministic output.

Re-ingestion with the same checksum preserves progress. When the existing book upsert encounters the same path with a different checksum, it must delete the old book row before inserting the replacement. This allows the old progress to cascade and prevents progress from transferring to different content. Updating a book checksum through `ON UPDATE CASCADE` is explicitly forbidden.

## Operations

### REST

- `GET /api/book-progress`
  - Returns `200 OK` with all saved records as a JSON array.
- `GET /api/book/{checksum}/progress`
  - Returns `200 OK` with the saved record.
  - Returns `204 No Content` when the book exists but has no progress.
- `PUT /api/book/{checksum}/progress`
  - Accepts raw `SaveBookProgressRequest`, converts it in the service, creates or replaces the record atomically with book existence, and returns `200 OK` with the persisted `BookProgress`.
- `DELETE /api/book/{checksum}/progress`
  - Removes any saved record and returns `204 No Content`.
  - The operation is idempotent for an existing book with no saved progress.

All per-book operations validate that the referenced book exists. A valid but unknown checksum returns `404 Not Found`; a malformed or overflowing checksum returns `400 Bad Request`. Invalid locator type, blank locator value, non-finite progression, or progression outside `0..=1` returns `400 Bad Request`. REST errors use the existing JSON error response shape. Unexpected repository failures are logged and returned as sanitized `500 Internal Server Error` responses.

When the book runtime is unavailable, every progress route returns the existing stable `503 Service Unavailable` book-library response. For PUT, Axum still has to run extractors before entering the handler body, so the payload parameter must be rejection-capturing, such as `Result<Json<SaveBookProgressRequest>, JsonRejection>`. The handler checks runtime availability before inspecting that captured result. This makes `503` take precedence even when JSON is malformed, type-invalid, or over the configured body limit. When the runtime is available, malformed or type-invalid JSON is mapped to the existing JSON error shape with `400`; payload-limit rejection is mapped to the same JSON shape with `413`.

### Tauri

Add and register:

- `list_book_progress() -> Result<Vec<BookProgress>, String>`
- `get_book_progress(checksum: String) -> Result<Option<BookProgress>, String>`
- `save_book_progress(checksum: String, progress: SaveBookProgressRequest) -> Result<BookProgress, String>`
- `delete_book_progress(checksum: String) -> Result<(), String>`

The commands delegate to testable core functions backed by the shared service. Their successful results match REST semantics, and validation or not-found failures use the same stable domain messages.

## Data Flow

1. A client submits a checksum and, for saves, a locator with optional progression.
2. REST or Tauri verifies that the book runtime is available.
3. `BookProgressService` parses the checksum and converts the raw request into validated domain values.
4. The repository lists progress or executes a per-book operation that atomically includes the book-existence decision.
5. The service maps the typed repository outcome to a progress value, no-progress result, or stable domain error.
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
- raw `SaveBookProgressRequest`;
- `BookLocator` and its EPUB CFI and PDF page variants;
- the list response.

The contract marks checksum as a signed-decimal string, `updatedOn` as a required server-authored RFC 3339 UTC string with `format: date-time`, locator type as exactly one supported variant, locator value as non-empty, and progression as optional with minimum `0` and maximum `1`. PUT documents only the client-owned request fields. The Rust request DTO keeps locator type as a raw string even though OpenAPI correctly advertises the two accepted values. GET-without-progress and DELETE document empty `204` responses.

## Implementation and Testing Workstreams

Follow test-driven development for every behavior. The detailed implementation plan must break the following workstreams into red-green-refactor tasks with exact files, commands, expected failures, and expected passing results.

### 1. Transport and Domain Models

Create the raw request DTO, validated locator enum/value, response model, shared error type, and service conversion. Tests cover exact EPUB/PDF serialization, conversion of unsupported raw locator strings into stable domain errors, checksum string serialization using `9223372036854775807`, optional progression, inclusive bounds, non-finite values, blank values, and preservation of opaque locator values including non-numeric PDF values.

### 2. Migration and Pooled Connection Enforcement

Add the constrained table and configure per-connection foreign-key enforcement before migrations run. A file-backed repository test with a pool capable of multiple connections verifies `PRAGMA foreign_keys = 1` on separately acquired connections and proves an orphan progress insert cannot succeed. Migration tests verify the foreign key, checks, columns, and RFC 3339 UTC timestamp storage.

### 3. Repository CRUD and Atomic Outcomes

Implement and test list, tri-state get, conditional upsert, and tri-state delete. Tests cover EPUB and PDF round trips, deterministic checksum ordering, last-write-wins replacement, server-authored UTC timestamps, unknown-book atomic save failure, idempotent reset for an existing book, and sanitized handling of unexpected database failures.

### 4. Book Lifecycle and Re-ingestion

Repository and service tests prove progress cascades through both `delete_book` and `delete_book_if_path_matches`; a path mismatch must preserve both book and progress. Re-ingestion tests prove same-checksum updates preserve progress and same-path/different-checksum replacement deletes old progress instead of transferring it to new content.

### 5. REST API

Register the list and per-book routes and implement response mapping. Integration tests cover list `200`; get `200` and no-progress `204`; save `200`; delete `204`; malformed/overflow checksum `400`; unknown book `404`; unsupported locator, blank value, and invalid progression `400`; payload-too-large `413`; unavailable runtime `503`; and sanitized repository `500`. A dedicated router test sends malformed and oversized PUT bodies while the runtime is unavailable and verifies JSON `503` takes precedence. Round-trip responses assert the checksum is the string `"9223372036854775807"`. Full book deletion must make the progress row disappear.

### 6. Tauri Parity

Add the four commands, their testable core functions, and command registration. Tests cover list, optional get, typed save result, unit delete result, replacement, reset, malformed and unknown checksums, raw unsupported locator conversion, progression validation, runtime unavailability, and stable error-message parity with REST domain errors. Compile-time command registration coverage must include all four command names.

### 7. OpenAPI Contract

OpenAPI tests require the new paths and schemas, validate representative EPUB and PDF request/response payloads, reject numeric and out-of-`i64` checksums, reject invalid locators or progression, check request-versus-response field ownership, and assert `updatedOn` is RFC 3339 UTC with `format: date-time`. Extend the local schema test helper or add direct assertions so `minimum`, `maximum`, and `minLength` are actually tested. Both the typed parser and project semantic validator must pass.

### 8. Regression Verification

After focused red-green cycles, run the complete default/Tauri and webserver configurations:

```bash
cargo test --lib
cargo test --all-targets
cargo test --no-default-features --features webserver --lib
cargo test --no-default-features --features webserver --test book_api_test
cargo test --no-default-features --features webserver --test book_router_test
cargo test --no-default-features --features webserver --test openapi_contract_test
cargo test --no-default-features --features webserver --all-targets
```

The final verification also runs `cargo fmt --check` and `git diff --check`. No completion claim may be made unless every command finishes successfully, or any environment-specific blocker is reported with its exact failing output.

## Worktree Safety

The existing generated frontend changes under `client/newapp` are unrelated user work. Implementation must not modify, stage, or commit those files.
