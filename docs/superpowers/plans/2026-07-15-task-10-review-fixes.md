# Task 10 Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make generated book download URLs safe for reserved filename characters and return stable JSON errors for invalid checksum path parameters.

**Architecture:** Encode each logical book path segment at the web URL boundary while leaving filesystem paths unchanged. Parse checksum strings inside the handlers so Axum cannot bypass the API response contract with its default extractor rejection.

**Tech Stack:** Rust, Axum 0.8, `urlencoding`, Tokio integration tests, Cargo test runner.

## Global Constraints

- Preserve existing `/api/media`, stream, thumbnail, and valid book API behavior.
- Keep book downloads confined to `BOOK_DIR` through the retained capability implementation.
- Use sanitized JSON `Response` errors for client input failures.
- Do not add HTTP range handling; it is outside Task 10 requirements.

---

### Task 1: Percent-encode book download URL segments

**Files:**
- Modify: `src/domain/algorithm/naming.rs`
- Test: `src/domain/algorithm/naming.rs`
- Test: `tests/book_api_test.rs`

**Interfaces:**
- Consumes: `get_book_url(collection: &str, file_name: &str) -> String`
- Produces: the same interface, with every path segment encoded independently and `/` retained only between segments.

- [ ] **Step 1: Write failing URL and end-to-end tests**

Add a unit assertion equivalent to:

```rust
assert_eq!(
    get_book_url("Programming/C# & Rust", "100%? Complete.epub"),
    "/api/books/download/Programming/C%23%20%26%20Rust/100%25%3F%20Complete.epub"
);
```

Add an integration test that stores and serves a real EPUB whose collection and filename contain `#`, `%`, spaces, and `&`, then downloads it through the URL returned by the API.

- [ ] **Step 2: Run tests and verify the expected failures**

Run: `cargo test --lib get_book_url --features webserver`

Expected: FAIL because `get_book_url` currently emits reserved characters unchanged.

Run the new `book_api_test` case and expect the returned URL or download request assertion to fail for the same reason.

- [ ] **Step 3: Encode logical path segments at the URL boundary**

Implement `get_book_url` by collecting normal collection components, applying `urlencoding::encode` to each UTF-8 component and the sanitized filename, joining them with `/`, and prefixing `/api/books/download/`. Do not change `get_book_download_path`, which remains a filesystem-relative path helper.

- [ ] **Step 4: Run the focused tests and verify they pass**

Run: `cargo test --lib get_book_url --features webserver`

Run the new `book_api_test` case.

Expected: PASS, including a successful download of the reserved-character fixture.

### Task 2: Return JSON 400 responses for invalid book checksums

**Files:**
- Modify: `src/entrypoints/api.rs`
- Test: `tests/book_api_test.rs`

**Interfaces:**
- Consumes: `GET /api/book/{checksum}` and `DELETE /api/book/{checksum}`.
- Produces: valid `i64` behavior unchanged; malformed and overflowing values return status 400 with `Response::error("invalid book checksum")`.

- [ ] **Step 1: Write failing integration tests**

Exercise both methods with `not-a-number` and `9223372036854775808`. Assert status 400, JSON content type, an empty success message, and exactly one sanitized error: `invalid book checksum`.

- [ ] **Step 2: Run tests and verify the expected failures**

Run the new `book_api_test` checksum case.

Expected: FAIL because Axum currently rejects `Path<i64>` before the handlers and returns plain text.

- [ ] **Step 3: Parse checksums inside the handlers**

Change both handlers to extract `Path<String>`. Add a shared parser:

```rust
fn parse_book_checksum(checksum: &str) -> Result<i64, StdResponse> {
    checksum.parse::<i64>().map_err(|_| {
        std_error(BAD_REQUEST, "invalid book checksum".to_string())
    })
}
```

Use `?` in the GET handler and an early return in the DELETE handler.

- [ ] **Step 4: Run the focused tests and verify they pass**

Run the new invalid-checksum integration test and the existing successful lookup/deletion tests.

Expected: PASS with stable JSON responses and no valid-request regression.

### Task 3: Verify, commit, and publish

**Files:**
- Verify all modified files and the existing Task 10 implementation.

**Interfaces:**
- Consumes: both completed fixes.
- Produces: a clean, pushed update to `codex/task-10-ebook-support` and PR #58.

- [ ] **Step 1: Run formatting and focused tests**

Run `cargo fmt --check` and both focused regression-test commands.

- [ ] **Step 2: Run complete verification**

Run `make test`, `cargo check`, and `git diff --check origin/spec/ebook-support..HEAD`.

Expected: all commands exit zero; the existing unrelated `DEFAULT_MIGRATIONS_DIR` warning may remain.

- [ ] **Step 3: Commit**

Stage only the review-fix plan, production changes, and regression tests. Commit with `Fix book API review findings`.

- [ ] **Step 4: Push**

Push `codex/task-10-ebook-support` to `origin` and verify the remote tracking SHA equals local `HEAD`.
