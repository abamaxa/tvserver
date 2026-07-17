# Ebook Merge Blockers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the five verified ebook-support merge blockers without changing public ebook APIs or the persisted checksum type.

**Architecture:** Separate legacy video path naming from portable book identifiers, centralize the optional book-root default in configuration, correct snapshot-aware PDF fallback handling, verify complete file contents before destructive deduplication, and isolate invalid book scan subtrees. Every production change follows a focused red-green test cycle.

**Tech Stack:** Rust 2021, Tokio, Axum, cap-std, SQLx/SQLite, Cargo tests.

## Global Constraints

- `BOOK_DIR`, when unset, resolves to lowercase `books` beside `MOVIE_DIR`.
- Existing video collection strings retain the behavior from `main`.
- Book collection identifiers remain portable `/`-separated identifiers.
- A checksum collision must never delete or overwrite either user file.
- CORS/authentication, automatic books-from-`MOVIE_DIR` routing, and the `i64` checksum schema are out of scope.

---

### Task 1: Preserve Legacy Video Collection Names

**Files:**
- Modify: `src/domain/algorithm/naming.rs:100-180`

**Interfaces:**
- Consumes: `get_movie_dir() -> String` and native `std::path::Path` formatting.
- Produces: unchanged public `get_collection_from_path` and `get_collection_and_video_from_path` behavior for videos; strict book helpers continue using `path_to_collection_id`.

- [ ] **Step 1: Write failing video regressions**

Add unit tests that call root-parameterized video helpers with
`/library/Mission: Impossible/movie.mkv` and, on Unix,
`/library/Manuals\Extras/movie.mkv`. Assert the returned collection is the raw
relative parent rather than `""`.

- [ ] **Step 2: Verify the regressions fail**

Run:
`cargo test domain::algorithm::naming::test::video_collection_helpers_preserve_legacy_nonportable_names -- --exact`

Expected: FAIL because the current portable conversion returns an empty collection.

- [ ] **Step 3: Restore video-only native derivation**

Add private root-aware helpers equivalent to the `main` implementation:

```rust
fn get_video_collection_from_rooted_path(path: &Path, root: impl AsRef<Path>) -> String {
    let short_path = path.strip_prefix(root.as_ref()).unwrap_or(path);
    let collection = if path.is_dir() {
        short_path
    } else {
        short_path.parent().unwrap_or_else(|| Path::new(""))
    };
    collection.to_str().unwrap_or_default().to_string()
}
```

Add the corresponding `(collection, file_name)` helper and route only the two
video public functions through them. Leave book helpers on strict portable
conversion.

- [ ] **Step 4: Verify focused naming tests pass**

Run: `cargo test domain::algorithm::naming::test`

Expected: PASS.

- [ ] **Step 5: Commit the video naming fix**

Run: `git add src/domain/algorithm/naming.rs && git commit -m "fix: preserve legacy video collection names"`

### Task 2: Default `BOOK_DIR` Beside `MOVIE_DIR`

**Files:**
- Modify: `src/domain/config.rs:28-36,136-185`
- Modify: `src/entrypoints/webserver.rs:8-20,72-76`
- Modify: `tests/book_router_test.rs:100-116`
- Modify: `README.md:45-64`
- Modify: `env.sample:5-8`

**Interfaces:**
- Consumes: required `MOVIE_DIR` and optional `BOOK_DIR`.
- Produces: `get_book_dir() -> String`, returning the override or `<MOVIE_DIR parent>/books`; `build_http_router` uses this same helper.

- [ ] **Step 1: Replace required-config expectations with failing default tests**

In `config.rs`, preserve and restore `MOVIE_DIR` as well as book variables, unset
`BOOK_DIR`, set `MOVIE_DIR=/library/movies`, and assert
`get_book_dir() == "/library/books"`. Keep assertions for explicit override and
thumbnail defaults.

In `book_router_test.rs`, rename `builder_requires_book_dir` to
`builder_defaults_book_dir_beside_movie_dir`, create a temporary `movies` and
expected sibling `books` path, unset `BOOK_DIR`, build the router, and assert the
default cover is created beneath `books/.thumbnails`.

- [ ] **Step 2: Verify default tests fail**

Run:
`cargo test domain::config::tests::book_dir_defaults_beside_movie_dir_and_thumbnail_dir_defaults_under_book_dir -- --exact`

Run:
`cargo test --features webserver --test book_router_test builder_defaults_book_dir_beside_movie_dir -- --exact`

Expected: the unit test panics and the router test returns the current missing-variable error.

- [ ] **Step 3: Implement the shared default**

Implement:

```rust
pub fn get_book_dir() -> String {
    env::var(BOOK_DIR).unwrap_or_else(|_| {
        let movie_dir = PathBuf::from(get_movie_dir());
        movie_dir
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .join("books")
            .to_string_lossy()
            .into_owned()
    })
}
```

Replace the webserver's direct `env::var(BOOK_DIR)` lookup with `get_book_dir()`.
Remove now-unused imports. Mark `BOOK_DIR` optional in README and `env.sample`,
documenting the lowercase sibling default.

- [ ] **Step 4: Verify configuration and router tests pass**

Re-run both commands from Step 2.

Expected: PASS.

- [ ] **Step 5: Commit the book-directory default**

Run: `git add src/domain/config.rs src/entrypoints/webserver.rs tests/book_router_test.rs README.md env.sample && git commit -m "fix: default book directory beside movies"`

### Task 3: Correct Snapshot-Derived PDF Titles

**Files:**
- Modify: `src/domain/services/book_metadata.rs:510-650, test module`

**Interfaces:**
- Consumes: the extraction path captured before `spawn_blocking` and `filename_derived_title(&Path)`.
- Produces: original source filename fallback title in saved `BookDetails`.

- [ ] **Step 1: Write a failing ingestion regression**

Create a metadata-free PDF named `the_hidden_library.pdf`, ingest it through
`generate_book_metadata_with_roots`, and assert both returned and persisted titles
equal `"the hidden library"` and contain no `snapshot` prefix.

- [ ] **Step 2: Verify the PDF regression fails**

Run:
`cargo test domain::services::book_metadata::tests::ingestion_metadata_free_pdf_uses_original_filename_title -- --exact`

Expected: FAIL with a `.snapshot-...`-derived title.

- [ ] **Step 3: Compare against the extraction path**

Clone the snapshot path before moving it into the blocking extraction closure and
change the fallback check to:

```rust
if format == BookFormat::Pdf
    && extraction.title.as_deref()
        == Some(filename_derived_title(&extraction_path_for_fixup).as_str())
{
    extraction.title = Some(filename_derived_title(&path));
}
```

- [ ] **Step 4: Verify PDF metadata tests pass**

Run the exact regression and then:
`cargo test domain::services::book_metadata::tests::pdf_`

Expected: PASS.

- [ ] **Step 5: Commit the PDF title fix**

Run: `git add src/domain/services/book_metadata.rs && git commit -m "fix: restore source filename for PDF fallback titles"`

### Task 4: Refuse Destructive Deduplication on Checksum Collisions

**Files:**
- Modify: `src/domain/traits.rs:165-205`
- Modify: `src/adaptors/object_store.rs:450-480,1034-1460`
- Modify: `src/domain/services/book_metadata.rs:452-505, test module`

**Interfaces:**
- Produces on `FileStore`:
  `async fn private_snapshot_matches_regular_no_follow(&self, snapshot: &PrivateSnapshot, path: &Path) -> anyhow::Result<bool>`.
- Consumes: retained private-snapshot authority, retained book-root capability, and full regular-file byte streams.

- [ ] **Step 1: Write the failing collision regression**

Create two 12 MiB `.pdf` files with identical first 11 MiB and different final
bytes. Seed the canonical file and database row using the shared prefix checksum,
then ingest the second file. Assert ingestion returns an error containing
`"checksum collision"`, the incoming source is restored byte-for-byte, and the
canonical file and row remain unchanged.

- [ ] **Step 2: Verify the collision regression fails**

Run:
`cargo test domain::services::book_metadata::tests::checksum_collision_restores_source_and_preserves_canonical_book -- --exact`

Expected: FAIL because current duplicate cleanup deletes the incoming source and returns the existing row.

- [ ] **Step 3: Add capability-safe complete comparison**

Add the `FileStore` method with an error-returning default for non-filesystem
implementations. In `FileSystemStore`, open the snapshot from its retained
authority and verify its device/inode. Walk every canonical parent with
`open_dir_nofollow`, open the final component with `FollowSymlinks::No`, verify it
is the same regular-file identity observed before opening, then compare both
readers in fixed-size buffers until EOF. Return `true` only when length and every
byte match.

- [ ] **Step 4: Gate duplicate cleanup on complete equality**

When the canonical path exists, call the new comparison. For `true`, retain the
existing healthy-duplicate cleanup. For `false`, call `cleanup_prepublication`
with `CleanupMode::Restore`, disarm the guard, and return a `checksum collision`
error. Comparison errors also restore the source before returning.

- [ ] **Step 5: Verify collision and identical-duplicate behavior**

Run the exact collision regression, then:
`cargo test domain::services::book_metadata::tests::identical_second_ingestion_keeps_first_file_and_row_canonical -- --exact`

Expected: both PASS.

- [ ] **Step 6: Verify low-level no-follow comparison tests**

Add adaptor tests for equal files, a late-byte difference, and a canonical
symlink. Run:
`cargo test adaptors::object_store::tests::private_snapshot_comparison`

Expected: equal returns true, late difference returns false, and symlink is rejected.

- [ ] **Step 7: Commit collision-safe deduplication**

Run: `git add src/domain/traits.rs src/adaptors/object_store.rs src/domain/services/book_metadata.rs && git commit -m "fix: verify complete books before deduplication"`

### Task 5: Skip Invalid Book Subtrees Without Halting the Scan

**Files:**
- Modify: `src/domain/services/book_check.rs:142-165, test module`

**Interfaces:**
- Consumes: `path_to_collection_id(&Path) -> Option<String>`.
- Produces: complete scans of valid subtrees even when a sibling name is nonportable.

- [ ] **Step 1: Convert the existing failure test into a failing resilience test**

Rename the invalid-collection test to
`invalid_collection_is_skipped_without_blocking_valid_sibling`. Keep
`a-good/new.pdf` and `z:invalid`, remove the orphan fixture, assert the scan
succeeds, and assert the receiver gets a `MediaEvent` for `a-good/new.pdf`.

- [ ] **Step 2: Verify the resilience test fails**

Run:
`cargo test domain::services::book_check::tests::invalid_collection_is_skipped_without_blocking_valid_sibling -- --exact`

Expected: FAIL because the invalid subtree currently aborts the scan.

- [ ] **Step 3: Validate each child before recursion**

Build `child_collection = relative_collection.join(&directory)`. If
`path_to_collection_id(&child_collection).is_none()`, log a warning containing the
book-root-relative path and continue. Otherwise recurse with that child.

- [ ] **Step 4: Verify focused scanner tests pass**

Run the exact regression and then:
`cargo test domain::services::book_check::tests`

Expected: PASS.

- [ ] **Step 5: Commit resilient book scanning**

Run: `git add src/domain/services/book_check.rs && git commit -m "fix: isolate invalid book scan subtrees"`

### Task 6: Final Verification

**Files:**
- Verify all modified files.

- [ ] **Step 1: Format and inspect whitespace**

Run: `cargo fmt --all -- --check`

Run: `git diff --check`

Expected: both exit 0.

- [ ] **Step 2: Run the affected library suites**

Run: `cargo test domain::algorithm::naming::test`

Run: `cargo test domain::config::tests`

Run: `cargo test domain::services::book_check::tests`

Run: `cargo test domain::services::book_metadata::tests`

Run: `cargo test adaptors::object_store::tests`

Expected: PASS.

- [ ] **Step 3: Verify default and webserver configurations**

Run: `cargo test --all-targets --no-run`

Run: `cargo test --features webserver --test book_router_test`

Expected: both exit 0.

- [ ] **Step 4: Run the complete library suite**

Run: `cargo test --lib`

Expected: PASS.

- [ ] **Step 5: Review scope**

Run: `git status --short` and `git diff --stat HEAD~1..`

Confirm only the approved specification, plan, implementation, tests, and
configuration documentation changed.
