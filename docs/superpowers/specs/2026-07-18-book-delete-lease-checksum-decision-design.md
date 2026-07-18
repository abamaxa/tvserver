# Book Delete Lease and Checksum Decision Design

## Context

`BookStore::delete` stages the canonical book file before conditionally deleting
its database row. During that staging window, `BookCheck` can observe the
canonical path as missing, delete the row as an orphan, and cause the user
deletion to restore the staged file after its conditional database delete
affects zero rows. A later scan can then ingest the restored file again.

The ebook implementation also persists a legacy `i64` checksum as the book
primary key. It is produced by Rust's `DefaultHasher` over a bounded prefix of
the file. Existing collision handling compares complete file contents and
fails closed, but the durable identity algorithm itself is neither
collision-free nor stable across Rust releases.

Finally, release builds intentionally use panic unwinding so worker-level panic
containment remains effective, but `Cargo.toml` does not record that dependency
beside the profile setting.

## Considered Lease Approaches

### Blocking path-scoped reconciliation lease

Add a blocking `acquire_reconciling(path)` operation to the existing
`BookPathLeaseCoordinator`. Inject the runtime's shared coordinator into
`BookStore`, then hold a reconciliation lease on the validated canonical path
from before file staging until all success cleanup or rollback work completes.

This is the selected approach. It gives user deletion deterministic completion,
serializes it with ingestion and orphan reconciliation for the same path, and
does not delay unrelated books.

### Non-blocking acquisition with a busy error

`BookStore::delete` could call the existing `try_acquire_reconciling` and return
an error when another operation owns the path. This is smaller, but it exposes
an avoidable transient failure to users and makes callers responsible for retry
behavior that the backend can safely provide.

### Global scanner/deletion lock

A single runtime-wide lock could serialize every book scan and deletion. It
would prevent the race, but unnecessarily couples unrelated paths and permits a
large library scan to delay all deletes.

## Lease Architecture

`BookPathLeaseCoordinator` retains the existing mutually exclusive path-state
map. Both blocking acquisition methods wait on the coordinator's notification
until the requested path is free:

- `acquire_processing(path)` records `Processing` and is used by ingestion;
- `acquire_reconciling(path)` records `Reconciling` and is used by explicit
  deletion;
- `try_acquire_reconciling(path)` remains non-blocking so periodic orphan
  reconciliation skips an active path instead of delaying the full scan.

`BookRuntime` creates one coordinator and passes clones to `BookCheck`,
`BookStore`, and `BookIngestionRuntime`. Existing standalone constructors keep
creating a private coordinator for compatibility; a new root-and-leases
constructor provides explicit injection for the runtime and deterministic tests.

`BookStore::delete` continues retrieving and validating the recorded collection
and filename before forming `book_root/collection/file_name`. It acquires the
blocking reconciliation lease after forming that canonical path and before the
first filesystem inspection or mutation. The local lease binding remains alive
through staging, conditional row deletion, staged-file cleanup, or rollback.
RAII releases the path on every return path.

The conditional database delete remains necessary. The path lease coordinates
in-process book workers and the scanner; it does not replace protection against
repository changes made by another process or database connection.

## Checksum Identity Decision

Ebook v1 retains the shared legacy `i64` checksum identity and current API and
schema. This is an explicit compatibility decision, not a claim that the
algorithm is suitable as a permanent durable identifier.

The accepted constraints are:

- `DefaultHasher` output may change across Rust releases;
- hashing only a bounded prefix permits distinct files to share an `i64` key;
- while a healthy canonical row occupies that key, a different colliding book
  is rejected and its source is restored;
- complete-content comparison prevents a collision from silently replacing or
  aliasing the canonical book;
- existing persisted checksums must not be recomputed merely because the server
  toolchain changes.

A future identity migration is separate work. It should introduce a specified
full-content digest and a durable identifier that is not derived from
`DefaultHasher`, backfill existing rows without losing their current API
identity, and provide an explicit compatibility period before removing the
legacy key. This change does not attempt that schema and API migration.

The decision is recorded as a concise ADR under `docs/adr/` so it remains
discoverable independently of implementation plans.

## Panic Containment Documentation

The release profile keeps `panic = "unwind"`. An inline comment will state that
worker-level `catch_unwind` containment depends on unwinding, preventing a
binary-size cleanup from restoring abort behavior and turning parser panics
into whole-process termination.

## Testing

The implementation follows test-first development:

1. A coordinator test proves blocking reconciliation waits while a processing
   lease owns the same path and acquires after release.
2. A deterministic `BookStore` regression pauses deletion after the canonical
   file is staged, runs `BookCheck` with the same coordinator, and proves the
   scanner preserves the row until deletion completes successfully.
3. Existing deletion, scanner, runtime, metadata, and API suites continue to
   pass under both feature configurations.

## Scope

This change does not alter the checksum algorithm, public checksum strings,
SQLite schema, full-content collision comparison, ingestion routing, PDF/EPUB
metadata behavior, or authentication policy. It does not create a frontend
metadata endpoint or merge `spec/ebook-support` into `main`.
