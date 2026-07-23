# Ebook Review Fixes Implementation Plan

**Goal:** Fix the three actionable review findings on `spec/ebook-support`
without changing public ebook APIs.

**Architecture:** Use the repository's existing path-conditional delete as the
concurrency boundary, derive collection parents from the final domain separator,
and compile HTTP integration infrastructure only with the `webserver` feature.

**Tech Stack:** Rust 2021, Tokio, SQLx/SQLite, Axum integration tests.

### Task 1: Return the immediate collection parent

**Files:**
- Modify: `src/domain/models/book.rs`

- [ ] Add a regression for a three-level collection and run it to observe the
  current root-parent result.
- [ ] Replace the first-separator lookup with a final-separator split.
- [ ] Re-run the focused model tests.

### Task 2: Protect deletion from concurrent relocation

**Files:**
- Modify: `src/services/book_store.rs`

- [ ] Add a repository test double that changes the stored path at the delete
  boundary and a regression asserting the newer row survives.
- [ ] Run the regression and observe the unconditional checksum delete fail it.
- [ ] Use `delete_book_if_path_matches` and restore staged artifacts when no row
  matches or the repository returns an error.
- [ ] Re-run focused deletion and book-store tests.

### Task 3: Restore default-feature test compilation

**Files:**
- Modify: `tests/common/mod.rs`
- Modify: webserver-only integration test crates under `tests/`

- [ ] Run `cargo test --all-targets --no-run` and retain the unresolved-import
  failure as the regression signal.
- [ ] Gate the server helper and every integration crate that consumes it behind
  `feature = "webserver"`.
- [ ] Re-run the default-feature all-target compile.
- [ ] Run focused ebook API, router, and OpenAPI tests with `webserver` enabled.

### Task 4: Final verification

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run focused unit and integration suites from a clean command invocation.
- [ ] Inspect `git diff --check` and the final diff for unintended changes.
