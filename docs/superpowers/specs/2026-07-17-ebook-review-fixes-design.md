# Ebook Review Fixes Design

## Context

Review of the ebook-support branch found three regressions: deletion can remove a
checksum row after a concurrent ingestion relocates it, deeply nested collection
metadata reports the root rather than the immediate parent, and default-feature
integration-test compilation imports the feature-gated webserver module.

## Deletion Consistency

Deletion continues to stage the book and thumbnail before changing the database,
but the database delete must match both the checksum and the path that was staged.
The existing `delete_book_if_path_matches` repository operation provides that
compare-and-delete boundary without adding another service-wide lease.

If the conditional delete affects no row, another operation changed or removed the
record after it was read. The service restores every staged artifact, returns an
error, and leaves the newer database state untouched. Repository errors follow the
same rollback path. A successful one-row delete keeps the existing finalize and
rollback behavior.

## Immediate Collection Parent

Collection identifiers use `/` as a domain separator independently of the host
filesystem. `BookCollectionDetails` therefore derives its parent by splitting on
the final `/`. Root collections retain an empty parent, while
`Fiction/Classics/British` reports `Fiction/Classics`.

## Feature-Gated Integration Tests

HTTP integration helpers and the test crates that consume them are webserver-only.
They are compiled only when the `webserver` feature is active. Default-feature
`cargo test --all-targets --no-run` must compile without importing
`entrypoints::webserver`; focused webserver integration tests must continue to run
when that feature is enabled.

## Testing

- A deterministic repository wrapper relocates a checksum row immediately before
  deletion. The regression proves the relocated row survives and staged artifacts
  are restored.
- A three-level collection regression proves the immediate parent is returned.
- A default-feature all-target compile verifies feature isolation, followed by the
  focused webserver ebook integration suites.
