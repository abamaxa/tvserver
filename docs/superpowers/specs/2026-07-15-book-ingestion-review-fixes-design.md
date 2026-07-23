# Book Ingestion Review Fixes Design

## Context

Task 8 stages a completed PDF or EPUB, creates a private snapshot, extracts metadata from that snapshot, publishes the book into `BOOK_DIR`, and finally saves the book row. Review of PR #57 identified two integrity gaps:

1. The snapshot is verified after extraction but remains a writable inode that is later hard-linked into the final destination. A mutation between verification and publication can make the stored checksum and metadata disagree with the published bytes.
2. Two successful ingestions with identical checksums but different destinations can both publish files. The repository's checksum-conflict upsert then repoints the single row to the second destination and leaves the first file orphaned.

## Goals

- Bind post-extraction validation to the exact bytes atomically published.
- Never expose a destination whose bytes do not match the validated snapshot.
- Keep the first complete, existing checksum match as the canonical book.
- Repair a checksum row whose canonical file is missing by allowing a new ingestion to replace its location.
- Preserve no-replace destination semantics, cancellation cleanup, thumbnail ownership, and video behavior.

## Non-Goals

- Supporting multiple database rows for the same checksum.
- Replacing a healthy canonical book with the newest duplicate.
- Changing the existing database checksum algorithm or schema.
- Defending against a privileged process that can mutate arbitrary tvserver memory or file descriptors.

## Snapshot Seal and Publication

After metadata extraction, ingestion computes a full-content seal for the private snapshot. The seal contains the byte length and a digest over the entire file; it is separate from the existing partial-content database checksum.

Publication receives the expected seal and performs the following work inside the file-store boundary:

1. Open the snapshot without following symlinks.
2. Create a fresh, non-replacing temporary file beside the destination.
3. Copy the snapshot into that temporary file while computing its full-content seal.
4. Flush and sync the temporary file.
5. Compare the copied bytes' seal with the expected post-extraction seal.
6. On mismatch, delete the temporary file and return an integrity error without publishing a destination.
7. On match, atomically rename the temporary file into the destination without replacing an existing path.
8. Remove the private snapshot after successful publication.

The destination therefore receives a distinct inode whose validated bytes cannot be changed through a retained handle to the snapshot. Existing cleanup restores the staged downloader source when publication fails.

## Duplicate-Content Policy

The existing thumbnail lease is keyed by checksum and spans extraction, publication, and repository persistence. While holding that lease, ingestion checks the repository for an existing checksum before generating a new thumbnail or publishing a destination.

- If the row's canonical file exists as a regular, non-symlink file, the first copy remains canonical. Ingestion discards the newly staged source and private snapshot, leaves the database row and thumbnail unchanged, and returns the existing book.
- If the row exists but its canonical file is missing, ingestion continues. The new file is published and the existing checksum-conflict upsert repairs the row to the new destination.
- Repository errors other than "row not found" fail ingestion and run normal pre-publication cleanup. They are not treated as permission to publish a possible duplicate.

The canonical path is derived from the validated stored collection and filename under `BOOK_DIR`; containment and no-symlink checks remain mandatory.

## Cancellation and Failure Semantics

- Cancellation before duplicate reconciliation restores the staged source and removes the snapshot and any newly generated thumbnail.
- Once verified publication begins, the current commit-boundary behavior remains: publication and persistence finish rather than abandoning a visible destination midway.
- Seal mismatch is a pre-publication failure. No destination or database change is allowed, and the original source is restored.
- Duplicate cleanup failure returns an error instead of reporting successful deduplication.

## Testing

Tests will be added before production changes:

1. Mutate the private snapshot after extraction verification but before publication. The test must initially demonstrate that mutated bytes are published, then pass only when sealed-copy publication rejects the mutation and restores the source.
2. Ingest identical content under two different names or collections. The test must initially demonstrate two files with one repointed row, then pass only when the first file and row remain canonical and the second source is discarded.
3. Cover a stale checksum row whose canonical file is missing and verify that a new ingestion repairs the row.
4. Re-run existing snapshot, rollback, collision, EXDEV, cancellation, routing, and video regressions.

## Compatibility

The change is internal to book ingestion and the low-level `FileStore` snapshot API. Public REST, Tauri, database schema, video ingestion, and download event formats remain unchanged.
