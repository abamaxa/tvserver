# Embedded Book Reading Progress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the separate reading-progress subsystem with an optional JSON-backed `progress` field on every book while retaining one narrow save operation.

**Architecture:** SQLite stores the nested progress object in `books.progress`; existing book list/detail queries deserialize it into `BookDetails`. REST and Tauri keep only save operations, while React reads progress from `BookDetails`, updates cached books optimistically, and removes the parallel progress fetch/store.

**Tech Stack:** Rust 2021, SQLx/SQLite, Serde, Axum 0.8, Tauri 2, OpenAPI 3.1, React 18, TypeScript, Redux Toolkit, Jest.

## Global Constraints

- Backend paths are relative to the `tvserver`/`src-tauri` repository; paths beginning with `../` are in the parent `lots-of-videos` repository.
- Update backend PR #65 on `codex/book-reading-progress`; do not replace it.
- Use isolated worktrees because both existing checkouts contain unrelated user changes.
- Rewrite undeployed `migrations/20260719000001_book_progress.sql` in place.
- `BookDetails.progress` is optional and contains locator, optional progression, and server-authored `updatedOn`; it never repeats checksum.
- Keep only REST PUT and Tauri `save_book_progress`; success returns no value/`204 No Content`.
- A progress save does not change book-level `updated_on` or emit a metadata event.
- Same-checksum ingestion preserves progress; same-path/different-checksum replacement discards it.
- Checksums remain decimal strings at transport boundaries.
- Do not modify or stage dirty `.github`, generated `client/newapp`, frontend configuration, search-service, temporary database, or unrelated design files.

---

### Task 1: Embed Progress Types in `BookDetails`

**Files:**
- Modify: `src/domain/models/book.rs`
- Test: `src/domain/models/book.rs`

**Interfaces:**
- Consumes: existing `BookDetails` custom serializer.
- Produces: `BookLocatorType`, `BookLocator`, `SaveBookProgressRequest`, `BookReadingProgress`, `SaveBookProgressRequest::validate`, and `BookDetails.progress`.

- [ ] **Step 1: Write failing model tests**

Add:

```rust
#[test]
fn serializes_optional_embedded_progress_without_checksum() {
    let mut book = sample_book();
    book.progress = Some(BookReadingProgress {
        locator: BookLocator {
            locator_type: BookLocatorType::EpubCfi,
            value: "epubcfi(/6/4)".into(),
        },
        progression: Some(0.42),
        updated_on: "2026-07-19T12:00:00.000Z".into(),
    });
    let value = serde_json::to_value(book).unwrap();
    assert_eq!(value["progress"], serde_json::json!({
        "locator": {"type": "epub-cfi", "value": "epubcfi(/6/4)"},
        "progression": 0.42,
        "updatedOn": "2026-07-19T12:00:00.000Z"
    }));
    assert!(value["progress"].get("checksum").is_none());
}

#[test]
fn validates_progress_request_boundaries() {
    let mut request: SaveBookProgressRequest = serde_json::from_value(
        serde_json::json!({"locator": {"type": "pdf-page", "value": "opaque"}, "progression": 1.0})
    ).unwrap();
    assert_eq!(request.validate(), Ok(()));
    request.locator.value = "\u{2003}".into();
    assert_eq!(request.validate(), Err("book locator value must not be blank"));
    request.locator.value = "1".into();
    request.progression = Some(f64::NAN);
    assert_eq!(
        request.validate(),
        Err("book progression must be finite and between 0 and 1")
    );
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --lib domain::models::book::test`

Expected: compilation fails because the new types and field do not exist.

- [ ] **Step 3: Add the minimal model**

```rust
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BookLocatorType { EpubCfi, PdfPage }

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BookLocator {
    #[serde(rename = "type")]
    pub locator_type: BookLocatorType,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveBookProgressRequest {
    pub locator: BookLocator,
    pub progression: Option<f64>,
}

impl SaveBookProgressRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.locator.value.trim().is_empty() {
            return Err("book locator value must not be blank");
        }
        if self.progression.is_some_and(|value| {
            !value.is_finite() || !(0.0..=1.0).contains(&value)
        }) {
            return Err("book progression must be finite and between 0 and 1");
        }
        Ok(())
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookReadingProgress {
    pub locator: BookLocator,
    pub progression: Option<f64>,
    pub updated_on: String,
}
```

Add `pub progress: Option<BookReadingProgress>` to `BookDetails` and update its serializer count/body. Keep the new types private to the `book` module for this intermediate commit; Task 2 removes the old progress module and then re-exports these names without a collision.

- [ ] **Step 4: Verify GREEN**

Run: `cargo test --lib domain::models::book::test`

Expected: all model tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/domain/models/book.rs
git commit -m "refactor: embed reading progress in books"
```

### Task 2: Replace the Backend Progress Subsystem

**Files:**
- Modify: `migrations/20260719000001_book_progress.sql`
- Modify: `src/domain/traits.rs`
- Modify: `src/adaptors/repository.rs`
- Modify: `src/domain/models/mod.rs`
- Delete: `src/domain/models/book_progress.rs`
- Modify: `src/domain/services/mod.rs`
- Delete: `src/domain/services/book_progress.rs`
- Modify: `src/domain/services/book_metadata.rs`
- Modify: `src/services/book_store.rs`
- Modify: `src/entrypoints/api.rs`
- Modify: `src/entrypoints/tauri_api.rs`
- Test: `src/adaptors/repository.rs`
- Test: `src/entrypoints/tauri_api.rs`
- Test: `tests/book_api_test.rs`
- Test: `tests/book_router_test.rs`

**Interfaces:**
- Consumes: Task 1 embedded model.
- Produces: `Databaser::save_book_progress(checksum, &request) -> Result<bool, sqlx::Error>`, REST 204 PUT, and unit-returning Tauri save.

- [ ] **Step 1: Write failing simplified tests**

Replace old CRUD/table tests with:

```rust
#[tokio::test]
async fn book_progress_is_embedded_in_book_rows() {
    let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
    let book = sample_book(42, "Shelf", "book.epub", "Book");
    db.save_book(&book).await.unwrap();
    let request = SaveBookProgressRequest {
        locator: BookLocator {
            locator_type: BookLocatorType::EpubCfi,
            value: "epubcfi(/6/4)".into(),
        },
        progression: Some(0.5),
    };
    assert!(db.save_book_progress(42, &request).await.unwrap());
    let stored = db.retrieve_book(42).await.unwrap().progress.unwrap();
    assert_eq!(stored.locator, request.locator);
    assert_eq!(stored.progression, Some(0.5));
    assert!(stored.updated_on.ends_with('Z'));
}

#[tokio::test]
async fn progress_migration_adds_only_the_books_column() {
    let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
    let names: Vec<String> = sqlx::query("PRAGMA table_info(books)")
        .fetch_all(&db.pool).await.unwrap().iter().map(|row| row.get("name")).collect();
    assert!(names.contains(&"progress".to_string()));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='book_progress'"
    ).fetch_one(&db.pool).await.unwrap();
    assert_eq!(count, 0);
}
```

Add a lifecycle test that saves progress, calls `save_book` again with the same checksum and asserts the progress remains, then saves a same-path book with a different checksum and asserts the replacement has `progress == None` while the old checksum returns `RowNotFound`.

REST tests must expect PUT 204, then GET the book and assert nested progress. Removed list/get/delete progress routes must return 404. Tauri core tests must expect `()`.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test --lib adaptors::repository::tests::book_progress -- --test-threads=1
cargo test --lib entrypoints::tauri_api::tests::book_progress
cargo test --no-default-features --features webserver --test book_api_test book_progress -- --test-threads=1
cargo test --no-default-features --features webserver --test book_router_test book_progress
```

Expected: failures identify the old table/routes/results and missing column.

- [ ] **Step 3: Rewrite persistence**

Migration contents:

```sql
ALTER TABLE books ADD COLUMN progress TEXT;
```

Decode beside metadata:

```rust
let progress = row
    .get::<Option<String>, _>("progress")
    .and_then(|value| serde_json::from_str::<BookReadingProgress>(&value).ok());
```

Set `BookDetails.progress`. Remove the old row mapper, four CRUD methods/outcome enums, fallback helper, and foreign-key-only pool configuration.

- [ ] **Step 4: Implement the one repository method**

Trait:

```rust
async fn save_book_progress(
    &self,
    checksum: i64,
    progress: &SaveBookProgressRequest,
) -> Result<bool, sqlx::Error>;
```

Implementation:

```rust
let stored = BookReadingProgress {
    locator: progress.locator.clone(),
    progression: progress.progression,
    updated_on: chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
};
let encoded = serde_json::to_string(&stored)
    .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
let result = sqlx::query("UPDATE books SET progress = ? WHERE checksum = ?")
    .bind(encoded).bind(checksum).execute(&self.pool).await?;
Ok(result.rows_affected() == 1)
```

Delegate this signature from the two test wrappers. Keep `progress` out of all `save_book` insert/conflict assignments.

- [ ] **Step 5: Replace REST with one ordinary PUT**

```rust
async fn save_book_progress(
    State(state): State<SharedState>,
    Path(checksum): Path<String>,
    Json(progress): Json<SaveBookProgressRequest>,
) -> AxumResponse {
    if state.get_available_book_runtime().is_none() {
        return book_library_unavailable_response().into_response();
    }
    let checksum = match parse_book_checksum(&checksum) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    if let Err(message) = progress.validate() {
        return std_error(BAD_REQUEST, message.to_string()).into_response();
    }
    match state.get_repository().save_book_progress(checksum, &progress).await {
        Ok(true) => NO_CONTENT.into_response(),
        Ok(false) => std_error(NOT_FOUND, BOOK_NOT_FOUND_MESSAGE.to_string()).into_response(),
        Err(error) => {
            tracing::error!("Failed to save book progress: {}", error);
            std_error(INTERNAL_SERVER_ERROR, INTERNAL_ERROR_MESSAGE.to_string()).into_response()
        }
    }
}
```

Remove list/get/delete routes and handlers, the service, and custom JSON rejection precedence.

- [ ] **Step 6: Replace Tauri with one save command**

```rust
async fn save_book_progress_core(
    repository: &Repository,
    checksum: &str,
    progress: SaveBookProgressRequest,
) -> Result<(), String> {
    let checksum = parse_book_checksum(checksum)?;
    progress.validate().map_err(str::to_string)?;
    match repository.save_book_progress(checksum, &progress).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(BOOK_NOT_FOUND_MESSAGE.to_string()),
        Err(error) => Err(error.to_string()),
    }
}
```

Register only `save_book_progress`. Delete old model/service files and exports.

- [ ] **Step 7: Verify GREEN**

Run all four Step 2 commands.

Expected: every selected suite passes and no separate table/read/delete route remains.

- [ ] **Step 8: Commit**

```bash
git add migrations/20260719000001_book_progress.sql src tests
git commit -m "refactor: store progress on book records"
```

### Task 3: Simplify OpenAPI

**Files:**
- Modify: `docs/api/openapi.yaml`
- Modify: `tests/openapi_contract_test.rs`

**Interfaces:**
- Consumes: Task 2 REST behavior.
- Produces: optional nested progress and PUT-only documentation.

- [ ] **Step 1: Write failing contract test**

```rust
#[test]
fn book_progress_is_nested_and_only_put_is_documented() {
    let document = contract();
    let path = &document["paths"]["/api/book/{checksum}/progress"];
    assert!(path.get("get").is_none());
    assert!(path.get("delete").is_none());
    assert!(path["put"]["responses"].get("204").is_some());
    assert!(document["paths"].get("/api/book-progress").is_none());
    assert_eq!(
        document["components"]["schemas"]["BookDetails"]["properties"]["progress"]["$ref"],
        "#/components/schemas/BookReadingProgress"
    );
    assert!(document["components"]["schemas"]["BookReadingProgress"]["properties"]
        .get("checksum").is_none());
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test --no-default-features --features webserver --test openapi_contract_test book_progress`

Expected: old list/get/delete and checksum assertions fail.

- [ ] **Step 3: Implement contract**

Add:

```yaml
progress:
  $ref: '#/components/schemas/BookReadingProgress'
```

to `BookDetails.properties`. Define `BookReadingProgress` with required locator/updatedOn, optional bounded progression, and no checksum. Keep `SaveBookProgressRequest` locator/progression only. Retain PUT with 204/400/401/404/500/503; remove `/api/book-progress`, GET/DELETE, and `BookProgressList`.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test --no-default-features --features webserver --test openapi_contract_test book_progress
cargo test --no-default-features --features webserver --test openapi_contract_test openapi_contract_typed
```

Expected: all selected contract tests pass.

- [ ] **Step 5: Commit**

```bash
git add docs/api/openapi.yaml tests/openapi_contract_test.rs
git commit -m "docs: simplify book progress contract"
```

### Task 4: Collapse Frontend Progress State into Books

**Files:**
- Modify: `../src/domain/Books.ts`
- Modify: `../src/services/Books.ts`
- Modify: `../src/services/Books.test.ts`
- Modify: `../src/domain/Store/BookReducer.ts`
- Modify: `../src/domain/Store/BookReducer.test.ts`
- Modify: `../src/adaptors/Interfaces.ts`
- Modify: `../src/adaptors/RestAdaptor.ts`

**Interfaces:**
- Consumes: Task 3 response shape.
- Produces: `BookDetails.progress`, `cacheBookProgress({checksum, progress})`, and save-only `BookService`.

- [ ] **Step 1: Write failing service/reducer tests**

```typescript
test('reads nested progress and sends only narrow saves', async () => {
  adaptor.get.mockResolvedValue({ ...book, progress });
  await expect(service.getBook(book.checksum)).resolves.toMatchObject({ progress });
  adaptor.put.mockResolvedValue(new Response(null, { status: 204 }));
  await service.saveProgress(book.checksum, {
    locator: progress.locator,
    progression: progress.progression,
  });
  expect(adaptor.put).toHaveBeenCalledWith(
    `book/${encodeURIComponent(book.checksum)}/progress`,
    { locator: progress.locator, progression: progress.progression },
  );
});

test('updates progress in detail and loaded collections', () => {
  let state = bookReducer(undefined, fetchBookCollection.fulfilled(collection, 'request', ''));
  state = bookReducer(state, fetchBookDetails.fulfilled(book, 'detail', book.checksum));
  state = bookReducer(state, cacheBookProgress({ checksum: book.checksum, progress }));
  expect(state.details[book.checksum].progress).toEqual(progress);
  expect(state.collections[''].books[0].progress).toEqual(progress);
});
```

- [ ] **Step 2: Verify RED**

Run: `npm run jest -- --runInBand src/services/Books.test.ts src/domain/Store/BookReducer.test.ts`

Expected: missing nested field/action failures.

- [ ] **Step 3: Change types and service**

```typescript
export interface BookReadingProgress extends BookProgressUpdate {
  updatedOn: string;
}
```

Insert `progress?: BookReadingProgress` after `updatedOn` in the existing `BookDetails` interface; keep every existing field unchanged.

`isBookDetails` accepts absent progress or `isReadingProgress`; remove checksum from progress validation. Delete `listProgress`, `getProgress`, and `deleteProgress`. Remove unused `getOptional` from the adaptor interface, HTTP implementation, mocks, and tests.

- [ ] **Step 4: Replace Redux progress state**

Delete the progress map, load state, generation maps, fetch thunk, selectors, and reducers. Copy nested progress in `serializableBook`. Add:

```typescript
cacheBookProgress: (
  state,
  action: PayloadAction<{ checksum: string; progress: BookReadingProgress }>,
) => {
  const { checksum, progress } = action.payload;
  const cached = serializableProgress(progress);
  if (state.details[checksum]) state.details[checksum].progress = cached;
  for (const collection of Object.values(state.collections)) {
    const book = collection.books.find((item) => item.checksum === checksum);
    if (book) book.progress = cached;
  }
},
```

- [ ] **Step 5: Verify GREEN**

Run the Step 2 command.

Expected: both suites pass without separate progress state or reads.

- [ ] **Step 6: Commit in the isolated parent worktree**

```bash
git add src/domain/Books.ts src/services/Books.ts src/services/Books.test.ts src/domain/Store/BookReducer.ts src/domain/Store/BookReducer.test.ts src/adaptors/Interfaces.ts src/adaptors/RestAdaptor.ts
git commit -m "refactor: embed reading progress in books"
```

### Task 5: Simplify Reader, Library, and Tauri Integration

**Files:**
- Modify: `../src/reader/ReaderSessionController.ts`
- Modify: `../src/reader/ReaderSessionController.test.ts`
- Modify: `../src/components/Books/BookLibraryPage.tsx`
- Modify: `../src/components/Books/BookLibraryPage.test.tsx`
- Modify: `../src/components/Books/BookShelf.tsx`
- Modify: `../src/components/Books/BookCard.tsx`
- Modify: `../src/components/Books/BookRow.tsx`
- Modify: `../src/adaptors/TauriRestAdaptor.ts`
- Modify: `../src/adaptors/TauriRestAdaptor.books.test.ts`
- Modify: `../e2e/book-contracts.spec.ts`
- Modify: `../e2e/book-reader.spec.ts`
- Modify: `../e2e/book-security.spec.ts`

**Interfaces:**
- Consumes: Task 4 embedded state/action.
- Produces: one-fetch reader restoration, direct library rendering, and save-only Tauri mapping.

- [ ] **Step 1: Write failing integration tests**

Make `getBook` return a book containing progress; remove `getProgress`; assert:

```typescript
expect(bookService.getBook).toHaveBeenCalledWith(book.checksum);
expect(bookService.saveProgress).toHaveBeenCalledWith(book.checksum, {
  locator: relocation.locator,
  progression: relocation.progression,
});
expect(dispatch).toHaveBeenCalledWith(
  cacheBookProgress({
    checksum: book.checksum,
    progress: expect.objectContaining({ locator: relocation.locator }),
  }),
);
```

Library tests pass only books and expect their own percentages. Tauri tests keep PUT and reject removed progress GET/DELETE mappings.

- [ ] **Step 2: Verify RED**

Run:

```bash
npm run jest -- --runInBand src/reader/ReaderSessionController.test.ts src/components/Books/BookLibraryPage.test.tsx src/adaptors/TauriRestAdaptor.books.test.ts
```

Expected: old `getProgress`, parallel props, and removed mappings cause failures.

- [ ] **Step 3: Restore from fetched book**

```typescript
type ReaderBookService = Pick<BookService, 'getBook' | 'saveProgress'>;
```

Fetch only `book`, then call `restoreProgress(renderer, book.progress, ...)`. Make `DirtyProgress` hold `checksum` separately from `record: BookReadingProgress`. On relocation:

```typescript
const progress: BookReadingProgress = {
  locator: { ...relocation.locator },
  updatedOn: this.now(),
  ...(relocation.progression === undefined ? {} : { progression: relocation.progression }),
};
this.pendingProgress.set(checksum, {
  checksum,
  record: progress,
  sequence: ++this.progressSequence,
  sessionVersion: this.openVersion,
});
this.dispatch(cacheBookProgress({ checksum, progress }));
```

Use `dirty.checksum` for saves/map keys. Preserve debounce, close flush, retry, and unsynced warnings.

- [ ] **Step 4: Read progress directly in library**

Remove `fetchBookProgress` and the progress selector/effect from `BookLibraryPage`. Remove progress-map props from shelf/card/row and render:

```tsx
<BookProgress book={book} progress={book.progress} />
```

- [ ] **Step 5: Remove obsolete Tauri/E2E routes**

Delete Tauri progress list, optional GET, and DELETE mapping/arguments; keep PUT-to-`save_book_progress`. Remove `getOptional`. Make E2E book fixtures embed progress and intercept only PUT saves.

- [ ] **Step 6: Verify GREEN**

Run the Step 2 command.

Expected: all selected suites pass.

- [ ] **Step 7: Commit**

```bash
git add src/reader src/components/Books src/adaptors/TauriRestAdaptor.ts src/adaptors/TauriRestAdaptor.books.test.ts e2e/book-contracts.spec.ts e2e/book-reader.spec.ts e2e/book-security.spec.ts
git commit -m "refactor: read progress from book records"
```

### Task 6: Full Verification and Publication

**Files:**
- Modify: parent repository `src-tauri` pointer after backend push.
- Verify: both repositories and both PRs.

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces: updated backend PR #65 and a frontend draft PR against `spec/ebook-support`.

- [ ] **Step 1: Verify backend format and whitespace**

Run:

```bash
cargo fmt --check
git diff --check origin/spec/ebook-support...HEAD
```

Expected: exit 0. If full format reports known legacy files only, record exact paths and run `rustfmt --check` on every changed Rust file; do not reformat unrelated code.

- [ ] **Step 2: Run backend matrix**

```bash
cargo test --all-targets
cargo test --no-default-features --features webserver --all-targets
```

Expected: all enabled tests pass. If the five established macOS `http_fetcher` system-configuration panics recur, record the unfiltered output and rerun filtering only those exact test names.

- [ ] **Step 3: Run frontend matrix**

```bash
npm run prettier:check
npm run typecheck
npm run jest -- --runInBand
npm run build
```

Expected: all exit 0; do not run auto-fixing lint over unrelated files.

- [ ] **Step 4: Audit scope**

In each repository run `git status --short`, `git diff --name-only origin/spec/ebook-support...HEAD`, and `git diff --check`. Backend may contain only progress source/tests/docs/migration; frontend may contain only book/reader/adaptor/E2E files plus intentional `src-tauri` pointer.

- [ ] **Step 5: Update backend PR #65**

Push `codex/book-reading-progress`. Verify through GitHub that PR #65 targets `spec/ebook-support`, is mergeable, and has no separate progress table or list/get/delete route.

- [ ] **Step 6: Publish frontend review branch**

Point the isolated frontend worktree's `src-tauri` submodule at the pushed backend head, stage only intended frontend files and `src-tauri`, commit `refactor: simplify book reading progress`, push a `codex/` branch, and open a draft PR against frontend `spec/ebook-support`. Link backend PR #65 and identify the submodule dependency.

- [ ] **Step 7: Verify remotes**

Fetch both PRs through GitHub and confirm base/head SHAs, draft state, changed-file lists, and mergeability. Report exact test evidence and any baseline failures.
