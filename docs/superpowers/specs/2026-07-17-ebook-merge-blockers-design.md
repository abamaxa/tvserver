# Ebook Merge Blockers Design

## Context

Review of `spec/ebook-support` found five merge-blocking regressions in shared
video collection naming, book configuration, PDF title fallback, duplicate
handling, and book scanning. This change fixes those five findings without
changing the public ebook API or replacing the existing `i64` checksum scheme.

## Legacy Video Collection Names

Video collection derivation returns to the behavior on `main`: paths relative to
`MOVIE_DIR` retain their native string representation even when a component
contains `:` or `\`. Strict portable-segment validation remains limited to book
collection identifiers and book path conversion.

This separation prevents an invalid video component from collapsing to the empty
collection and making `MediaCheck` repeatedly list the movie root.

## Optional Book Directory

`BOOK_DIR` remains an explicit override. When it is absent, the default is a
lowercase `books` directory beside `MOVIE_DIR`. For example,
`MOVIE_DIR=/srv/media/movies` resolves to `/srv/media/books`.

All entrypoints, including the webserver router, use the same configuration
helper so video-only upgrades no longer panic or fail solely because `BOOK_DIR`
is unset. Documentation describes `BOOK_DIR` as optional and records the default.

## PDF Filename Fallback

PDF extraction runs against a private snapshot, so its filename-derived fallback
title is based on the snapshot path. The ingestion fixup compares the extracted
title with the title derived from that extraction path. On a match it replaces
the snapshot title with the title derived from the original source path.

## Collision-Safe Duplicate Handling

A checksum match is only treated as a healthy duplicate when the private
snapshot and the existing canonical book contain identical bytes. Comparison
opens the canonical file through the book-root capability without following
symlinks and compares the complete contents rather than the checksum prefix.

If the bytes match, ingestion removes the snapshot and discards the staged
duplicate as before. If they differ, ingestion treats the match as a checksum
collision: it removes the private snapshot, restores the staged source, returns
an error, and leaves the canonical file and database record untouched. This is a
minimal data-safety fix that avoids a checksum migration or API change.

## Resilient Book Scanning

The book scanner continues enforcing portable collection identifiers. When a
child directory cannot be represented as a portable identifier, the scanner logs
and skips that subtree while continuing through valid sibling directories. A
single unsupported directory name therefore cannot prevent ingestion or orphan
reconciliation for the rest of `BOOK_DIR`.

## Testing

Each behavior receives a focused regression test written before its production
change:

- video paths containing `:` and `\` preserve their collection names;
- an unset `BOOK_DIR` defaults beside `MOVIE_DIR`, and the webserver accepts it;
- a metadata-free PDF receives the original filename-derived title;
- identical full files deduplicate, while a shared-prefix checksum collision
  restores the source and preserves the existing book;
- a nonportable book directory is skipped without blocking a valid sibling.

After the focused red-green cycles, verification runs formatting, the affected
unit and integration suites, both feature configurations, and `git diff --check`.

## Scope

This pass does not change CORS/authentication policy, automatic PDF/EPUB routing
from `MOVIE_DIR`, the checksum schema, or the optional maintainability and
performance suggestions from the review.
