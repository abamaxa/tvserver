# PDF Backend Renderer Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the unused backend PDF renderer API and Cargo feature while preserving safe filename/default-cover PDF ingestion and documenting the actual product behavior.

**Architecture:** Keep `extract_pdf_metadata` as the single PDF backend boundary and move the existing fallback body directly into it. Remove renderer compatibility symbols and configuration rather than replacing them with deprecated stubs; frontend PDF.js metadata submission remains separate work in GitHub issue #64.

**Tech Stack:** Rust 2021, Cargo features, existing book-metadata unit tests, Markdown documentation.

## Global Constraints

- The Rust backend must not parse PDF bytes or render PDF pages.
- PDF files remain ingestible and downloadable with a filename-derived title and the shared default cover.
- Remove `pdf-thumbnails`, `PdfThumbnailRenderer`, `DefaultPdfThumbnailRenderer`, and `extract_pdf_metadata_with_renderer` completely.
- Do not add a frontend metadata-update endpoint in this task.
- Do not change EPUB extraction, book identity, authentication, or unrelated review findings.
- Do not apply a repository-wide formatting rewrite; stable rustfmt has a known nightly-only baseline limitation.

---

### Task 1: Delete the Renderer Surface and Correct the PDF Contract

**Files:**
- Modify: `Cargo.toml:78-84`
- Modify: `README.md:61-76`
- Modify: `src/domain/services/book_metadata.rs:1074-1126`
- Modify: `src/domain/services/book_metadata.rs:2256-2352`
- Modify: `src/domain/services/mod.rs:12-18`
- Test: `src/domain/services/book_metadata.rs`

**Interfaces:**
- Consumes: `ensure_default_book_thumbnail(&Path)`, `filename_derived_title(&Path)`, `DEFAULT_BOOK_THUMBNAIL`, and `BookMetadataExtraction`.
- Produces: `pub fn extract_pdf_metadata(&Path, &Path, &str) -> Result<BookMetadataExtraction, BookMetadataExtractionError>` as the only public PDF extraction function.

- [ ] **Step 1: Run the failing renderer-surface absence assertion**

Run before editing:

```bash
if rg -n \
  'PdfThumbnailRenderer|DefaultPdfThumbnailRenderer|extract_pdf_metadata_with_renderer|pdf-thumbnails' \
  Cargo.toml README.md src; then
  exit 1
fi
```

Expected: exit 1 with matches in `Cargo.toml`, `README.md`,
`src/domain/services/book_metadata.rs`, and `src/domain/services/mod.rs`. This
is the RED contract: the unused surface is still present.

- [ ] **Step 2: Collapse safe PDF fallback into the retained function**

Delete the renderer trait, default renderer type and implementation, and
`extract_pdf_metadata_with_renderer`. Replace the current delegating
`extract_pdf_metadata` body with:

```rust
pub fn extract_pdf_metadata(
    pdf_path: &Path,
    thumbnail_dir: &Path,
    _thumbnail_key: &str,
) -> Result<BookMetadataExtraction, BookMetadataExtractionError> {
    ensure_default_book_thumbnail(thumbnail_dir)
        .map_err(|error| BookMetadataExtractionError::Pdf(error.to_string()))?;
    Ok(BookMetadataExtraction {
        title: Some(filename_derived_title(pdf_path)),
        authors: Vec::new(),
        description: None,
        page_count: None,
        thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
        metadata: BookMetadata {
            raw: Some(json!({
                "pdf": { "metadataSkipped": "untrusted PDF parsing disabled" }
            })),
            ..BookMetadata::default()
        },
        warnings: vec!["PDF metadata parsing is disabled for untrusted input".to_string()],
        ..BookMetadataExtraction::default()
    })
}
```

The `_thumbnail_key` parameter stays because ingestion already supplies it and
the common extraction signature remains useful; it must not trigger any
thumbnail generation.

- [ ] **Step 3: Remove public exports and the Cargo feature**

Change the book-metadata exports in `src/domain/services/mod.rs` to:

```rust
pub use book_metadata::{
    extract_epub_metadata, extract_pdf_metadata, generate_book_metadata,
    BookMetadataExtraction, BookMetadataExtractionError,
};
```

Delete this line from `[features]` in `Cargo.toml`:

```toml
pdf-thumbnails = []
```

Do not replace it with an alias, deprecated feature, or `compile_error!`.

- [ ] **Step 4: Convert PDF regressions to the retained public function**

Delete `CountingRenderer` and its `PdfThumbnailRenderer` implementation. In
each of these tests:

- `invalid_pdf_uses_filename_and_default_cover_without_parsing_or_rendering`
- `pdf_metadata_bytes_are_skipped_in_favor_of_safe_fallback`
- `pdf_fallback_uses_filename_title_and_empty_authors`

replace the renderer-aware call with the direct function:

```rust
let result = extract_pdf_metadata(&pdf_path, &covers, "unsafe")
    .expect("safe PDF fallback should not parse input bytes");
```

Use each test's existing thumbnail key in place of `"unsafe"`. Delete only the
renderer construction and call-count assertions. Preserve assertions for title,
empty metadata fields, diagnostic marker, warning, default-cover filename, and
materialized default-cover bytes.

- [ ] **Step 5: Replace the stale README claims**

Replace the current three paragraphs beginning `PDF metadata extraction is
Rust-native` through the Android Pdfium paragraph with:

```markdown
The backend does not parse PDF metadata or render PDF pages. PDF files remain ingestible and
downloadable: the original filename provides the fallback title, authors and page count remain
empty, and the book uses `default-book.jpg`. Rich PDF metadata supplied by a frontend is not yet
part of the backend API.

EPUB metadata and cover extraction remain available in the backend and use bounded archive,
document, and image-processing limits.
```

Do not describe GitHub issue #64 as implemented behavior.

- [ ] **Step 6: Run the renderer-surface assertion and focused tests**

Run:

```bash
if rg -n \
  'PdfThumbnailRenderer|DefaultPdfThumbnailRenderer|extract_pdf_metadata_with_renderer|pdf-thumbnails' \
  Cargo.toml README.md src; then
  exit 1
fi
cargo test --lib domain::services::book_metadata::tests::invalid_pdf_uses_filename_and_default_cover_without_parsing_or_rendering -- --exact
cargo test --lib domain::services::book_metadata::tests::pdf_metadata_bytes_are_skipped_in_favor_of_safe_fallback -- --exact
cargo test --lib domain::services::book_metadata::tests::pdf_fallback_uses_filename_title_and_empty_authors -- --exact
```

Expected: the search prints nothing and exits 0; each focused test reports 1
passed and 0 failed.

- [ ] **Step 7: Run scoped and compile verification**

Run:

```bash
cargo test --lib domain::services::book_metadata::tests
cargo test --all-targets --no-run
cargo test --features webserver --all-targets --no-run
git diff --check
```

Expected: the book-metadata suite passes, both target matrices compile, and
`git diff --check` prints nothing. Do not run a broad formatter rewrite. If
`cargo fmt --all -- --check` is run, record the existing nightly-only baseline
failure without editing unrelated files.

- [ ] **Step 8: Commit the cleanup**

Review `git diff -- Cargo.toml README.md src/domain/services/book_metadata.rs
src/domain/services/mod.rs`, then run:

```bash
git add Cargo.toml README.md src/domain/services/book_metadata.rs src/domain/services/mod.rs
git commit -m "refactor: remove unused PDF renderer API"
```

Expected: one commit containing only the renderer cleanup, direct fallback test
updates, and corrected README contract.
