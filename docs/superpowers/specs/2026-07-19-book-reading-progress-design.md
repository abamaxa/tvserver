# Embedded Book Reading Progress Design

## Summary

Store the current reading position as an optional field of each book record. Book list and detail responses carry progress with the rest of `BookDetails`; clients do not load or cache a second progress resource.

Keep one narrow write operation, `PUT /api/book/{checksum}/progress`, because reader relocations should not submit the complete book record. Remove the separate progress table, service, read/list/delete APIs, Tauri commands, and frontend progress store.

## Scope

The feature includes:

- one optional current position per book;
- EPUB CFI and PDF page locators;
- an optional normalized progression value;
- a server-authored update timestamp;
- progress embedded in all book list and detail responses;
- one REST PUT route and one Tauri save command;
- immediate frontend cache updates after reader relocations.

The feature does not include per-user progress, history, bookmarks, highlights, annotations, a separate progress resource, or live synchronization between clients.

## Book Model

Add optional `progress` to `BookDetails`:

```json
{
  "fileName": "example.epub",
  "checksum": "9223372036854775807",
  "format": "epub",
  "progress": {
    "locator": {
      "type": "epub-cfi",
      "value": "epubcfi(/6/4!/4/2/8)"
    },
    "progression": 0.42,
    "updatedOn": "2026-07-19T12:00:00Z"
  }
}
```

The nested progress object does not repeat the book checksum. Its owning `BookDetails` already supplies that identity.

Clients save only the fields they own:

```json
{
  "locator": {
    "type": "epub-cfi",
    "value": "epubcfi(/6/4!/4/2/8)"
  },
  "progression": 0.42
}
```

`updatedOn` is assigned by the backend and serialized as RFC 3339 UTC with a `Z` suffix. The request cannot set it.

The locator type is a closed enum with `epub-cfi` and `pdf-page`. Locator values are opaque to the backend but must contain a non-whitespace character. `progression` is optional and, when present, must be finite and within inclusive `0..=1`.

The checksum remains an internal `i64` and a decimal string at transport boundaries. Tests continue to cover `9223372036854775807` so JavaScript never has to represent a full-width checksum as a number.

## Persistence

The existing progress migration has never been deployed, so rewrite it rather than add a follow-up migration:

```sql
ALTER TABLE books ADD COLUMN progress TEXT;
```

`progress` contains the nested JSON object or SQL `NULL`. This follows the existing JSON-backed `metadata` pattern and avoids a second table, foreign key configuration, joins, and cascade logic.

Book row mapping deserializes valid progress JSON into `BookDetails.progress`. A missing, null, or malformed value is treated as absent, matching the repository's tolerant metadata decoding.

Saving progress performs one conditional update by checksum. The repository serializes a validated locator, optional progression, and server timestamp, then executes:

```sql
UPDATE books SET progress = ? WHERE checksum = ?;
```

Zero affected rows means the book does not exist. The operation does not change the book-level `updated_on` field and does not emit a book metadata event.

Existing book ingestion must not include `progress` in its insert/update assignments. A same-checksum refresh therefore preserves progress. Same-path/different-checksum replacement deletes the old book row, naturally discarding its embedded progress rather than transferring it to different content.

## Backend Architecture

Keep the locator and progress DTOs in the book model. A small validation method converts the save request into validated values. Do not introduce a standalone progress service or progress-specific read/delete outcome types.

Extend `Databaser` with only the narrow save method needed by the transports. It returns whether a matching book was updated. Existing `list_books`, `list_all_books`, and `retrieve_book` automatically carry progress because they already return `BookDetails`.

REST and Tauri each:

1. verify the book runtime is available;
2. parse the checksum using the existing book checksum rules;
3. validate the request model;
4. invoke the repository save method;
5. map no matching row to book-not-found.

This limited duplication is preferable to a new service layer for a single write operation.

## Operations

### REST

Keep only:

- `PUT /api/book/{checksum}/progress`
  - accepts locator and optional progression;
  - returns `204 No Content` after saving;
  - returns `400 Bad Request` for an invalid checksum, locator, or progression;
  - returns `404 Not Found` for an unknown book;
  - returns the existing `503 Service Unavailable` response when the handler reaches an unavailable book runtime;
  - logs unexpected repository details and returns a sanitized `500 Internal Server Error`.

Use ordinary Axum extraction and rejection ordering. There is no custom rule making runtime unavailability take precedence over malformed or oversized JSON.

Remove:

- `GET /api/book-progress`;
- `GET /api/book/{checksum}/progress`;
- `DELETE /api/book/{checksum}/progress`.

### Tauri

Keep and register only:

```text
save_book_progress(checksum: String, progress: SaveBookProgressRequest) -> Result<(), String>
```

Remove the list, get, and delete progress commands. Book list and detail commands return the embedded field through `BookDetails`.

## Frontend Architecture

In the React application:

- add optional `progress: BookReadingProgress` to `BookDetails`;
- define `BookReadingProgress` as locator, optional progression, and `updatedOn`, without a checksum;
- validate optional nested progress as part of `BookDetails` response validation;
- retain `BookService.saveProgress` and remove `listProgress`, `getProgress`, and `deleteProgress`;
- remove the initial `/book-progress` request;
- remove the Redux progress map, load state, mutation generations, request generations, and fetch thunk;
- copy nested progress in `serializableBook`;
- have book cards and rows read `book.progress` directly instead of accepting a parallel progress map;
- open the reader with one `getBook` request and restore from `book.progress`;
- keep debounced/flush-on-close saves through the narrow PUT operation;
- update cached `BookDetails.progress` values optimistically for the detail record and any loaded collection entries so the library indicator stays current.

The reader controller keeps the checksum beside its pending save internally; the nested progress model itself does not contain the checksum. Failed saves retain the existing unsynced-position warning behavior.

The Tauri REST adaptor keeps only the PUT-to-`save_book_progress` mapping and removes progress list, optional get, and delete mappings.

## OpenAPI

Update `docs/api/openapi.yaml` to:

- add optional `progress` to `BookDetails`;
- retain reusable schemas for `BookReadingProgress`, `SaveBookProgressRequest`, and the two locator variants;
- omit checksum from the nested progress schema;
- document only the PUT progress path with a `204` success response;
- remove the progress list schema and the list/get/delete operations.

The schema preserves the signed-decimal checksum string, closed locator variants, nonblank locator value, optional bounded progression, and UTC `updatedOn` timestamp.

## Error Handling

Model validation produces stable messages for blank locators and invalid progression. Unsupported locator types may be rejected by normal Serde/Axum or Tauri deserialization because cross-transport rejection parity is no longer a requirement.

Repository failures are logged without exposing SQL details. A stored malformed progress JSON is ignored on read rather than preventing the containing book from loading.

## Testing

Backend tests cover:

- request and nested progress serialization for EPUB and PDF;
- blank locator and progression validation;
- the nullable `books.progress` migration column;
- save and replace behavior with a server-authored timestamp;
- unknown-book saves;
- progress included in collection and detail responses;
- same-checksum ingestion preservation and different-checksum replacement cleanup;
- REST PUT success, invalid input, unknown book, unavailable runtime, and sanitized repository failure;
- Tauri save registration and core behavior;
- the simplified OpenAPI contract.

Frontend tests cover:

- `BookDetails` validation with absent and present progress;
- the narrow save request;
- reducer updates to embedded progress across cached detail and collection records;
- cards and rows rendering progress from each book;
- reader restore from the fetched book and continued debounced saving;
- Tauri adaptor mapping for the single save operation.

Delete tests for separate progress fetching, fetch races, list/get/delete routes, progress-table constraints, foreign keys, and cascade behavior.

## Worktree Safety

The backend repository is the `src-tauri` submodule and the frontend is its parent repository. Implementation must commit changes in the correct repository and update the parent submodule pointer intentionally.

Existing dirty `.github`, generated `client/newapp`, frontend configuration, search-service, temporary database, and unrelated design-document changes belong to the user. Do not modify, stage, or commit them.
