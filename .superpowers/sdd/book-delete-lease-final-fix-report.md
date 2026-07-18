# Book Delete Lease Final Fix Report

## Scope

Applied the approved final-review corrections on `spec/ebook-support` only:

- hardened the deletion/reconciliation race regression harness;
- made the orphan-reconciliation skip log operationally accurate; and
- amended the committed implementation plan so its Task 2 instructions match
  the corrected implementation.

## Files Changed

- `src/services/book_store.rs`
  - Gives the pausing `RelocatingDeleteRepository` only to `BookStore`.
  - Gives `BookCheck` the underlying SQLite `inner.clone()` repository.
  - Records the row state while deletion is paused, then always releases and
    awaits deletion before asserting the reconciliation and deletion outcomes.
- `src/domain/services/book_check.rs`
  - Changes the skip log to: `Skipping orphan reconciliation because another
    operation owns the book path`.
- `docs/superpowers/plans/2026-07-18-book-delete-lease-checksum-decision.md`
  - Updates Task 2's exact regression snippet and cleanup instructions.
  - Documents the corrected operational log wording and why it must not claim
    ingestion specifically.

## Verification

| Command | Result |
| --- | --- |
| `cargo test --lib services::book_store::tests::delete_lease_prevents_orphan_reconciliation_from_restoring_the_book -- --exact` | PASS — 1 passed, 0 failed, 277 filtered out. |
| `cargo test --lib services::book_store::tests -- --nocapture` | PASS — 26 passed, 0 failed, 252 filtered out. |
| `cargo test --lib domain::services::book_check::tests -- --nocapture` | PASS — 15 passed, 0 failed, 263 filtered out. |
| `git diff --check` | PASS — no whitespace errors. |

An additional `cargo fmt --check` was attempted. It exits nonzero because of
extensive pre-existing formatting drift across unrelated repository files and
does not modify files; it is not a required verification command for this
review wave.

## Self-Review

- The checker still uses the actual `BookCheck`, filesystem store, SQLite
  repository, and shared lease coordinator.
- A deletion-lease regression now allows the checker to complete reconciliation
  against the real underlying repository, making the preserved-row observation
  fail without a test-double deadlock.
- The release permit and deletion task are completed before any assertion,
  including the preserved-row assertion, making a regression path terminate
  predictably.
- The log message correctly covers ingestion, deletion, and any other owner of
  the path lease.
- The plan contains the same corrected wiring, cleanup sequence, and log text
  as the implementation.

## Concerns

- No implementation concerns found.
- Repository-wide `cargo fmt --check` remains noisy because of unrelated
  pre-existing formatting differences; no formatting changes outside this scope
  were made.
