# Book Reading Progress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist one format-neutral reading-progress record per book and expose matching REST and Tauri list/get/save/delete operations.

**Architecture:** Add raw transport DTOs and validated domain types in a focused model module, then centralize checksum parsing, validation, and typed repository-outcome mapping in a stateless `BookProgressService`. Extend `Databaser` and `SqlRepository` with atomic progress operations; REST and Tauri remain thin transport adapters over the shared service.

**Tech Stack:** Rust 2021, Tokio, SQLx/SQLite migrations, Serde, Chrono, Axum 0.8, Tauri 2, OpenAPI 3.1, `oas3`, and `roas`.

## Global Constraints

- The existing generated frontend changes under `client/newapp` are unrelated user work; do not modify, stage, or commit them.
- The internal checksum is `i64`; every progress API response serializes it as a decimal string and backend boundary tests use `9223372036854775807` (`i64::MAX`).
- `SaveBookProgressRequest` contains only `locator` and optional `progression`; clients cannot set `checksum` or `updatedOn`.
- Transport locator type is a raw `String`; `BookProgressService` converts it to the closed `BookLocatorType::{EpubCfi, PdfPage}` domain enum with wire values `epub-cfi` and `pdf-page`.
- Locator values are opaque but must contain at least one non-whitespace character; do not parse or normalize PDF page values.
- Optional progression must be finite and within inclusive `0..=1`.
- `updatedOn` is server-authored RFC 3339 UTC and serializes with a `Z` suffix.
- Book existence and each per-book progress read/write/delete outcome must be decided atomically in the repository; never add a service-level existence query followed by another operation.
- Foreign keys must be enabled on every pooled SQLite connection before migrations run; one post-connect `PRAGMA foreign_keys = ON` is insufficient.
- Re-ingestion with the same checksum preserves progress; same-path/different-checksum replacement deletes the old row so progress cascades and never transfers through an updated checksum.
- REST PUT checks runtime availability before interpreting captured JSON extraction errors, so unavailable runtime returns `503` even for malformed, type-invalid, or oversized payloads.
- Unexpected repository details are logged but never exposed through REST responses.

---

### Task 1: Raw and Validated Progress Models

**Files:**
- Create: `src/domain/models/book_progress.rs`
- Modify: `src/domain/models/mod.rs`

**Interfaces:**
- Consumes: Serde and Chrono already present in `Cargo.toml`.
- Produces: `RawBookLocator { locator_type: String, value: String }`, `SaveBookProgressRequest { locator: RawBookLocator, progression: Option<f64> }`, `BookLocatorType`, `BookLocator`, and `BookProgress`.

- [ ] **Step 1: Write failing model contract tests**

Add unit tests in `book_progress.rs` asserting exact camel-case JSON, enum wire values, omitted `progression`, and string serialization of `i64::MAX`:

```rust
#[test]
fn progress_serializes_full_width_checksum_and_utc_timestamp() {
    let progress = BookProgress {
        checksum: i64::MAX,
        locator: BookLocator {
            locator_type: BookLocatorType::EpubCfi,
            value: "epubcfi(/6/4!/4/2/8)".into(),
        },
        progression: Some(0.42),
        updated_on: "2026-07-19T12:00:00.000Z".into(),
    };
    assert_eq!(serde_json::to_value(progress).unwrap(), serde_json::json!({
        "checksum": "9223372036854775807",
        "locator": { "type": "epub-cfi", "value": "epubcfi(/6/4!/4/2/8)" },
        "progression": 0.42,
        "updatedOn": "2026-07-19T12:00:00.000Z"
    }));
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --lib domain::models::book_progress::tests`

Expected: FAIL because `book_progress` and its types do not exist.

- [ ] **Step 3: Implement the model types and exports**

Use a dedicated checksum serializer and keep the request raw:

```rust
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveBookProgressRequest {
    pub locator: RawBookLocator,
    pub progression: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawBookLocator {
    #[serde(rename = "type")]
    pub locator_type: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookLocatorType { EpubCfi, PdfPage }

#[derive(Clone, Debug, PartialEq)]
pub struct BookLocator {
    pub locator_type: BookLocatorType,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BookProgress {
    pub checksum: i64,
    pub locator: BookLocator,
    pub progression: Option<f64>,
    pub updated_on: String,
}
```

Implement/derive serialization so JSON fields are `locator.type`, `locator.value`, `progression`, `checksum`, and `updatedOn`; `checksum` uses `serializer.collect_str(&value)` and `BookLocatorType` emits only the two wire values. Re-export every type from `src/domain/models/mod.rs`.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test --lib domain::models::book_progress::tests`

Expected: all model tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/domain/models/book_progress.rs src/domain/models/mod.rs
git commit -m "feat: add book progress models"
```

### Task 2: Shared Validation Service and Stable Errors

**Files:**
- Create: `src/domain/services/book_progress.rs`
- Modify: `src/domain/services/mod.rs`
- Modify: `src/domain/traits.rs`

**Interfaces:**
- Consumes: Task 1 model types and `Repository`.
- Produces: `BookProgressError`, `BookProgressService::new(Repository)`, `list`, `get(&str)`, `save(&str, SaveBookProgressRequest)`, and `delete(&str)`; repository outcome enums named below.

- [ ] **Step 1: Write failing validation tests**

Test exact errors for malformed/overflow checksum, unsupported locator type, blank locator, `NaN`, infinities, and values outside `0..=1`; test success for `0`, `1`, omitted progression, EPUB CFI, and opaque nonnumeric PDF values.

```rust
assert_eq!(service.validate_checksum("9223372036854775808").unwrap_err().to_string(), "invalid book checksum");
assert_eq!(service.validate_request(raw("future", "x", None)).unwrap_err().to_string(), "invalid book locator type");
assert_eq!(service.validate_request(raw("pdf-page", "chapter-a", Some(1.0))).unwrap().locator.value, "chapter-a");
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --lib domain::services::book_progress::tests`

Expected: FAIL because the service and error type do not exist.

- [ ] **Step 3: Add repository outcome interfaces**

Extend `Databaser` with these exact methods and define the enums beside the trait:

```rust
pub enum GetBookProgressOutcome { BookNotFound, NoProgress, Progress(BookProgress) }
pub enum SaveBookProgressOutcome { BookNotFound, Saved(BookProgress) }
pub enum DeleteBookProgressOutcome { BookNotFound, Deleted }

async fn list_book_progress(&self) -> Result<Vec<BookProgress>, sqlx::Error>;
async fn get_book_progress(&self, checksum: i64) -> Result<GetBookProgressOutcome, sqlx::Error>;
async fn save_book_progress(&self, checksum: i64, progress: &BookLocator, progression: Option<f64>) -> Result<SaveBookProgressOutcome, sqlx::Error>;
async fn delete_book_progress(&self, checksum: i64) -> Result<DeleteBookProgressOutcome, sqlx::Error>;
```

Update the two test-only `Databaser` wrappers in `src/services/book_store.rs` and `src/domain/services/book_metadata.rs` to delegate all four methods to `inner` so the trait remains object-safe and compilation remains complete.

- [ ] **Step 4: Implement validation and orchestration**

Use stable variants/messages:

```rust
#[derive(Debug, thiserror::Error)]
pub enum BookProgressError {
    #[error("invalid book checksum")] InvalidChecksum,
    #[error("book not found")] BookNotFound,
    #[error("invalid book locator type")] InvalidLocatorType,
    #[error("book locator value must not be blank")] BlankLocatorValue,
    #[error("book progression must be finite and between 0 and 1")] InvalidProgression,
    #[error("book progress repository failure")] Repository(#[source] sqlx::Error),
}
```

Map typed repository outcomes to `Option<BookProgress>`, `BookProgress`, or `()` without any preflight existence query. Add `source()`/pattern access needed by REST logging while keeping SQL text out of `Display`.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test --lib domain::services::book_progress::tests`

Expected: all validation/service tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/domain/services/book_progress.rs src/domain/services/mod.rs src/domain/traits.rs src/services/book_store.rs src/domain/services/book_metadata.rs
git commit -m "feat: add book progress service"
```

### Task 3: Migration and Per-Connection Foreign Keys

**Files:**
- Create: `migrations/20260719000001_book_progress.sql`
- Modify: `src/adaptors/repository.rs`

**Interfaces:**
- Consumes: existing `books(checksum)` table.
- Produces: constrained `book_progress` table and a pool whose every connection enforces foreign keys before migrations.

- [ ] **Step 1: Write failing migration and pooled-connection tests**

In `repository.rs`, add tests that inspect `PRAGMA table_info(book_progress)`, `PRAGMA foreign_key_list(book_progress)`, the table SQL from `sqlite_master`, and acquire at least two file-backed pooled connections to assert `PRAGMA foreign_keys = 1` on each. Directly inserting checksum `404` into `book_progress` must fail with a foreign-key violation.

- [ ] **Step 2: Verify RED**

Run: `cargo test --lib adaptors::repository::tests::book_progress_migration`

Expected: FAIL because the table is absent or foreign keys are disabled.

- [ ] **Step 3: Add the constrained migration**

```sql
CREATE TABLE book_progress (
    checksum INTEGER PRIMARY KEY NOT NULL REFERENCES books(checksum) ON DELETE CASCADE,
    locator_type TEXT NOT NULL CHECK (locator_type IN ('epub-cfi', 'pdf-page')),
    locator_value TEXT NOT NULL CHECK (length(trim(locator_value)) > 0),
    progression REAL CHECK (progression IS NULL OR (progression >= 0.0 AND progression <= 1.0)),
    updated_on TEXT NOT NULL
);
```

- [ ] **Step 4: Configure every pooled connection before migration**

Replace `SqlitePool::connect` with parsed `SqliteConnectOptions` and `SqlitePoolOptions::connect_with`:

```rust
let options = url.parse::<SqliteConnectOptions>()?.create_if_missing(url != MEMORY_DB_URL).foreign_keys(true);
let pool = SqlitePoolOptions::new().connect_with(options).await?;
SqlRepository::do_migrations(&pool).await?;
```

Preserve existing database-creation behavior or remove it only when `create_if_missing` provides identical file-backed semantics.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test --lib adaptors::repository::tests::book_progress_migration`

Expected: migration, per-connection PRAGMA, and orphan-rejection tests PASS.

- [ ] **Step 6: Commit**

```bash
git add migrations/20260719000001_book_progress.sql src/adaptors/repository.rs
git commit -m "feat: persist book reading progress"
```

### Task 4: Atomic Repository CRUD and Book Lifecycle

**Files:**
- Modify: `src/adaptors/repository.rs`
- Modify: `src/services/book_store.rs`

**Interfaces:**
- Consumes: Task 2 repository outcome signatures and Task 3 table.
- Produces: deterministic list, atomic tri-state get/delete, conditional upsert, and correct cascade behavior through both deletion paths and re-ingestion.

- [ ] **Step 1: Write failing CRUD tests**

Add repository tests for EPUB/PDF round trips, checksum ordering, last-write-wins, changed server timestamp, `i64::MAX`, unknown-book save, existing/no-progress get and delete, unknown-book delete, and opaque PDF locators. Assert `DateTime::parse_from_rfc3339` succeeds and the stored/serialized suffix is `Z`.

- [ ] **Step 2: Verify RED**

Run: `cargo test --lib adaptors::repository::tests::book_progress`

Expected: FAIL because progress repository methods are not implemented.

- [ ] **Step 3: Implement atomic SQL operations**

Use `ORDER BY checksum` for list. Implement get with one `LEFT JOIN` from `books` to `book_progress`. Implement save as one conditional statement using `INSERT ... SELECT ... FROM books WHERE checksum = ? ON CONFLICT(checksum) DO UPDATE ... RETURNING ...`; if no row returns, emit `SaveBookProgressOutcome::BookNotFound`. Generate `updated_on` in SQL using `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`. Implement delete in one transaction that first conditionally deletes and then distinguishes existing book/no-progress from missing book without a race.

- [ ] **Step 4: Verify CRUD GREEN**

Run: `cargo test --lib adaptors::repository::tests::book_progress`

Expected: all focused CRUD tests PASS.

- [ ] **Step 5: Write failing lifecycle tests**

Add tests proving:

```rust
// delete_book and matching delete_book_if_path_matches remove progress;
// a path mismatch preserves both rows;
// save_book with the same checksum preserves progress;
// save_book with the same path and a new checksum deletes old progress and creates no progress for the new checksum.
```

- [ ] **Step 6: Verify lifecycle RED**

Run: `cargo test --lib adaptors::repository::tests::book_progress_cascades`

Expected: the same-path/different-checksum case FAILS because current `ON CONFLICT(collection, file_name)` updates the checksum.

- [ ] **Step 7: Fix same-path/different-checksum replacement**

Inside the existing `save_book` transaction, when `path_row == Some(old_checksum)` and `old_checksum != details.checksum`, explicitly `DELETE FROM books WHERE checksum = ?` before the insert. Remove primary-key mutation from the path-conflict branch so the subsequent insert creates the new identity; preserve same-checksum update behavior and existing event semantics.

- [ ] **Step 8: Verify lifecycle GREEN**

Run: `cargo test --lib adaptors::repository::tests::book_progress_cascades`

Expected: all cascade and re-ingestion tests PASS.

- [ ] **Step 9: Commit**

```bash
git add src/adaptors/repository.rs src/services/book_store.rs
git commit -m "feat: add atomic book progress repository"
```

### Task 5: REST Progress Operations

**Files:**
- Modify: `src/entrypoints/api.rs`
- Modify: `tests/book_api_test.rs`
- Modify: `tests/book_router_test.rs`

**Interfaces:**
- Consumes: `BookProgressService` and Task 1 request/response types.
- Produces: `GET /api/book-progress` and GET/PUT/DELETE `/api/book/{checksum}/progress`.

- [ ] **Step 1: Write failing REST integration tests**

Cover list `200`, get `200`/`204`, save `200`, delete/reset `204`, malformed and overflow checksum `400`, unknown book `404`, invalid locator/progression `400`, oversized payload `413`, unavailable runtime `503`, sanitized repository `500`, string `i64::MAX`, and cascade after full book deletion. Assert the existing `Response::error` JSON shape for failures.

- [ ] **Step 2: Verify RED**

Run: `cargo test --no-default-features --features webserver --test book_api_test book_progress`

Expected: FAIL with missing routes (`404`) or missing handler symbols.

- [ ] **Step 3: Register routes and thin handlers**

Register:

```rust
.route("/api/book-progress", get(list_book_progress))
.route("/api/book/{checksum}/progress", get(get_book_progress).put(save_book_progress).delete(delete_book_progress))
```

Create a `BookProgressService` from `state.get_repository()` only after `get_available_book_runtime()` succeeds. Map `Invalid*` to `400`, `BookNotFound` to `404`, and repository failure to logged/sanitized `500`. Return JSON for list/get/save, empty `204` for no progress and successful delete.

- [ ] **Step 4: Capture PUT extraction rejection after runtime check**

Use this handler shape:

```rust
async fn save_book_progress(
    State(state): State<SharedState>,
    checksum: Result<Path<String>, PathRejection>,
    payload: Result<Json<SaveBookProgressRequest>, JsonRejection>,
) -> AxumResponse
```

Check runtime first; then checksum; then map JSON syntax/data rejection to JSON `400` and `BytesRejection::LengthLimitError`/payload-too-large rejection to JSON `413`.

- [ ] **Step 5: Verify REST GREEN**

Run: `cargo test --no-default-features --features webserver --test book_api_test book_progress`

Expected: all focused REST tests PASS.

- [ ] **Step 6: Add precedence router tests**

In `book_router_test.rs`, send malformed, type-invalid, and body-limit-exceeding PUT bodies against an unavailable `BookRuntime`; each must return JSON `503` containing exactly `book library unavailable`.

- [ ] **Step 7: Verify precedence GREEN**

Run: `cargo test --no-default-features --features webserver --test book_router_test book_progress`

Expected: all progress router precedence tests PASS.

- [ ] **Step 8: Commit**

```bash
git add src/entrypoints/api.rs tests/book_api_test.rs tests/book_router_test.rs
git commit -m "feat: expose book progress REST API"
```

### Task 6: Tauri Command Parity

**Files:**
- Modify: `src/entrypoints/tauri_api.rs`

**Interfaces:**
- Consumes: `BookProgressService`, `BookProgress`, and `SaveBookProgressRequest`.
- Produces: four public Tauri commands and testable core functions with the signatures from the design.

- [ ] **Step 1: Write failing command-core tests**

Test list, optional get, typed save, unit delete, replacement/reset, malformed/unknown checksum, unsupported raw type, progression validation, unavailable runtime, stable domain messages, and `i64::MAX` serialization.

- [ ] **Step 2: Verify RED**

Run: `cargo test --lib entrypoints::tauri_api::tests::book_progress`

Expected: FAIL because progress command cores do not exist.

- [ ] **Step 3: Implement core functions and commands**

Add core functions taking `&BookProgressService` and commands:

```rust
#[tauri::command]
pub async fn list_book_progress(state: tauri::State<'_, SharedState>) -> Result<Vec<BookProgress>, String>;
#[tauri::command]
pub async fn get_book_progress(state: tauri::State<'_, SharedState>, checksum: String) -> Result<Option<BookProgress>, String>;
#[tauri::command]
pub async fn save_book_progress(state: tauri::State<'_, SharedState>, checksum: String, progress: SaveBookProgressRequest) -> Result<BookProgress, String>;
#[tauri::command]
pub async fn delete_book_progress(state: tauri::State<'_, SharedState>, checksum: String) -> Result<(), String>;
```

Each command calls `require_available_books` before constructing the service and maps `BookProgressError::to_string()` directly.

- [ ] **Step 4: Register all four commands**

Add `list_book_progress`, `get_book_progress`, `save_book_progress`, and `delete_book_progress` to `tauri::generate_handler!`. Add a source-level registration test that reads `tauri_api.rs` and asserts every exact command name appears inside the handler list, providing compile-time macro coverage when the default Tauri build compiles.

- [ ] **Step 5: Verify GREEN**

Run: `cargo test --lib entrypoints::tauri_api::tests::book_progress`

Expected: all focused Tauri tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/entrypoints/tauri_api.rs
git commit -m "feat: expose book progress Tauri commands"
```

### Task 7: OpenAPI Contract

**Files:**
- Modify: `docs/api/openapi.yaml`
- Modify: `tests/openapi_contract_test.rs`

**Interfaces:**
- Consumes: REST paths and JSON shapes from Tasks 1 and 5.
- Produces: reusable `BookProgress`, `BookProgressList`, `SaveBookProgressRequest`, `BookLocator`, `EpubCfiLocator`, and `PdfPageLocator` schemas.

- [ ] **Step 1: Extend schema-test helper and write failing contract assertions**

Teach `schema_accepts` to enforce numeric `minimum`/`maximum`, string `minLength`, and `format: date-time` by parsing RFC 3339. Add representative valid EPUB/PDF request and response values, then reject numeric/out-of-`i64` checksums, invalid type, blank locator, out-of-range progression, request `checksum`/`updatedOn`, and response records without server-owned fields.

- [ ] **Step 2: Verify RED**

Run: `cargo test --no-default-features --features webserver --test openapi_contract_test book_progress`

Expected: FAIL because paths and schemas are missing.

- [ ] **Step 3: Add OpenAPI paths and reusable schemas**

Document `GET /api/book-progress` and GET/PUT/DELETE `/api/book/{checksum}/progress`, including empty `204`, JSON `400/404/413/500/503`, and authentication `401`. Use a signed-i64 decimal-string pattern that rejects values outside `-9223372036854775808..=9223372036854775807` in tests (a reusable `BookChecksumValue` schema may use explicit regex alternation). Define locator as `oneOf` exact object variants with `additionalProperties: false`, `value: { type: string, minLength: 1, pattern: '.*\\S.*' }`; progression has `minimum: 0`, `maximum: 1`; `updatedOn` is required with `format: date-time`.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test --no-default-features --features webserver --test openapi_contract_test
```

Expected: typed parser, local semantic validator, `roas`, and progress payload tests all PASS.

- [ ] **Step 5: Commit**

```bash
git add docs/api/openapi.yaml tests/openapi_contract_test.rs
git commit -m "docs: add book progress OpenAPI contract"
```

### Task 8: Regression Verification and Review

**Files:**
- Verify only; modify files solely to fix failures attributable to Tasks 1-7.

**Interfaces:**
- Consumes: completed feature branch.
- Produces: fresh evidence for every configuration required by the design.

- [ ] **Step 1: Run formatting and whitespace checks**

```bash
cargo fmt --check
git diff --check
```

Expected: both exit `0`.

- [ ] **Step 2: Run default/Tauri verification**

```bash
cargo test --lib
cargo test --all-targets
```

Expected: both exit `0`. If the known macOS `system-configuration` panic in `adaptors::http_fetcher::tests` remains, record the exact five failing tests and separately run the same commands with those pre-existing tests skipped; do not claim the full commands pass.

- [ ] **Step 3: Run webserver verification**

```bash
cargo test --no-default-features --features webserver --lib
cargo test --no-default-features --features webserver --test book_api_test
cargo test --no-default-features --features webserver --test book_router_test
cargo test --no-default-features --features webserver --test openapi_contract_test
cargo test --no-default-features --features webserver --all-targets
```

Expected: every command exits `0`, subject only to an explicitly reported identical pre-existing environment failure.

- [ ] **Step 4: Audit scope and requirements**

Run `git status --short`, `git diff --stat <merge-base>..HEAD`, and inspect every changed file. Confirm all eight spec workstreams are covered and no `client/newapp` file is staged or committed.

- [ ] **Step 5: Request whole-branch review**

Use `superpowers:requesting-code-review`, fix every Critical/Important finding through a focused red-green cycle, re-run affected tests, and repeat review until clean.

- [ ] **Step 6: Finish the branch**

Use `superpowers:finishing-a-development-branch`; present merge/PR/keep/cleanup options only after fresh verification evidence has been read.
