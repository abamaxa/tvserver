# PDF Backend Renderer Cleanup Design

## Context

The Rust backend deliberately does not parse untrusted PDF bytes. PDF ingestion
derives a title from the original filename, assigns the shared default cover,
and leaves authors, page count, and other rich metadata empty. The repository
still exposes a no-op PDF renderer trait, renderer type, renderer-aware
extraction function, and empty `pdf-thumbnails` Cargo feature. The README also
describes metadata extraction and optional Pdfium rendering that no longer
exist.

Rich PDF metadata extracted by the PDF.js frontend is separate work tracked in
GitHub issue #64. That future workflow requires its own authenticated,
validated, concurrency-safe update design.

## Decision

Delete the unused backend renderer surface instead of preserving deprecated or
no-op compatibility APIs.

- Remove the `pdf-thumbnails` Cargo feature.
- Remove `PdfThumbnailRenderer` and `DefaultPdfThumbnailRenderer`.
- Remove `extract_pdf_metadata_with_renderer`.
- Remove the corresponding public re-exports.
- Keep `extract_pdf_metadata` as the backend's safe PDF fallback entry point.

The retained function does not open or inspect PDF bytes. It materializes the
default book thumbnail, derives the title from the supplied PDF filename, and
returns the existing metadata-skipped diagnostic and warning.

## Product Documentation

Update the README to state the current behavior directly:

- the backend does not parse PDF metadata or render PDF pages;
- PDFs remain ingestible and downloadable;
- the original filename supplies the fallback title;
- the shared default cover is used;
- rich client-side metadata submission is not yet part of the backend API.

Do not advertise issue #64 as implemented behavior or promise an endpoint that
does not exist.

## Testing

Tests continue to prove that invalid or metadata-looking PDF bytes are ignored,
the original filename determines the title, and the default cover is used.
Renderer call-count mocks and renderer-specific tests are deleted with the API.

Verification includes:

- focused PDF metadata tests;
- the complete book-metadata suite;
- default and `webserver` all-target compilation, proving no feature or export
  references remain;
- a repository search for the removed names and `pdf-thumbnails`;
- `git diff --check`.

The repository-wide stable-rustfmt baseline limitation remains outside this
cleanup; no broad formatting rewrite is permitted.

## Scope

This cleanup does not add a frontend metadata-update API, restore an
out-of-process PDF parser, change EPUB extraction, change book identity, or
address other review findings.
