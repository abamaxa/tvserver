# Book Ingestion Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure book ingestion publishes exactly the bytes validated after extraction and keeps the first healthy file canonical when duplicate content is downloaded again.

**Architecture:** Add a full-file SHA-256 `FileSeal` shared by the domain service and filesystem adaptor. The adaptor publishes snapshots by copying into a fresh temporary inode, verifying that copy against the expected seal, and atomically linking it into place without replacement. Reuse the checksum-keyed thumbnail lease to serialize duplicate reconciliation; a healthy existing checksum row wins, while a row whose file is missing may be repaired by the new ingestion.

**Tech Stack:** Rust 2021, Tokio, `async-trait`, `cap-std`, `sha2`, SQLx/SQLite, existing unit and integration test harnesses.

## Global Constraints

- Keep the existing database checksum algorithm and schema unchanged.
- Keep public REST, Tauri, video-ingestion, and download-event interfaces unchanged.
- Never replace an existing destination path.
- Never publish bytes that differ from the post-extraction full-content seal.
- Keep the first healthy checksum match canonical; only repair its location when its file is missing.
- Preserve staged-source restoration, thumbnail cleanup, cancellation commit boundaries, and no-symlink filesystem checks.

---

## File Map

- `Cargo.toml`, `Cargo.lock`: declare the direct `sha2` dependency used for full-content seals.
- `src/domain/algorithm/file_integrity.rs`: own `FileSeal` and streaming full-file seal calculation.
- `src/domain/algorithm/mod.rs`: export the seal API.
- `src/domain/traits.rs`: extend `FileStore` with snapshot sealing, verified publication, and safe canonical-file existence checks.
- `src/adaptors/object_store.rs`: implement capability-anchored sealing, verified copy/no-replace publication, and regular-file checks.
- `src/domain/services/book_metadata.rs`: consume the seal during ingestion and reconcile checksum duplicates under the existing checksum-keyed lease.

---

### Task 1: Bind Snapshot Validation to Published Bytes

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Create: `src/domain/algorithm/file_integrity.rs`
- Modify: `src/domain/algorithm/mod.rs`
- Modify: `src/domain/traits.rs:116-164`
- Modify: `src/adaptors/object_store.rs:310-409,656-795`
- Modify: `src/domain/services/book_metadata.rs:392-533,603-605,2960-3460`

**Interfaces:**
- Produces: `FileSeal { len: u64, sha256: [u8; 32] }` and `seal_reader<R: Read>(&mut R) -> io::Result<FileSeal>`.
- Produces: `FileStore::seal_private_snapshot(&PrivateSnapshot) -> anyhow::Result<FileSeal>`.
- Changes: `FileStore::publish_private_snapshot_no_replace(&PrivateSnapshot, &str, &FileSeal) -> anyhow::Result<()>`.
- Produces: `FileStore::regular_file_exists_no_follow(&Path) -> anyhow::Result<bool>` for Task 2.

- [ ] **Step 1: Add failing full-content seal tests**

Create `src/domain/algorithm/file_integrity.rs` with test-only expectations before the implementation:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_changes_when_same_length_content_changes() {
        let first = seal_reader(&mut &b"first"[..]).unwrap();
        let second = seal_reader(&mut &b"other"[..]).unwrap();
        assert_eq!(first.len, second.len);
        assert_ne!(first.sha256, second.sha256);
    }

    #[test]
    fn seal_covers_bytes_after_database_checksum_window() {
        let mut first = vec![0_u8; 12 * 1024 * 1024];
        let mut second = first.clone();
        second[11 * 1024 * 1024] = 1;
        assert_ne!(
            seal_reader(&mut first.as_slice()).unwrap(),
            seal_reader(&mut second.as_slice()).unwrap()
        );
    }
}
```

- [ ] **Step 2: Run the seal tests and verify RED**

Run: `cargo test --lib domain::algorithm::file_integrity::tests -- --nocapture`

Expected: compilation fails because `FileSeal` and `seal_reader` are not implemented.

- [ ] **Step 3: Implement the streaming seal primitive**

Add `sha2 = "0.10"` to `[dependencies]`, export `file_integrity` from `src/domain/algorithm/mod.rs`, and implement:

```rust
use sha2::{Digest, Sha256};
use std::io::{self, Read};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSeal {
    pub len: u64,
    pub sha256: [u8; 32],
}

pub fn seal_reader<R: Read>(reader: &mut R) -> io::Result<FileSeal> {
    let mut hasher = Sha256::new();
    let mut len = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        len += read as u64;
    }
    Ok(FileSeal {
        len,
        sha256: hasher.finalize().into(),
    })
}
```

- [ ] **Step 4: Run the seal tests and verify GREEN**

Run: `cargo test --lib domain::algorithm::file_integrity::tests -- --nocapture`

Expected: both seal tests pass.

- [ ] **Step 5: Add failing object-store publication regressions**

In `src/adaptors/object_store.rs`, add tests using a real `FileSystemStore`:

```rust
#[tokio::test]
async fn verified_snapshot_publication_rejects_post_seal_mutation() {
    // Stage and snapshot an EPUB, seal the snapshot, overwrite the snapshot
    // with same-length bytes, then attempt verified publication.
    // Assert the call returns an integrity error, destination is absent,
    // and the snapshot remains available for caller cleanup/restoration.
}

#[cfg(unix)]
#[tokio::test]
async fn verified_snapshot_publication_uses_a_distinct_destination_inode() {
    // Seal and publish an unchanged snapshot.
    // Assert destination bytes match, the snapshot path is removed, and the
    // destination inode differs from the snapshot inode captured before publish.
}
```

- [ ] **Step 6: Run the object-store regressions and verify RED**

Run: `cargo test --lib adaptors::object_store::tests::verified_snapshot_publication -- --nocapture`

Expected: compilation fails because the `FileStore` seal and verified-publication interfaces do not exist.

- [ ] **Step 7: Extend the `FileStore` contract and verified-copy implementation**

In `src/domain/traits.rs`, add:

```rust
async fn seal_private_snapshot(
    &self,
    snapshot: &PrivateSnapshot,
) -> anyhow::Result<FileSeal>;
async fn publish_private_snapshot_no_replace(
    &self,
    snapshot: &PrivateSnapshot,
    destination: &str,
    expected_seal: &FileSeal,
) -> anyhow::Result<()>;
async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool>;
```

In `FileSystemStore`:

- Resolve the snapshot only inside `.tvserver-book-snapshots` and open it with `FollowSymlinks::No`.
- Implement `seal_private_snapshot` by passing that retained file to `seal_reader`.
- Replace hard-link publication with `copy_private_snapshot_no_replace_verified`: create a unique temporary file beside the destination, copy while hashing, sync it, compare the actual seal with `expected_seal`, hard-link the temporary inode to the final no-replace destination, and remove both temporary and snapshot paths after the final link succeeds.
- On a seal mismatch or any pre-publication failure, remove only the temporary file and leave the snapshot for the service's cleanup path.
- Implement `regular_file_exists_no_follow` through the retained root capability. Return `false` for `NotFound`; reject symlinks and non-regular paths instead of treating them as healthy canonical files.
- Update every `FileStore` test double in `book_metadata.rs` to forward the new methods and pass `expected_seal` through publication wrappers.

- [ ] **Step 8: Run the object-store and existing hardening tests and verify GREEN**

Run: `cargo test --lib adaptors::object_store::tests -- --nocapture`

Expected: all object-store tests pass, including mutation rejection and distinct-inode publication.

- [ ] **Step 9: Add the failing service-level post-seal mutation regression**

Add a `PostSealMutationStore` in `book_metadata.rs` that delegates sealing, mutates the snapshot immediately afterward, and delegates publication. Add:

```rust
#[tokio::test]
async fn ingestion_rejects_snapshot_mutated_after_post_extraction_seal() {
    // Ingest through PostSealMutationStore.
    // Assert an integrity error, no destination row or file, restored source,
    // cleaned snapshot, and no owned thumbnail leak.
}
```

- [ ] **Step 10: Run the service regression and verify RED**

Run: `cargo test --lib domain::services::book_metadata::tests::ingestion_rejects_snapshot_mutated_after_post_extraction_seal -- --nocapture`

Expected: the test fails because ingestion does not yet seal and pass an expected seal to publication.

- [ ] **Step 11: Wire the seal into book ingestion**

During post-extraction verification, call `storer.seal_private_snapshot(&snapshot)` between the existing before/after fingerprints, keep the existing database-checksum equality check, retain the accepted `FileSeal` with `BookDetails`, and publish with:

```rust
storer
    .publish_private_snapshot_no_replace(&snapshot, destination_path, &snapshot_seal)
    .await
```

Treat seal calculation failure exactly like other pre-publication integrity failures: remove snapshot/owned thumbnail, restore the staged source, and do not save a row.

- [ ] **Step 12: Run the focused ingestion integrity tests and verify GREEN**

Run: `cargo test --lib domain::services::book_metadata::tests::ingestion_rejects_snapshot_mutated_after_post_extraction_seal -- --nocapture`

Then run: `cargo test --lib domain::services::book_metadata::tests -- --nocapture`

Expected: the new regression and all existing book metadata tests pass.

- [ ] **Step 13: Commit Task 1**

```bash
git add Cargo.toml Cargo.lock src/domain/algorithm/file_integrity.rs src/domain/algorithm/mod.rs src/domain/traits.rs src/adaptors/object_store.rs src/domain/services/book_metadata.rs
git commit -m "Bind book publication to verified snapshot bytes"
```

---

### Task 2: Keep the First Healthy Checksum Destination Canonical

**Files:**
- Modify: `src/domain/services/book_metadata.rs:392-645,3560-4800`

**Interfaces:**
- Consumes: `FileStore::regular_file_exists_no_follow(&Path) -> anyhow::Result<bool>` from Task 1.
- Produces: internal duplicate reconciliation that returns the existing `BookDetails` without publishing or updating the repository when its canonical file is healthy.

- [ ] **Step 1: Add the failing two-ingestion regression**

In `book_metadata.rs`, ingest the same valid EPUB bytes from two different source paths/collections using a real repository and store:

```rust
#[tokio::test]
async fn identical_second_ingestion_keeps_first_file_and_row_canonical() {
    // First ingest: Originals/first.epub.
    // Second ingest: Reprints/second.epub with identical bytes.
    // Assert the returned second result equals the first canonical details,
    // the first destination still exists, the second destination and source
    // do not exist, and retrieve_book(checksum) still points to the first path.
}
```

- [ ] **Step 2: Run the duplicate regression and verify RED**

Run: `cargo test --lib domain::services::book_metadata::tests::identical_second_ingestion_keeps_first_file_and_row_canonical -- --nocapture`

Expected: the second destination exists and the checksum row points to it.

- [ ] **Step 3: Implement first-copy-wins reconciliation**

Immediately after acquiring the checksum-keyed thumbnail lease and before thumbnail creation/extraction:

```rust
match repository.retrieve_book(checksum).await {
    Ok(existing) => {
        validate_collection(&existing.collection)?;
        let existing_file = Path::new(&existing.file_name);
        if existing_file.components().count() != 1
            || !matches!(existing_file.components().next(), Some(std::path::Component::Normal(_)))
        {
            anyhow::bail!("stored book file name is not a safe path component");
        }
        let relative = crate::domain::algorithm::get_book_download_path(
            &existing.collection,
            &existing.file_name,
        );
        if storer
            .regular_file_exists_no_follow(Path::new(&relative))
            .await?
        {
            storer.remove_private_snapshot(&snapshot).await?;
            storer.discard_staged(&staged).await?;
            staged_guard.disarm();
            return Ok(Some(existing));
        }
    }
    Err(sqlx::Error::RowNotFound) => {}
    Err(error) => {
        let cleanup = cleanup_prepublication(
            &storer,
            &staged,
            Some(&snapshot),
            None,
            CleanupMode::Restore,
        )
        .await;
        staged_guard.disarm();
        return Err(with_cleanup_error(error.into(), cleanup));
    }
}
```

Factor the successful duplicate cleanup into a helper that restores the staged source if snapshot removal fails and reports discard failure instead of returning success. Do not create, overwrite, or delete the canonical thumbnail.

- [ ] **Step 4: Run the duplicate regression and verify GREEN**

Run: `cargo test --lib domain::services::book_metadata::tests::identical_second_ingestion_keeps_first_file_and_row_canonical -- --nocapture`

Expected: one canonical file and one unchanged row remain; the second staged source is discarded.

- [ ] **Step 5: Add the stale-row repair regression**

```rust
#[tokio::test]
async fn identical_ingestion_repairs_checksum_row_when_canonical_file_is_missing() {
    // Save a BookDetails row with the incoming checksum and a missing path.
    // Ingest a valid book at a new destination.
    // Assert the new destination exists and the checksum row now points to it.
}
```

- [ ] **Step 6: Run the stale-row test and verify GREEN without broadening policy**

Run: `cargo test --lib domain::services::book_metadata::tests::identical_ingestion_repairs_checksum_row_when_canonical_file_is_missing -- --nocapture`

Expected: pass because a missing canonical file does not trigger duplicate discard.

- [ ] **Step 7: Run all book ingestion and repository regressions**

Run: `cargo test --lib domain::services::book_metadata::tests -- --nocapture`

Then run: `cargo test --lib adaptors::repository::tests -- --nocapture`

Expected: all tests pass; repository checksum-upsert behavior remains available for stale-row repair.

- [ ] **Step 8: Commit Task 2**

```bash
git add src/domain/services/book_metadata.rs
git commit -m "Keep first ingested book canonical"
```

---

### Task 3: Whole-Branch Verification and Review

**Files:**
- Verify: all files changed since `aeaf9d99abcbc14701762ded658d702492bd1cd2`

**Interfaces:**
- Consumes: completed Task 1 and Task 2 commits.
- Produces: verified, reviewed branch ready to push to PR #57.

- [ ] **Step 1: Format and inspect the patch**

Run: `cargo fmt --all -- --check`

Run: `git diff --check origin/spec/ebook-support...HEAD`

Expected: both commands exit 0.

- [ ] **Step 2: Run the complete project test target**

Run: `make test`

Expected: all unit and integration tests pass; ignored renderer tests remain ignored unless their feature is enabled.

- [ ] **Step 3: Run both supported builds**

Run: `make build`

Then run: `cargo build`

Expected: both builds exit 0.

- [ ] **Step 4: Request independent code review**

Package the exact `aeaf9d99abcbc14701762ded658d702492bd1cd2...HEAD` range and dispatch a read-only reviewer. Require explicit checks for the original two findings, cleanup/cancellation regressions, symlink containment, duplicate repair semantics, and unchanged video routing.

- [ ] **Step 5: Address any Critical or Important findings test-first**

For each confirmed finding, add a focused failing regression, run it to confirm RED, implement the minimum fix, rerun to GREEN, and repeat the complete verification commands from Steps 1-3.

- [ ] **Step 6: Commit any review follow-ups**

```bash
git add -u
git commit -m "Address book ingestion review follow-ups"
```

Skip this commit when the reviewer reports no actionable findings and the worktree is clean.

- [ ] **Step 7: Push the feature branch**

Run: `git push origin codex/ebook-support-task-8`

Expected: GitHub reports the branch updated successfully and PR #57 contains the new commits.
