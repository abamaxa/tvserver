# Book Ingestion Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Task 8 book ingestion operate on one stable staged source identity, bind workers to manager lifetime, clean transient thumbnails, and exercise EXDEV publication end to end.

**Architecture:** Extend the low-level file store with explicit stage/publish operations, then make book ingestion read only the staged path through a bounded verification loop. Own metadata workers in the manager event loop and use an RAII path reservation. Keep thumbnail and EXDEV seams private and narrowly testable.

**Tech Stack:** Rust 2021, Tokio `JoinSet`, cap-std directory capabilities, async-trait, existing PDF/EPUB extractors and SQL repository.

## Global Constraints

- Retry changing staged sources three times with a 500 ms interval, then restore and return an explicit error.
- Never follow a source symlink or weaken rooted destination protections.
- Preserve the original filename for title fallback, collection, and final destination.
- Preserve legacy `FileStore::rename` behavior and the shared metadata semaphore.
- Never delete `default-book.jpg` or a thumbnail that predated the ingestion attempt.

---

### Task 1: Stable staged source and EXDEV seam

**Files:**
- Modify: `src/domain/traits.rs`
- Modify: `src/adaptors/object_store.rs`

**Interfaces:**
- Produces: `StagedFile { original_path: PathBuf, staged_path: PathBuf }`
- Produces: `FileStore::stage_no_follow(&self, source: &str) -> Result<StagedFile>`
- Produces: `FileStore::publish_staged_no_replace(&self, staged: &StagedFile, destination: &str) -> Result<()>`

- [ ] Add public-operation tests showing staged regular-file identity, symlink rejection, and injected EXDEV success/collision/temp cleanup.
- [ ] Run the focused tests and record failures caused by missing staging and EXDEV injection contracts.
- [ ] Implement capability-scoped stage, restore-on-prepublication-error, and publish-at-commit-point behavior; make `rename_no_replace` compose the two operations.
- [ ] Run the focused tests and the complete object-store suite.

### Task 2: Stable ingestion retry and thumbnail cleanup

**Files:**
- Modify: `src/domain/services/book_metadata.rs`

**Interfaces:**
- Consumes: `stage_no_follow` and `publish_staged_no_replace`
- Produces: three-attempt staged fingerprint/checksum verification loop
- Produces: ingestion-scoped generated-thumbnail guard

- [ ] Add adversarial tests for same-size path replacement, source-to-symlink replacement, one transient size change followed by success, and checksum/metadata/final-byte agreement.
- [ ] Add move/save failure tests proving newly generated covers are removed while default and pre-existing thumbnails remain.
- [ ] Run the focused tests and record the expected identity/retry/cleanup failures.
- [ ] Stage before reading, process only the staged path, retry mutations explicitly, restore on terminal failure, and disarm thumbnail cleanup only after repository save.
- [ ] Run focused tests and the complete book-metadata module suite.

### Task 3: Manager-owned workers and RAII duplicate reservations

**Files:**
- Modify: `src/services/video_information.rs`

**Interfaces:**
- Produces: manager-owned `JoinSet<()>`
- Produces: synchronous `ProcessingPathGuard` removed on `Drop`

- [ ] Add tests proving abort prevents a blocked processor from later persisting and a processor panic releases the path for a later event without killing the manager loop.
- [ ] Run lifecycle tests and record detached-worker/stuck-reservation failures.
- [ ] Spawn workers through the manager `JoinSet`, reap join errors in the event loop, shut workers down on receiver close, and move path cleanup into the RAII guard.
- [ ] Run lifecycle and existing routing/duplicate-suppression tests.

### Task 4: Verification and evidence

**Files:**
- Modify: `.superpowers/sdd/task-8-report.md`

- [ ] Run `cargo test --no-default-features --features webserver adaptors::object_store::tests:: -- --nocapture`.
- [ ] Run `cargo test --no-default-features --features webserver domain::services::book_metadata::tests:: -- --nocapture`.
- [ ] Run `cargo test --no-default-features --features webserver services::video_information::tests:: -- --nocapture`.
- [ ] Run `cargo test --no-default-features --features webserver domain::algorithm::video_utils::tests::test_skip_file -- --exact --nocapture`.
- [ ] Append exact RED/GREEN and covering results to the Task 8 report, run `git diff --check`, and commit the coherent fix wave.
