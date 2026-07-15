# Book Ingestion Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Task 8 book ingestion operate on a server-owned immutable snapshot, shut workers down cooperatively and observably, serialize equal-checksum thumbnail ownership, and exercise EXDEV publication end to end.

**Architecture:** Extend the low-level file store with explicit stage/snapshot/publish/cleanup operations. Stability is established on the staged downloader inode, then a no-follow copy creates a new private snapshot beneath `BOOK_DIR`; all checksum, extraction, and publication reads use only that snapshot. Own metadata workers and a cancellation token in the manager, drain workers on explicit shutdown, and use awaited phase-aware cleanup. Serialize thumbnail ownership with the existing weak-lock pattern, after the destination lock.

**Tech Stack:** Rust 2021, Tokio `JoinSet`, cap-std directory capabilities, async-trait, existing PDF/EPUB extractors and SQL repository.

## Global Constraints

- Retry staged-source changes during stability or snapshot copy three times with a 500 ms interval, then remove snapshots, restore, and return an explicit error.
- Never follow a source symlink or weaken rooted destination protections.
- Never checksum, extract, or publish the downloader-owned inode; use only the private snapshot.
- Preserve the original filename for title fallback, collection, and final destination.
- Preserve legacy `FileStore::rename` behavior and the shared metadata semaphore.
- Never delete `default-book.jpg` or a thumbnail that predated the ingestion attempt.
- Explicit manager shutdown must signal cancellation and await all workers; do not hard-abort across blocking work.
- Lock ordering is destination reservation, then checksum-thumbnail lease.

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

---

### Task 5: Server-controlled snapshot and awaited cleanup amendment

**Files:**
- Modify: `src/domain/traits.rs`
- Modify: `src/adaptors/object_store.rs`
- Modify: `src/domain/services/book_metadata.rs`

**Interfaces:**
- Produces: a private snapshot value rooted beneath the file store
- Produces: no-follow staged-to-snapshot copy and capability-backed snapshot/staged cleanup
- Consumes: snapshot for checksum, extraction, thumbnail keying, and publication

- [ ] Add production-boundary regressions for staged mutation/replacement during copy/extraction and after final verification, symlink replacement, terminal restoration, and snapshot cleanup.
- [ ] Run focused tests and record failures showing the current downloader inode remains mutable throughout processing.
- [ ] Implement `create_new` private snapshot copy through a no-follow source handle, with staged fingerprint checks bracketing the copy.
- [ ] Move checksum, extraction, verification, and publication exclusively to the snapshot; make pre-publication restoration/snapshot/thumbnail cleanup awaited.
- [ ] Run focused tests and the object-store and book-metadata suites.

### Task 6: Cooperative manager shutdown amendment

**Files:**
- Modify: `src/services/video_information.rs`
- Modify: `src/entrypoints/tvserver.rs`
- Modify: `src/entrypoints/webserver.rs`

- [ ] Add blocking stage, extraction/cover, and publication shutdown regressions. Assert shutdown waits, pre-publication cancellation restores without saving, publication-phase cancellation completes save, and panic releases reservations.
- [ ] Run lifecycle tests and record failures from hard-aborted `JoinSet` workers.
- [ ] Add a manager-owned `CancellationToken`, stop intake on cancellation, drain workers, and propagate cancellation into production book ingestion.
- [ ] Make handle and TVServer explicit shutdown async/awaited while preserving handle `Future` behavior for dbtool.
- [ ] Run lifecycle/routing tests.

### Task 7: Equal-checksum thumbnail ownership amendment

**Files:**
- Modify: `src/domain/services/book_metadata.rs`

- [ ] Add a concurrent same-checksum regression where one ingestion succeeds and one fails, proving the successful row's cover remains.
- [ ] Run it and record the ownership race failure.
- [ ] Add a process-wide weak thumbnail mutex, acquired after destination reservation and held through extraction, save, and cleanup/disarm.
- [ ] Run the focused regression and the complete book-metadata suite.

### Task 8: Final amended verification and evidence

**Files:**
- Modify: `.superpowers/sdd/task-8-report.md`

- [ ] Run the object-store, book-metadata, lifecycle/routing, and exact `skip_file` suites specified above.
- [ ] Run `git diff --check`, self-review the entire amendment against the approved phase rules, append exact RED/GREEN evidence, and commit one coherent fix wave.
