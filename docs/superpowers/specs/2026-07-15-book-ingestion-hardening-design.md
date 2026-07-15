# Book Ingestion Hardening Design

## Scope

Harden Task 8 book ingestion against source-path replacement and worker-lifecycle races, clean transient generated thumbnails, and test the public cross-device no-replace path deterministically. Existing video processing, book filename/collection semantics, and legacy `FileStore::rename` replacement behavior remain unchanged.

## Stable source identity and private snapshot

`FileStore` gains a narrow staged-move contract. `stage_no_follow` capability-validates and atomically renames the incoming path to a unique hidden sibling before ingestion reads any bytes. It validates the staged object again without following symlinks. `publish_staged_no_replace` publishes that exact staged regular file to an absent rooted destination, restoring it to the original path on all pre-publication errors. Once the destination is published, cleanup is warning-only and the move reports success.

Book ingestion retains the original UTF-8 filename separately for title fallback, collection selection, and final destination. The initial stability checks use the staged path, but the downloader-owned inode is never used for checksum, extraction, or publication.

Each stability attempt snapshots no-follow file identity, length, and modification time, waits 500 ms, and compares again. A stable candidate is copied through a no-follow opened source handle into a `create_new` file in a private server-controlled directory beneath `BOOK_DIR`. The staged fingerprint is checked immediately before and after the copy. A changed or replaced staged source causes the private snapshot to be removed and the attempt retried; a symlink is rejected without reading its target.

From successful snapshot creation onward, checksum, PDF/EPUB extraction, thumbnail keying, and final no-replace publication use only that new snapshot inode. Changes through a downloader-retained descriptor or later staged-path replacement cannot affect extracted metadata or final bytes. Pre-publication errors and cancellation remove the private snapshot, remove only an owned generated thumbnail, and restore the original staged source through awaited `FileStore` operations. Publication is the commit point: once it begins, cancellation is deferred until repository save and source cleanup complete. Three failed stability/copy attempts produce a clear terminal error and restore the staged source.

## Worker lifecycle

`MetaDataManager` owns metadata workers in a `JoinSet`; it does not detach workers. A manager-owned `CancellationToken` is cloned into every worker and production book ingestion. Explicit shutdown signals the token, stops accepting events, and drains the `JoinSet` by awaiting every worker. Blocking stages, extraction, and publication are allowed to finish; the async worker then regains control and performs phase-appropriate awaited cleanup before shutdown returns. Worker panics are logged while the manager continues draining.

`MetaDataManagerHandle::shutdown` is the production completion boundary and consumes the handle so shutdown can be awaited. `TVServer::shutdown` is likewise async and consuming, and the webserver awaits it. The handle remains a `Future` for dbtool. Dropping a handle only signals best-effort cancellation; it does not hard-abort workers across `spawn_blocking`.

Path duplicate suppression uses a synchronous RAII reservation. Insertion occurs before spawning; the guard is owned by the worker and removes the path during success, error, panic, or cancellation unwinding. The existing process-wide semaphore remains unchanged.

## Thumbnail ownership and cleanup

After snapshot checksum calculation, ingestion acquires a process-wide weak mutex keyed by the checksum thumbnail path. Lock ordering is destination first, thumbnail second. The thumbnail lease is held across the preexistence check, blocking extraction, publication, repository save, and cleanup/disarm. This prevents two equal-content ingestions from disagreeing about ownership or deleting the successful ingestion's cover.

Cleanup tracks only a non-default thumbnail that did not exist before extraction. Pre-publication failure and cancellation delete it through an awaited capability-backed `FileStore` operation while the checksum lease remains held. The default thumbnail and a pre-existing checksum thumbnail are never targets. Drop guards are limited to nonblocking bookkeeping/best-effort diagnostics and are not the primary restoration or deletion mechanism.

## Deterministic EXDEV testing

`FileSystemStore` uses a private filesystem-operation boundary for the staged-source hard-link publication attempt. Production delegates directly to `Dir::hard_link`; tests inject one `CrossesDevices` result. This forces public `rename_no_replace` through the real copy/publish branch without a second mount. End-to-end tests cover successful publication, collision restoration, and temporary-file cleanup.

## Verification

Tests must demonstrate private-snapshot inode independence, mutation/replacement during copy or extraction, mutation after final verification, symlink rejection without target reads, terminal retry restoration and snapshot cleanup, checksum/metadata/final-byte agreement, awaited manager shutdown at blocking stage/extraction/publication boundaries, no persistence after pre-publication cancellation, publication-phase save completion, panic reservation cleanup, concurrent equal-checksum thumbnail ownership, default/pre-existing thumbnail preservation, and deterministic EXDEV success/collision behavior. Covering suites are object-store, book-metadata, metadata routing/lifecycle, and `skip_file`.
