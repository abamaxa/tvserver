# Task 10 Portability Review Fixes Design

## Purpose

Close the remaining Task 10 review findings without introducing platform-specific runtime dependencies. The fixes must behave consistently on mobile, desktop, and cloud deployments while preserving the existing book API and filesystem security boundaries.

## Scope

This follow-up covers four review findings:

1. Replace a stale `default-book.jpg` portably when the destination already exists.
2. Store and expose nested collection identifiers with `/` separators on every host platform.
3. Remove worktree/agent scratch exclusions from the feature's tracked `.gitignore` diff.
4. Replace the text `generated-cover.jpg` fixture with test setup that serves valid JPEG bytes.

HTTP range support and unrelated formatting changes remain outside scope.

## Portable Default-Cover Replacement

`ensure_default_book_thumbnail` will remain standard-library-only. A process-wide mutex will serialize materialization so concurrent metadata tasks cannot remove or replace the same destination simultaneously.

The operation will:

1. Create the thumbnail directory.
2. Return immediately when the existing regular file already contains the embedded cover bytes.
3. Write and sync the embedded JPEG to a unique temporary file in the destination directory.
4. If the destination exists, remove that path entry with `remove_file`; this removes a symlink itself rather than following it.
5. Rename the temporary file to `default-book.jpg`.
6. Sync the containing directory on Unix, where that operation is available.
7. Remove the temporary file on every failure path.

This deliberately accepts a short remove-to-rename availability gap in exchange for using one portable implementation across Windows, macOS, Linux, mobile, and cloud targets. Router startup performs this before serving requests. Same-process calls are serialized; a competing external process can cause a safe failure, but cannot redirect the write outside the thumbnail directory.

Existing behavior remains unchanged for an already-current cover, directories at the destination path, and symlink destinations. A stale regular file or symlink is replaced by the embedded regular JPEG without modifying a symlink target.

## Platform-Neutral Collection Identifiers

Collection identifiers are domain/API values, not filesystem paths. A shared helper will convert relative `Path` components to UTF-8 strings and join them with `/`, regardless of the host separator.

The helper will be used by:

- Task 8's authoritative ingestion `collection_from_source` in
  `src/domain/services/book_metadata.rs`;
- rooted collection derivation in `src/domain/algorithm/naming.rs`;
- collection-and-filename derivation in the same module;
- `BookStore::collection_from_source` when its legacy/public add-file path is used.

Invalid UTF-8 remains an error where the existing interface supports errors and falls back to the existing empty-string behavior in legacy string-returning helpers. Parent traversal, roots, and prefixes are not converted into collection identifiers.

Filesystem operations continue to parse validated `/`-separated collection identifiers into native `PathBuf` components through the existing path-validation functions. Repository queries, serialized payloads, and HTTP paths therefore use one stable separator while local file access remains native.

## Test Fixtures and Repository Hygiene

The tracked text file named `generated-cover.jpg` will be removed. The generated-thumbnail integration test will create an isolated thumbnail directory and write the already-embedded valid JPEG bytes under a generated filename before starting the test router. The test will assert JPEG content type and exact valid JPEG bytes.

The feature-only `/.worktrees/` and `/.superpowers/` additions will be removed from tracked `.gitignore`, leaving no net housekeeping change in the PR. Local scratch paths may remain excluded through Git's untracked local exclude mechanism and will not be committed.

## Task 8 Integration Validation

The design was revalidated after fetching `origin/spec/ebook-support` at
`52742e8`, which includes Task 8 and its ingestion-hardening follow-ups.

Task 8 changes `src/domain/services/book_metadata.rs` substantially and now
derives and persists collections in its own `collection_from_source` helper.
That is the production boundary the normalization fix must cover. The shared
path-component-to-identifier helper will therefore be consumed by Task 8's
ingestion code as well as the existing naming and `BookStore` paths, avoiding
parallel separator logic.

Task 8 does not change `src/domain/models/book.rs`, `.gitignore`, or the Task 10
thumbnail fixture. It calls `ensure_default_book_thumbnail` from multiple
ingestion failure paths, so the portable stale-cover fix remains necessary.
Task 8's generated-thumbnail publication cannot replace that helper: its
no-replace ownership rule deliberately preserves an existing generated file,
whereas the fixed default asset must replace stale bytes.

Task 8 also changes the shutdown block at the top of
`src/entrypoints/webserver.rs`. The existing Task 10 router changes touch the
same file but a separate hunk, and Git's merge-tree resolves them without a
content conflict. The portability follow-up itself does not modify
`webserver.rs`.

Before implementation, merge the fetched `origin/spec/ebook-support` into the
feature branch so tests and edits use the actual Task 8 ingestion code. Do not
reimplement or bypass Task 8's snapshot, publication, locking, cleanup, or
cancellation behavior.

## Testing

Implementation will follow red-green cycles:

- A stale-cover replacement regression will exercise the portable remove-and-rename path and retain the existing symlink-target safety test.
- Collection conversion tests will assert `/`-joined nested identifiers from path components. A Windows-only case will use native backslash input when compiled on Windows.
- Task 8 ingestion tests will verify a nested source under `BOOK_DIR` persists a `/`-separated collection without changing its native destination path.
- BookStore tests will verify its existing add-file path consumes the same shared normalization helper.
- The book thumbnail integration test will serve a valid JPEG created in an isolated directory.
- Existing Task 10 API, static-route security, library, and integration suites will remain green.

Windows-specific execution cannot be claimed from a non-Windows host. The platform-neutral helper will be tested on the current host, Windows-only assertions will compile and run in Windows CI when available, and the implementation will avoid target-specific APIs.

## Acceptance Criteria

- Replacing a stale default cover does not rely on rename-over-existing semantics.
- A symlink at the default-cover path is removed without modifying its target.
- Nested collection identifiers use `/` in repository and API values on every platform.
- Native filesystem paths still resolve the same book directories.
- Generated-thumbnail tests serve valid JPEG bytes.
- The PR has no net `.gitignore` housekeeping diff.
- No new platform-specific runtime dependency is added.
