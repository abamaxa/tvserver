# Book Delete Lease and Checksum Decision Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent explicit book deletion from racing orphan reconciliation, preserve the legacy ebook checksum identity as an explicit v1 decision, and document release panic-unwinding ownership.

**Architecture:** Extend the existing path coordinator with a blocking reconciliation acquisition and inject the runtime-owned coordinator into `BookStore`. Deletion holds that path-scoped lease across filesystem staging, conditional persistence, cleanup, and rollback, while periodic scanning keeps its non-blocking skip behavior. Record the checksum compatibility/risk decision in an ADR without changing the API or schema.

**Tech Stack:** Rust 2021, Tokio synchronization, async traits, SQLite through SQLx, Cargo release profiles, Markdown ADRs.

## Global Constraints

- Work on `spec/ebook-support`; do not merge into `main` or push unless explicitly requested.
- Keep the existing `i64` checksum algorithm, checksum string API, and SQLite schema unchanged.
- Keep full-content collision comparison and fail-closed source restoration unchanged.
- Keep `try_acquire_reconciling(path)` non-blocking for periodic scanner work.
- Serialize only operations targeting the same canonical book path; do not add a global scanner lock.
- Keep PDF/EPUB metadata behavior, ingestion routing, authentication policy, and frontend APIs unchanged.
- Use test-first development for each Rust behavior change.

---

### Task 1: Add blocking reconciliation acquisition

**Files:**
- Modify: `src/domain/services/book_path_lease.rs`
- Test: `src/domain/services/book_path_lease.rs`

**Interfaces:**
- Consumes: `BookPathLeaseCoordinator`, `BookPathLease`, and the existing path-keyed `LeaseState` map.
- Produces: `pub async fn acquire_reconciling(&self, path: &Path) -> BookPathLease`.

- [ ] **Step 1: Write the failing coordinator test**

Add this test beside the two existing lease lifecycle tests:

```rust
#[tokio::test]
async fn blocking_reconciling_lease_waits_for_processing() {
    let coordinator = BookPathLeaseCoordinator::new();
    let path = Path::new("library/collection/book.epub");
    let processing = coordinator.acquire_processing(path).await;
    let waiting_coordinator = coordinator.clone();
    let waiting_path = path.to_path_buf();
    let (acquired_tx, mut acquired_rx) = oneshot::channel();
    let waiter = tokio::spawn(async move {
        let reconciling = waiting_coordinator
            .acquire_reconciling(&waiting_path)
            .await;
        acquired_tx.send(()).unwrap();
        reconciling
    });

    assert!(tokio::time::timeout(Duration::from_millis(25), &mut acquired_rx)
        .await
        .is_err());
    drop(processing);
    tokio::time::timeout(Duration::from_secs(1), &mut acquired_rx)
        .await
        .expect("reconciliation should acquire after processing releases")
        .unwrap();
    drop(waiter.await.unwrap());
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test --lib domain::services::book_path_lease::tests::blocking_reconciling_lease_waits_for_processing -- --exact
```

Expected: compilation fails because `BookPathLeaseCoordinator` has no method named `acquire_reconciling`.

- [ ] **Step 3: Implement one shared blocking acquisition loop**

Replace the body of `acquire_processing` and add the private/public methods below. Leave `try_acquire_reconciling` unchanged.

```rust
async fn acquire(&self, path: &Path, state: LeaseState) -> BookPathLease {
    loop {
        let changed = self.inner.changed.notified();
        let acquired = {
            let mut states = self
                .inner
                .states
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if states.contains_key(path) {
                false
            } else {
                states.insert(path.to_path_buf(), state);
                true
            }
        };
        if acquired {
            return BookPathLease {
                inner: self.inner.clone(),
                path: path.to_path_buf(),
                state,
            };
        }
        changed.await;
    }
}

pub async fn acquire_processing(&self, path: &Path) -> BookPathLease {
    self.acquire(path, LeaseState::Processing).await
}

pub async fn acquire_reconciling(&self, path: &Path) -> BookPathLease {
    self.acquire(path, LeaseState::Reconciling).await
}
```

- [ ] **Step 4: Run the focused lease suite and verify GREEN**

Run:

```bash
cargo test --lib domain::services::book_path_lease::tests -- --nocapture
```

Expected: all three coordinator tests pass.

- [ ] **Step 5: Commit the coordinator API**

```bash
git add src/domain/services/book_path_lease.rs
git commit -m "feat: add blocking book reconciliation lease"
```

---

### Task 2: Hold the shared lease throughout explicit deletion

**Files:**
- Modify: `src/services/book_store.rs:1-48,137-225,405-590,850-950`
- Modify: `src/entrypoints/book_runtime.rs:129-153`
- Test: `src/services/book_store.rs`

**Interfaces:**
- Consumes: `BookPathLeaseCoordinator::acquire_reconciling(&Path) -> BookPathLease` from Task 1.
- Produces: `BookStore::new_with_roots_and_leases(store, thumbnail_store, repo, book_root, thumbnail_root, leases) -> BookStore` and a runtime-shared deletion lease.

- [ ] **Step 1: Add the deterministic failing race test**

Extend the test imports to include `BookCheck`, `BookPathLeaseCoordinator`, and `BookChecker`:

```rust
domain::{
    models::{
        BookDetails, BookFormat, BookState, CollectionItem, VideoDetails,
        DEFAULT_BOOK_THUMBNAIL,
    },
    services::{BookCheck, BookPathLeaseCoordinator},
    traits::{BookChecker, Databaser, FileStorer, Repository},
},
```

Add two optional synchronization fields to `RelocatingDeleteRepository` and a helper that pauses only the conditional delete:

```rust
struct RelocatingDeleteRepository {
    inner: Repository,
    relocated: BookDetails,
    delete_entered: Option<Arc<tokio::sync::Semaphore>>,
    delete_release: Option<Arc<tokio::sync::Semaphore>>,
}

impl RelocatingDeleteRepository {
    async fn relocate(&self) -> Result<(), sqlx::Error> {
        self.inner.save_book(&self.relocated).await.map(|_| ())
    }

    async fn wait_before_conditional_delete(&self) {
        if let (Some(entered), Some(release)) =
            (self.delete_entered.as_ref(), self.delete_release.as_ref())
        {
            entered.add_permits(1);
            release.acquire().await.unwrap().forget();
        }
    }
}
```

In `delete_book_if_path_matches`, pause after relocation and before forwarding the delete:

```rust
async fn delete_book_if_path_matches(
    &self,
    checksum: i64,
    collection: &str,
    file_name: &str,
) -> Result<u64, sqlx::Error> {
    self.relocate().await?;
    self.wait_before_conditional_delete().await;
    self.inner
        .delete_book_if_path_matches(checksum, collection, file_name)
        .await
}
```

Update the existing relocation-test construction with inactive barriers:

```rust
let repository: Repository = Arc::new(RelocatingDeleteRepository {
    inner: inner.clone(),
    relocated,
    delete_entered: None,
    delete_release: None,
});
```

Add the new regression after `delete_removes_book_generated_thumbnail_and_repository_row`:

```rust
#[tokio::test]
async fn delete_lease_prevents_orphan_reconciliation_from_restoring_the_book() {
    let layout = TestLayout::new("delete-orphan-reconciliation-race");
    let book_path = layout.book_root.join("Fiction/Dune.epub");
    tokio::fs::create_dir_all(book_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&book_path, b"book").await.unwrap();

    let inner: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    let book = sample_book(35, "Fiction", "Dune.epub");
    inner.save_book(&book).await.unwrap();
    let delete_entered = Arc::new(tokio::sync::Semaphore::new(0));
    let delete_release = Arc::new(tokio::sync::Semaphore::new(0));
    let repository: Repository = Arc::new(RelocatingDeleteRepository {
        inner: inner.clone(),
        relocated: book.clone(),
        delete_entered: Some(delete_entered.clone()),
        delete_release: Some(delete_release.clone()),
    });
    let book_files: FileStorer = Arc::new(FileSystemStore::new(
        layout.book_root.to_str().expect("book root should be UTF-8"),
    ));
    let thumbnail_files: FileStorer = Arc::new(FileSystemStore::new(
        layout
            .thumbnail_root
            .to_str()
            .expect("thumbnail root should be UTF-8"),
    ));
    let leases = BookPathLeaseCoordinator::new();
    let store = BookStore::new_with_roots_and_leases(
        book_files.clone(),
        thumbnail_files,
        repository.clone(),
        &layout.book_root,
        &layout.thumbnail_root,
        leases.clone(),
    );
    let (sender, _receiver) = tokio::sync::mpsc::channel(8);
    let checker = BookCheck::new_with_root_and_leases(
        book_files,
        repository,
        sender,
        &layout.book_root,
        leases,
    );

    let deletion = tokio::spawn(async move { store.delete(book.checksum).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        delete_entered.acquire(),
    )
    .await
    .expect("delete should reach the conditional database operation")
    .unwrap()
    .forget();

    checker.check_book_information().await.unwrap();

    assert!(inner.retrieve_book(35).await.is_ok());
    delete_release.add_permits(1);
    deletion.await.unwrap().unwrap();
    assert!(!book_path.exists());
    assert!(matches!(
        inner.retrieve_book(35).await,
        Err(sqlx::Error::RowNotFound)
    ));
}
```

- [ ] **Step 2: Run the race test and verify RED**

Run:

```bash
cargo test --lib services::book_store::tests::delete_lease_prevents_orphan_reconciliation_from_restoring_the_book -- --exact
```

Expected: compilation fails because `BookStore::new_with_roots_and_leases` does not exist. This missing injection point is part of the race defect.

- [ ] **Step 3: Inject the coordinator into `BookStore`**

Add the coordinator beside the existing algorithm, config, model, and trait
imports inside `use crate::domain::{...}`:

```rust
services::BookPathLeaseCoordinator,
```

Add the field and delegate standalone construction to a private coordinator:

```rust
#[derive(Clone)]
pub struct BookStore {
    store: FileStorer,
    thumbnail_store: FileStorer,
    repo: Repository,
    book_root: PathBuf,
    thumbnail_root: PathBuf,
    leases: BookPathLeaseCoordinator,
}

pub fn new_with_roots(
    store: FileStorer,
    thumbnail_store: FileStorer,
    repo: Repository,
    book_root: impl AsRef<Path>,
    thumbnail_root: impl AsRef<Path>,
) -> Self {
    Self::new_with_roots_and_leases(
        store,
        thumbnail_store,
        repo,
        book_root,
        thumbnail_root,
        BookPathLeaseCoordinator::new(),
    )
}

pub fn new_with_roots_and_leases(
    store: FileStorer,
    thumbnail_store: FileStorer,
    repo: Repository,
    book_root: impl AsRef<Path>,
    thumbnail_root: impl AsRef<Path>,
    leases: BookPathLeaseCoordinator,
) -> Self {
    Self {
        store,
        thumbnail_store,
        repo,
        book_root: book_root.as_ref().to_path_buf(),
        thumbnail_root: thumbnail_root.as_ref().to_path_buf(),
        leases,
    }
}
```

- [ ] **Step 4: Acquire the canonical-path lease before deletion work**

In `BookStore::delete`, place the lease immediately after building `book_path` and before containment inspection:

```rust
let book_path = self.book_root.join(collection).join(file_name);
let _reconciling = self.leases.acquire_reconciling(&book_path).await;
ensure_path_within_root(&book_path, &self.book_root, "book file").await?;
```

The binding must remain in the function scope through every success and rollback return.

- [ ] **Step 5: Share the runtime coordinator with `BookStore`**

Change `BookRuntime::try_initialize` to use the injected constructor and retain the original coordinator for ingestion:

```rust
let store = Arc::new(BookStore::new_with_roots_and_leases(
    book_storer.clone(),
    thumbnail_storer,
    repository,
    &book_root,
    &thumbnail_root,
    leases.clone(),
));
```

- [ ] **Step 6: Run the race test and affected suites and verify GREEN**

Run:

```bash
cargo test --lib services::book_store::tests::delete_lease_prevents_orphan_reconciliation_from_restoring_the_book -- --exact
cargo test --lib services::book_store::tests -- --nocapture
cargo test --lib domain::services::book_check::tests -- --nocapture
cargo test --lib entrypoints::book_runtime::tests -- --nocapture
```

Expected: the race regression and all affected suites pass.

- [ ] **Step 7: Commit the deletion lease**

```bash
git add src/services/book_store.rs src/entrypoints/book_runtime.rs
git commit -m "fix: serialize book deletion with reconciliation"
```

---

### Task 3: Record checksum ownership and panic containment

**Files:**
- Create: `docs/adr/0001-ebook-v1-checksum-identity.md`
- Modify: `Cargo.toml:71-76`

**Interfaces:**
- Consumes: the accepted decision in `docs/superpowers/specs/2026-07-18-book-delete-lease-checksum-decision-design.md`.
- Produces: an accepted ADR for ebook v1 identity and an inline release-profile ownership comment.

- [ ] **Step 1: Create the checksum identity ADR**

Create `docs/adr/0001-ebook-v1-checksum-identity.md` with exactly this decision record:

```markdown
# ADR 0001: Retain the Legacy `i64` Checksum for Ebook v1

- Status: Accepted
- Date: 2026-07-18

## Context

Books currently use the same legacy identity shape as videos: an `i64` produced
by Rust's `DefaultHasher` over a bounded prefix of the file. The checksum is the
SQLite primary key and is serialized as a string in REST, Tauri, and event
contracts.

Rust does not specify `DefaultHasher` as stable across releases. Prefix hashing
also permits distinct files to share a key. Ebook ingestion mitigates silent
data corruption by comparing complete contents when a checksum row already
exists. Different colliding content is rejected, the incoming source is
restored, and the canonical row and file are preserved.

## Decision

Ebook v1 retains the existing `i64` checksum algorithm, schema, and public
contract for compatibility with the shared video identity model. Persisted
checksums are treated as stored identifiers and are not recomputed solely
because the Rust toolchain changes.

This is explicit risk acceptance for the first ebook release, not approval of
`DefaultHasher` as a permanent durable identity algorithm.

## Consequences

- A healthy canonical row blocks ingestion of different content with the same
  checksum until that identity conflict is resolved.
- Complete-content comparison keeps collisions fail-closed instead of silently
  replacing or aliasing a book.
- Checksum values may differ when the same bytes are newly ingested by builds
  using different Rust hashing implementations.
- A future migration must introduce a specified full-content digest and a
  durable identifier independent of `DefaultHasher`.
- That migration must backfill existing rows, preserve compatibility with
  current checksum-based URLs and events during a transition, and define how
  legacy and stable identities are resolved before removing the `i64` key.

## Out of Scope

This decision does not change video identity, the books table, public API
routes, collision comparison, or source-restoration behavior.
```

- [ ] **Step 2: Document the release panic-profile dependency**

Replace the uncommented profile line with:

```toml
panic = "unwind" # Worker-level catch_unwind containment depends on unwinding.
```

- [ ] **Step 3: Verify the documentation assertions**

Run:

```bash
rg -n 'Status: Accepted|DefaultHasher|specified full-content digest' docs/adr/0001-ebook-v1-checksum-identity.md
rg -n 'panic = "unwind".*catch_unwind' Cargo.toml
git diff --check
```

Expected: all required decision terms and the ownership comment are present, and `git diff --check` exits zero.

- [ ] **Step 4: Commit the decision documentation**

```bash
git add Cargo.toml docs/adr/0001-ebook-v1-checksum-identity.md
git commit -m "docs: record ebook checksum identity decision"
```

---

### Task 4: Verify the complete review fix

**Files:**
- Verify only: all files changed since `70c37b7`

**Interfaces:**
- Consumes: the blocking lease, runtime injection, race regression, ADR, and Cargo comment from Tasks 1-3.
- Produces: fresh evidence that the review fix is complete under both supported feature configurations.

- [ ] **Step 1: Run the complete library suite**

Run outside the restricted sandbox when necessary for the macOS `system-configuration` HTTP tests:

```bash
cargo test --lib --quiet
```

Expected: 274 runnable tests pass and the existing four ignored tests remain ignored.

- [ ] **Step 2: Compile every target in both feature configurations**

Run:

```bash
cargo test --all-targets --no-run --quiet
cargo test --features webserver --all-targets --no-run --quiet
```

Expected: both commands exit zero.

- [ ] **Step 3: Verify scope and branch state**

Run:

```bash
git diff --check 70c37b7..HEAD
git status --short --branch
git diff --stat 70c37b7..HEAD
git log --oneline -4
```

Expected: no whitespace errors, a clean `spec/ebook-support` worktree, and only the planned lease, runtime, test, Cargo comment, and ADR changes after the design commit.

- [ ] **Step 4: Request final code review before completion**

Review the diff `70c37b7..HEAD` against the approved design. Treat any Critical or Important finding as blocking; fix it test-first and repeat the verification commands before reporting completion.
