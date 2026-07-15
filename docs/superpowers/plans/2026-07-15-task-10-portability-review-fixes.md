# Task 10 Portability Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Task 10's default-cover publication, collection identifiers, generated-thumbnail tests, and repository diff portable across mobile, desktop, and cloud targets.

**Architecture:** Keep filesystem paths native at I/O boundaries, but introduce one shared domain helper that serializes normal relative path components with `/` for repository and HTTP values. Publish the embedded default cover with a process-wide standard-library mutex and an explicit remove-then-rename sequence, and make thumbnail tests construct valid JPEG data in isolated temporary roots.

**Tech Stack:** Rust standard library, Tokio, Axum/Reqwest integration tests, existing `anyhow` error handling, Cargo/Make, Git worktrees.

## Global Constraints

- Use only the Rust standard library for default-cover replacement; add no platform-specific runtime dependency.
- Domain, repository, JSON, and HTTP collection identifiers always use `/`; native `PathBuf` values remain confined to filesystem boundaries.
- Preserve Task 8's staging, private snapshot, locking, cleanup, cancellation, and no-replace publication behavior.
- Replace a stale regular file or symlink at `default-book.jpg` without following a symlink target.
- Accept the documented brief remove-to-rename availability gap in exchange for one implementation across Windows, macOS, Linux, mobile, and cloud.
- Remove the Task 10-only `/.worktrees/` and `/.superpowers/` tracked ignore additions.
- Do not add HTTP range support in this follow-up.
- Do not claim Windows execution from a non-Windows host; Windows-only tests run under `#[cfg(windows)]`.

---

### Task 1: Portable Default-Cover Replacement

**Files:**
- Modify: `src/domain/models/book.rs:1-59`
- Test: `src/domain/models/book.rs:531-618`

**Interfaces:**
- Consumes: `default_book_thumbnail_bytes() -> &'static [u8]` and `DEFAULT_BOOK_THUMBNAIL: &str`.
- Produces: unchanged public interface `ensure_default_book_thumbnail<P: AsRef<Path>>(thumbnail_dir: P) -> io::Result<PathBuf>` with serialized, portable stale-destination replacement.

- [ ] **Step 1: Preserve and focus the stale-cover regression**

Split the existing materialization test so stale replacement is an explicit regression and retain the Unix symlink-target safety test:

```rust
#[test]
fn stale_default_thumbnail_is_replaced_with_embedded_jpeg() {
    let thumbnail_dir = std::env::temp_dir().join(format!(
        "tvserver-book-thumbnail-stale-test-{}",
        std::process::id()
    ));
    if thumbnail_dir.exists() {
        std::fs::remove_dir_all(&thumbnail_dir).unwrap();
    }
    std::fs::create_dir_all(&thumbnail_dir).unwrap();
    let thumbnail_path = thumbnail_dir.join(DEFAULT_BOOK_THUMBNAIL);
    std::fs::write(&thumbnail_path, b"stale").unwrap();

    ensure_default_book_thumbnail(&thumbnail_dir).unwrap();

    assert_eq!(
        std::fs::read(&thumbnail_path).unwrap(),
        default_book_thumbnail_bytes()
    );
    std::fs::remove_dir_all(&thumbnail_dir).unwrap();
}
```

Keep `default_thumbnail_provisioning_does_not_follow_symlinks` unchanged so it continues asserting that the external target remains `b"do not replace"` and the destination becomes a regular embedded JPEG.

- [ ] **Step 2: Run the regression before implementation**

Run: `cargo test --no-default-features --features webserver --lib stale_default_thumbnail_is_replaced_with_embedded_jpeg`

Expected on this Unix host: PASS because Unix permits rename-over-existing. Expected on Windows before the implementation: FAIL when `fs::rename` encounters the existing destination. This is a platform-semantic regression, so the review finding plus the Windows expectation is the red evidence.

- [ ] **Step 3: Serialize and replace the destination portably**

Import `Mutex`, add a process-wide lock, and replace the direct rename with explicit no-follow destination removal:

```rust
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

pub const DEFAULT_BOOK_THUMBNAIL: &str = "default-book.jpg";
static NEXT_DEFAULT_THUMBNAIL_TEMP: AtomicU64 = AtomicU64::new(0);
static DEFAULT_BOOK_THUMBNAIL_LOCK: Mutex<()> = Mutex::new(());
```

At the start of `ensure_default_book_thumbnail`, immediately after `create_dir_all`, acquire the lock and map poisoning to an I/O error:

```rust
let _guard = DEFAULT_BOOK_THUMBNAIL_LOCK.lock().map_err(|_| {
    io::Error::other("default book thumbnail materialization lock is poisoned")
})?;
```

Inside the existing closure, after syncing and dropping the temporary file, replace `fs::rename(&temp_path, &thumbnail_path)?;` with:

```rust
match fs::symlink_metadata(&thumbnail_path) {
    Ok(_) => fs::remove_file(&thumbnail_path)?,
    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
    Err(error) => return Err(error),
}
fs::rename(&temp_path, &thumbnail_path)?;
```

Keep the existing unique same-directory temp name, file sync, Unix directory sync, and post-closure `remove_file(&temp_path)` error cleanup. A directory at the destination must continue to fail at `remove_file` rather than being recursively removed.

- [ ] **Step 4: Run focused default-cover tests**

Run: `cargo test --no-default-features --features webserver --lib default_thumbnail`

Expected: all matching tests PASS, including materialization, stale replacement, idempotence, and Unix symlink safety.

- [ ] **Step 5: Commit the portable replacement**

```bash
git add src/domain/models/book.rs
git commit -m "fix: replace stale default book cover portably"
```

---

### Task 2: Platform-Neutral Collection Identifiers

**Files:**
- Modify: `src/domain/algorithm/naming.rs:1-155`
- Modify: `src/domain/algorithm/mod.rs:8-29`
- Modify: `src/domain/services/book_metadata.rs:1-9,980-1000`
- Test: `src/domain/services/book_metadata.rs:4948-4996`
- Modify: `src/services/book_store.rs:1-124`
- Test: `src/services/book_store.rs:520-567`

**Interfaces:**
- Produces: `pub fn path_to_collection_id(path: &Path) -> Option<String>` in `domain::algorithm`; it returns `Some("")` for an empty path, joins only `Component::Normal` UTF-8 components with `/`, and returns `None` for roots, prefixes, parent/current traversal, or invalid UTF-8.
- Consumes: Task 8's unchanged `collection_from_source(path: &Path, book_root: &Path) -> anyhow::Result<String>` and BookStore's unchanged `collection_from_source(&self, full_path: &Path) -> Result<String>`.

- [ ] **Step 1: Add failing shared-helper tests**

Add these tests to `src/domain/algorithm/naming.rs`:

```rust
#[test]
fn collection_ids_join_native_path_components_with_forward_slashes() {
    let path = ["Fiction", "Classics", "British"].iter().collect::<PathBuf>();
    assert_eq!(
        path_to_collection_id(&path),
        Some("Fiction/Classics/British".to_string())
    );
    assert_eq!(path_to_collection_id(Path::new("")), Some(String::new()));
    assert_eq!(path_to_collection_id(Path::new("../Fiction")), None);
}

#[cfg(windows)]
#[test]
fn collection_ids_normalize_native_windows_separators() {
    assert_eq!(
        path_to_collection_id(Path::new(r"Fiction\Classics\British")),
        Some("Fiction/Classics/British".to_string())
    );
}
```

- [ ] **Step 2: Run the helper test to verify it fails**

Run: `cargo test --no-default-features --features webserver --lib collection_ids_join_native_path_components_with_forward_slashes`

Expected: compilation FAIL because `path_to_collection_id` does not exist.

- [ ] **Step 3: Implement and export the shared converter**

Add this function to `src/domain/algorithm/naming.rs`:

```rust
pub fn path_to_collection_id(path: &Path) -> Option<String> {
    path.components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|components| components.join("/"))
}
```

Re-export it from `src/domain/algorithm/mod.rs` alongside the rooted collection helpers.

Update `get_collection_from_rooted_path` to choose either `short_path.as_path()` or `short_path.parent()` and then call `path_to_collection_id(...).unwrap_or_default()`. Update `get_collection_and_file_from_rooted_path` so its parent value uses the same helper while its file-name handling stays unchanged.

- [ ] **Step 4: Add consumer regressions at the Task 8 and BookStore boundaries**

In Task 8's `book_metadata` tests, add a real ingestion test using existing `TestDir`, `write_epub`, and `ingestion_dependencies` helpers:

```rust
#[tokio::test]
async fn ingestion_persists_nested_collection_as_portable_identifier() {
    let temp = TestDir::new();
    let book_root = temp.path().join("books");
    let thumbnail_root = temp.path().join("book-thumbnails");
    let source = book_root.join("Fiction").join("Classics").join("Emma.epub");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    write_epub(
        &source,
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest/></package>"#,
        &[],
    );
    let (storer, repository) = ingestion_dependencies(&book_root).await;

    let details = generate_book_metadata_with_roots(
        source.clone(),
        storer,
        repository.clone(),
        None,
        book_root.clone(),
        thumbnail_root,
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(details.collection, "Fiction/Classics");
    assert_eq!(
        repository.retrieve_book(details.checksum).await.unwrap().collection,
        "Fiction/Classics"
    );
    assert!(book_root.join("Fiction").join("Classics").join("Emma.epub").exists());
}
```

In `book_store` tests, add:

```rust
#[tokio::test]
async fn collection_from_nested_book_root_uses_portable_identifier() {
    let layout = TestLayout::new("nested-portable-collection");
    let source = layout.book_root.join("Fiction").join("Classics").join("Emma.epub");
    let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

    assert_eq!(
        store.collection_from_source(&source).unwrap(),
        "Fiction/Classics"
    );
}
```

- [ ] **Step 5: Route both consumers through the shared converter**

Import `path_to_collection_id` next to `title_case` in `book_metadata.rs` and next to the existing algorithm imports in `book_store.rs`.

In both `collection_from_source` implementations, replace `relative.to_str().map(str::to_string)` with:

```rust
path_to_collection_id(relative)
    .ok_or_else(|| anyhow::anyhow!("book collection path is not valid UTF-8"))
```

For the metadata outside-root fallback, retain the existing immediate-parent behavior but serialize that single component through the helper:

```rust
let fallback = parent.file_name().map(Path::new).unwrap_or_else(|| Path::new(""));
path_to_collection_id(fallback)
    .ok_or_else(|| anyhow::anyhow!("book collection path is not valid UTF-8"))
```

Use the corresponding `comparable_parent` value and existing detailed error in BookStore:

```rust
let fallback = comparable_parent
    .file_name()
    .map(Path::new)
    .unwrap_or_else(|| Path::new(""));
path_to_collection_id(fallback).ok_or_else(|| {
    anyhow::anyhow!(
        "book source parent is not valid UTF-8: {}",
        comparable_parent.display()
    )
})
```

Do not change destination construction, validation, staging, publication, or cancellation code.

- [ ] **Step 6: Run collection and ingestion tests**

Run:

```bash
cargo test --no-default-features --features webserver --lib collection_ids_
cargo test --no-default-features --features webserver --lib ingestion_persists_nested_collection_as_portable_identifier
cargo test --no-default-features --features webserver --lib collection_from_nested_book_root_uses_portable_identifier
```

Expected: all focused tests PASS. On Windows, `collection_ids_normalize_native_windows_separators` also runs and passes.

- [ ] **Step 7: Commit collection normalization**

```bash
git add src/domain/algorithm/naming.rs src/domain/algorithm/mod.rs src/domain/services/book_metadata.rs src/services/book_store.rs
git commit -m "fix: normalize book collection identifiers"
```

---

### Task 3: Valid Generated-Thumbnail Test Data

**Files:**
- Modify: `tests/book_api_test.rs:431-452`
- Modify: `tests/book_router_test.rs:124-190`
- Delete: `tests/fixtures/book_dir/.thumbnails/generated-cover.jpg`

**Interfaces:**
- Consumes: `default_book_thumbnail_bytes() -> &'static [u8]` as an embedded valid JPEG byte source.
- Produces: isolated HTTP tests whose `.jpg` bodies are valid JPEG bytes and whose shared fixture tree no longer contains a text file masquerading as JPEG.

- [ ] **Step 1: Make the API thumbnail test own its valid generated JPEG**

Replace `serves_default_and_generated_book_thumbnails` setup with an isolated `TempRoot` and explicit JPEG creation before server startup:

```rust
let temp_root = TempRoot::new("thumbnails", 57204)?;
let book_root = temp_root.0.join("books");
let book_thumbnail_root = book_root.join(".thumbnails");
fs::create_dir_all(&book_thumbnail_root).await?;
let generated_bytes = default_book_thumbnail_bytes();
fs::write(
    book_thumbnail_root.join("generated-cover.jpg"),
    generated_bytes,
)
.await?;
let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
let (server, _) = start_server_with_repository(
    57204,
    repository,
    &book_root,
    &book_thumbnail_root,
)
.await?;
```

For both default and generated responses, assert `CONTENT_TYPE == "image/jpeg"`; assert the generated body equals `generated_bytes` exactly.

- [ ] **Step 2: Make the router security test use valid generated bytes**

In `book_static_routes_enforce_capability_and_file_type_boundaries`, replace:

```rust
let generated_thumbnail = b"valid generated thumbnail";
```

with:

```rust
let generated_thumbnail = default_book_thumbnail_bytes();
```

Keep the existing exact body and `image/jpeg` content-type assertions.

- [ ] **Step 3: Delete the misleading tracked fixture and run the integration tests**

Delete `tests/fixtures/book_dir/.thumbnails/generated-cover.jpg`, then run:

```bash
env NO_PROXY='*' cargo test --no-default-features --features webserver --test book_api_test serves_default_and_generated_book_thumbnails -- --exact
env NO_PROXY='*' cargo test --no-default-features --features webserver --test book_router_test book_static_routes_enforce_capability_and_file_type_boundaries -- --exact
```

Expected: both tests PASS; `git ls-files tests/fixtures/book_dir/.thumbnails/generated-cover.jpg` prints nothing.

- [ ] **Step 4: Commit the fixture correction**

```bash
git add tests/book_api_test.rs tests/book_router_test.rs tests/fixtures/book_dir/.thumbnails/generated-cover.jpg
git commit -m "test: serve valid generated book thumbnails"
```

---

### Task 4: Remove Tracked Scratch Ignore Rules

**Files:**
- Modify: `.gitignore:20-21`
- Local-only: Git common-directory `info/exclude` reported by `git rev-parse --git-common-dir`

**Interfaces:**
- Produces: no net Task 10 `.gitignore` diff against `origin/spec/ebook-support`; local `.worktrees/` and `.superpowers/` scratch paths remain ignored only in this checkout.

- [ ] **Step 1: Add checkout-local exclusions**

Resolve the common Git directory with `git rev-parse --git-common-dir`. In its `info/exclude`, add these lines if absent:

```gitignore
/.worktrees/
/.superpowers/
```

This file is Git metadata and must not be staged.

- [ ] **Step 2: Remove the two tracked rules**

Delete these lines from `.gitignore`:

```gitignore
/.worktrees/
/.superpowers/
```

- [ ] **Step 3: Verify the tracked diff is gone and commit**

Run: `git diff origin/spec/ebook-support -- .gitignore`

Expected: no output.

```bash
git add .gitignore
git commit -m "chore: keep worktree ignores local"
```

---

### Task 5: Full Verification, Review, and Publication

**Files:**
- Verify only: all files changed by Tasks 1-4
- Update through GitHub: existing PR for `codex/task-10-ebook-support`, linked to issue `#45`

**Interfaces:**
- Consumes: all prior task commits and the merged `origin/spec/ebook-support` baseline.
- Produces: a formatted, tested, reviewed, pushed branch and updated ready-for-review PR.

- [ ] **Step 1: Format and inspect the complete diff**

Run:

```bash
cargo fmt --all -- --check
git diff --check origin/spec/ebook-support..HEAD
git diff --stat origin/spec/ebook-support..HEAD
git status --short
```

Expected: formatting and whitespace checks exit 0; status contains no unintended files; `.gitignore` and the deleted text JPEG have no positive additions in the PR diff.

- [ ] **Step 2: Run the full suite from a fresh command**

Run: `env NO_PROXY='*' make test`

Expected: 0 failures in the library and every `tests/*_test.rs` integration target. The pre-existing `DEFAULT_MIGRATIONS_DIR` dead-code warning may remain.

- [ ] **Step 3: Request and receive independent code review**

Use `superpowers:requesting-code-review` to dispatch a reviewer against `origin/spec/ebook-support..HEAD`. Apply `superpowers:receiving-code-review` to every finding: verify it against the code and requirements, implement valid Important/Critical corrections with focused tests, and rerun affected verification. Repeat review until no unresolved Critical or Important issue remains.

- [ ] **Step 4: Confirm commit and issue/PR state**

Run:

```bash
git log --oneline origin/codex/task-10-ebook-support..HEAD
git status --short --branch
gh pr view 58 --json url,state,isDraft,baseRefName,headRefName,body
```

Expected: intentional commits only; clean branch; PR base is `spec/ebook-support`, head is `codex/task-10-ebook-support`, and its body links or closes issue `#45`.

- [ ] **Step 5: Push and publish the reviewed result**

Run:

```bash
git push origin codex/task-10-ebook-support
gh pr ready 58
```

Expected: push succeeds and PR 58 is ready for review. If the PR body does not already link issue 45, update it with `Closes #45` before marking ready.
