# ADR 0001: Retain the Legacy `i64` Checksum for Ebook v1

- Status: Accepted
- Date: 2026-07-18

## Context

Books currently use the same legacy identity shape as videos: an `i64` produced
by Rust's `DefaultHasher` over a bounded prefix of the file. The checksum is the
SQLite primary key and is serialized as a string in REST, Tauri, and event
contracts.

Rust does not specify `DefaultHasher` as stable across releases. Prefix hashing
also permits distinct files to share a key. Ebook ingestion mitigates silent
data corruption by comparing complete contents when a checksum row already
exists. Different colliding content is rejected, the incoming source is
restored, and the canonical row and file are preserved.

## Decision

Ebook v1 retains the existing `i64` checksum algorithm, schema, and public
contract for compatibility with the shared video identity model. Persisted
checksums are treated as stored identifiers and are not recomputed solely
because the Rust toolchain changes.

This is explicit risk acceptance for the first ebook release, not approval of
`DefaultHasher` as a permanent durable identity algorithm.

## Consequences

- A healthy canonical row blocks ingestion of different content with the same
  checksum until that identity conflict is resolved.
- Complete-content comparison keeps collisions fail-closed instead of silently
  replacing or aliasing a book.
- Checksum values may differ when the same bytes are newly ingested by builds
  using different Rust hashing implementations.
- A future migration must introduce a specified full-content digest and a
  durable identifier independent of `DefaultHasher`.
- That migration must backfill existing rows, preserve compatibility with
  current checksum-based URLs and events during a transition, and define how
  legacy and stable identities are resolved before removing the `i64` key.

## Out of Scope

This decision does not change video identity, the books table, public API
routes, collision comparison, or source-restoration behavior.
