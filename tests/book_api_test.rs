#![cfg(feature = "webserver")]

mod common;

use std::{
    env,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use app_lib::{
    adaptors::SqlRepository,
    domain::{
        config::MOVIE_DIR,
        messages::Response,
        models::{
            default_book_thumbnail_bytes, ensure_default_book_thumbnail, BookCollectionDetails,
            BookDetails, BookFormat, BookState, DEFAULT_BOOK_THUMBNAIL,
        },
        traits::Repository,
    },
};
use chrono::Local;
use reqwest::{header::CONTENT_TYPE, Method, StatusCode};
use serde_json::Value;
use sqlx::{Connection, SqliteConnection};
use tokio::{fs, task::JoinHandle};
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::common::{
    get_book_services_at, get_checker, get_context_with_book_services, get_media_store,
    get_pirate_search, get_task_manager,
};

const BOOK_ROOT: &str = "tests/fixtures/book_dir";
const BOOK_THUMBNAIL_ROOT: &str = "tests/fixtures/book_dir/.thumbnails";
const MOVIE_ROOT: &str = "tests/fixtures/media_dir";

fn sample_book(checksum: i64, collection: &str, file_name: &str) -> BookDetails {
    let now = Local::now().naive_local();
    BookDetails {
        file_name: file_name.to_string(),
        collection: collection.to_string(),
        title: file_name
            .trim_end_matches(".epub")
            .trim_end_matches(".pdf")
            .to_string(),
        authors: vec!["Test Author".to_string()],
        format: if file_name.ends_with(".pdf") {
            BookFormat::Pdf
        } else {
            BookFormat::Epub
        },
        thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
        checksum,
        state: BookState::Ready,
        created_on: now,
        updated_on: now,
        ..BookDetails::default()
    }
}

fn epub_fixture() -> Result<Vec<u8>> {
    let mut epub = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    epub.start_file("mimetype", stored)?;
    epub.write_all(b"application/epub+zip")?;

    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    epub.start_file("META-INF/container.xml", deflated)?;
    epub.write_all(
        br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )?;
    epub.start_file("EPUB/content.opf", deflated)?;
    epub.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">reserved-characters</dc:identifier>
    <dc:title>Reserved Characters</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest/>
  <spine/>
</package>"#,
    )?;

    Ok(epub.finish()?.into_inner())
}

async fn start_server(port: u16) -> Result<(JoinHandle<Result<()>>, Repository)> {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    start_server_with_repository(
        port,
        repository,
        Path::new(BOOK_ROOT),
        Path::new(BOOK_THUMBNAIL_ROOT),
    )
    .await
}

async fn start_server_with_repository(
    port: u16,
    repository: Repository,
    book_root: &Path,
    book_thumbnail_root: &Path,
) -> Result<(JoinHandle<Result<()>>, Repository)> {
    env::set_var(MOVIE_DIR, MOVIE_ROOT);

    let book_runtime =
        get_book_services_at(repository.clone(), book_root, book_thumbnail_root).await;
    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;
    let context = get_context_with_book_services(
        get_media_store(),
        searcher,
        get_task_manager(),
        repository.clone(),
        get_checker(),
        book_runtime,
    )
    .await?;

    Ok((
        common::create_server(context, port).await,
        repository,
    ))
}

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(label: &str, port: u16) -> Result<Self> {
        let path = env::temp_dir().join(format!(
            "tvserver-book-api-{label}-{}-{port}",
            std::process::id()
        ));
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn assert_json_error(
    response: reqwest::Response,
    status: StatusCode,
    message: &str,
) -> Result<()> {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
    let result: Response = response.json().await?;
    assert!(result.message.is_empty());
    assert_eq!(result.errors, [message]);
    Ok(())
}

#[tokio::test]
async fn book_progress_lifecycle_round_trips_lists_replaces_and_resets() -> Result<()> {
    let (server, repository) = start_server(57214).await?;
    repository
        .save_book(&sample_book(i64::MAX, "", "largest.epub"))
        .await?;
    repository
        .save_book(&sample_book(2, "", "second.pdf"))
        .await?;
    let client = reqwest::Client::builder().no_proxy().build()?;

    let empty = client
        .get(format!("http://localhost:57214/api/book/{}/progress", i64::MAX))
        .send()
        .await?;
    assert_eq!(empty.status(), StatusCode::NO_CONTENT);
    assert!(empty.bytes().await?.is_empty());

    let largest_response = client
        .put(format!("http://localhost:57214/api/book/{}/progress", i64::MAX))
        .json(&serde_json::json!({
            "locator": { "type": "epub-cfi", "value": "epubcfi(/6/4!/4/2/8)" },
            "progression": 0.42
        }))
        .send()
        .await?;
    assert_eq!(largest_response.status(), StatusCode::OK);
    let largest: Value = largest_response.json().await?;
    assert_eq!(largest["checksum"], i64::MAX.to_string());
    assert_eq!(largest["locator"]["type"], "epub-cfi");
    assert_eq!(largest["locator"]["value"], "epubcfi(/6/4!/4/2/8)");
    assert_eq!(largest["progression"], 0.42);
    assert!(largest["updatedOn"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));

    let second_response = client
        .put("http://localhost:57214/api/book/2/progress")
        .json(&serde_json::json!({
            "locator": { "type": "pdf-page", "value": "7" }
        }))
        .send()
        .await?;
    assert_eq!(second_response.status(), StatusCode::OK);

    let list_response = client
        .get("http://localhost:57214/api/book-progress")
        .send()
        .await?;
    assert_eq!(list_response.status(), StatusCode::OK);
    let listed: Value = list_response.json().await?;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(listed[0]["checksum"], "2");
    assert_eq!(
        listed[0]["locator"],
        serde_json::json!({ "type": "pdf-page", "value": "7" })
    );
    assert!(listed[0].get("progression").is_none());
    assert_eq!(listed[1]["checksum"], i64::MAX.to_string());

    let replacement_response = client
        .put(format!("http://localhost:57214/api/book/{}/progress", i64::MAX))
        .json(&serde_json::json!({
            "locator": { "type": "pdf-page", "value": "99" }
        }))
        .send()
        .await?;
    assert_eq!(replacement_response.status(), StatusCode::OK);
    let replacement: Value = replacement_response.json().await?;
    assert_eq!(
        replacement["locator"],
        serde_json::json!({ "type": "pdf-page", "value": "99" })
    );
    assert!(replacement.get("progression").is_none());

    let retrieved_response = client
        .get(format!("http://localhost:57214/api/book/{}/progress", i64::MAX))
        .send()
        .await?;
    assert_eq!(retrieved_response.status(), StatusCode::OK);
    let retrieved: Value = retrieved_response.json().await?;
    assert_eq!(retrieved, replacement);

    for _ in 0..2 {
        let reset = client
            .delete(format!("http://localhost:57214/api/book/{}/progress", i64::MAX))
            .send()
            .await?;
        assert_eq!(reset.status(), StatusCode::NO_CONTENT);
        assert!(reset.bytes().await?.is_empty());
    }
    let after_reset = client
        .get(format!("http://localhost:57214/api/book/{}/progress", i64::MAX))
        .send()
        .await?;
    assert_eq!(after_reset.status(), StatusCode::NO_CONTENT);

    Ok(server.abort())
}

#[tokio::test]
async fn book_progress_rejects_invalid_input_and_unknown_books_with_json_errors() -> Result<()> {
    let (server, repository) = start_server(57215).await?;
    repository
        .save_book(&sample_book(7, "", "valid.epub"))
        .await?;
    let client = reqwest::Client::builder().no_proxy().build()?;

    for method in [Method::GET, Method::PUT, Method::DELETE] {
        for checksum in ["not-a-number", "9223372036854775808", "%FF"] {
            assert_json_error(
                client
                    .request(
                        method.clone(),
                        format!("http://localhost:57215/api/book/{checksum}/progress"),
                    )
                    .header(CONTENT_TYPE, "application/json")
                    .body(r#"{"locator":{"type":"pdf-page","value":"1"}}"#)
                    .send()
                    .await?,
                StatusCode::BAD_REQUEST,
                "invalid book checksum",
            )
            .await?;
        }
    }

    for method in [Method::GET, Method::PUT, Method::DELETE] {
        assert_json_error(
            client
                .request(method, "http://localhost:57215/api/book/404/progress")
                .header(CONTENT_TYPE, "application/json")
                .body(r#"{"locator":{"type":"pdf-page","value":"1"}}"#)
                .send()
                .await?,
            StatusCode::NOT_FOUND,
            "book not found",
        )
        .await?;
    }

    for (body, message) in [
        (
            serde_json::json!({ "locator": { "type": "future", "value": "1" } }),
            "invalid book locator type",
        ),
        (
            serde_json::json!({ "locator": { "type": "pdf-page", "value": "  " } }),
            "book locator value must not be blank",
        ),
        (
            serde_json::json!({ "locator": { "type": "pdf-page", "value": "1" }, "progression": 1.01 }),
            "book progression must be finite and between 0 and 1",
        ),
    ] {
        assert_json_error(
            client
                .put("http://localhost:57215/api/book/7/progress")
                .json(&body)
                .send()
                .await?,
            StatusCode::BAD_REQUEST,
            message,
        )
        .await?;
    }

    for body in [
        r#"{"locator":{"type":"pdf-page","value":"1"}"#.to_string(),
        r#"{"locator":{"type":"pdf-page","value":"1"},"progression":"half"}"#.to_string(),
    ] {
        assert_json_error(
            client
                .put("http://localhost:57215/api/book/7/progress")
                .header(CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .await?,
            StatusCode::BAD_REQUEST,
            "invalid request body",
        )
        .await?;
    }

    let oversized = format!(
        r#"{{"locator":{{"type":"pdf-page","value":"{}"}}}}"#,
        "x".repeat(2 * 1024 * 1024)
    );
    assert_json_error(
        client
            .put("http://localhost:57215/api/book/7/progress")
            .header(CONTENT_TYPE, "application/json")
            .body(oversized)
            .send()
            .await?,
        StatusCode::PAYLOAD_TOO_LARGE,
        "request body too large",
    )
    .await?;

    Ok(server.abort())
}

#[tokio::test]
async fn book_progress_cascades_after_full_book_deletion() -> Result<()> {
    let temp_root = TempRoot::new("progress-cascade", 57216)?;
    let book_root = temp_root.0.join("books");
    let book_thumbnail_root = book_root.join(".thumbnails");
    let book_path = book_root.join("Delete/delete-progress.epub");
    fs::create_dir_all(book_path.parent().unwrap()).await?;
    fs::write(&book_path, b"delete progress fixture").await?;
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    repository
        .save_book(&sample_book(16, "Delete", "delete-progress.epub"))
        .await?;
    let (server, _) =
        start_server_with_repository(57216, repository, &book_root, &book_thumbnail_root).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;

    client
        .put("http://localhost:57216/api/book/16/progress")
        .json(&serde_json::json!({
            "locator": { "type": "epub-cfi", "value": "epubcfi(/6/2)" },
            "progression": 0.5
        }))
        .send()
        .await?
        .error_for_status()?;
    client
        .delete("http://localhost:57216/api/book/16")
        .send()
        .await?
        .error_for_status()?;

    let listed: Value = client
        .get("http://localhost:57216/api/book-progress")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(listed, serde_json::json!([]));
    assert_json_error(
        client
            .get("http://localhost:57216/api/book/16/progress")
            .send()
            .await?,
        StatusCode::NOT_FOUND,
        "book not found",
    )
    .await?;

    Ok(server.abort())
}

#[tokio::test]
async fn book_progress_repository_errors_are_sanitized() -> Result<()> {
    let temp_root = TempRoot::new("progress-repository-error", 57217)?;
    let database_path = temp_root.0.join("books.sqlite");
    let database_url = format!("sqlite://{}", database_path.display());
    let repository: Repository = Arc::new(SqlRepository::new(&database_url, None).await?);
    repository
        .save_book(&sample_book(17, "", "repository-error.epub"))
        .await?;
    let mut connection = SqliteConnection::connect(&database_url).await?;
    sqlx::query("DROP TABLE book_progress")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let (server, _) = start_server_with_repository(
        57217,
        repository,
        Path::new(BOOK_ROOT),
        Path::new(BOOK_THUMBNAIL_ROOT),
    )
    .await?;
    let client = reqwest::Client::builder().no_proxy().build()?;

    for (method, path) in [
        (Method::GET, "/api/book-progress"),
        (Method::GET, "/api/book/17/progress"),
        (Method::PUT, "/api/book/17/progress"),
        (Method::DELETE, "/api/book/17/progress"),
    ] {
        let response = client
            .request(method, format!("http://localhost:57217{path}"))
            .header(CONTENT_TYPE, "application/json")
            .body(r#"{"locator":{"type":"pdf-page","value":"1"}}"#)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR, "{path}");
        let bytes = response.bytes().await?;
        let result: Response = serde_json::from_slice(&bytes)?;
        assert_eq!(result.errors, ["internal server error"], "{path}");
        assert!(
            !String::from_utf8_lossy(&bytes).contains("no such table"),
            "{path}"
        );
    }

    Ok(server.abort())
}

#[tokio::test]
async fn lists_root_and_nested_book_collections() -> Result<()> {
    let (server, repository) = start_server(57200).await?;
    repository
        .save_book(&sample_book(100, "", "root.epub"))
        .await?;
    repository
        .save_book(&sample_book(101, "Nonfiction/Programming", "static-book.epub"))
        .await?;
    repository
        .save_book(&sample_book(102, "Nonfiction/History", "history.pdf"))
        .await?;

    let root: BookCollectionDetails = reqwest::get("http://localhost:57200/api/books")
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(root.collection, "");
    assert_eq!(root.books.len(), 1);
    assert_eq!(root.books[0].checksum, 100);
    assert_eq!(root.child_collections.len(), 1);
    assert_eq!(root.child_collections[0].collection, "Nonfiction");
    assert_eq!(
        root.child_collections[0].thumbnail,
        "/api/book-thumbnails/default-book.jpg"
    );

    let nested: BookCollectionDetails =
        reqwest::get("http://localhost:57200/api/books/Nonfiction/Programming")
            .await?
            .error_for_status()?
            .json()
            .await?;
    assert_eq!(nested.collection, "Nonfiction/Programming");
    assert_eq!(nested.books.len(), 1);
    assert_eq!(nested.books[0].checksum, 101);

    Ok(server.abort())
}

#[tokio::test]
async fn gets_one_book_with_a_string_checksum() -> Result<()> {
    let (server, repository) = start_server(57201).await?;
    repository
        .save_book(&sample_book(i64::MAX, "Nonfiction", "largest.pdf"))
        .await?;

    let response = reqwest::get(format!("http://localhost:57201/api/book/{}", i64::MAX)).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let book: Value = response.json().await?;
    assert_eq!(book["checksum"], i64::MAX.to_string());
    assert_eq!(book["fileName"], "largest.pdf");
    assert_eq!(book["url"], "/api/books/download/Nonfiction/largest.pdf");

    Ok(server.abort())
}

#[tokio::test]
async fn invalid_book_checksums_return_json_bad_request() -> Result<()> {
    let (server, _) = start_server(57212).await?;
    let client = reqwest::Client::new();

    for method in [Method::GET, Method::DELETE] {
        for checksum in ["not-a-number", "9223372036854775808", "%FF"] {
            let response = client
                .request(
                    method.clone(),
                    format!("http://localhost:57212/api/book/{checksum}"),
                )
                .send()
                .await?;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
            let result: Response = response.json().await?;
            assert!(result.message.is_empty());
            assert_eq!(result.errors, ["invalid book checksum"]);
        }
    }

    Ok(server.abort())
}

#[tokio::test]
async fn missing_book_get_returns_not_found() -> Result<()> {
    let (server, _) = start_server(57205).await?;
    let response = reqwest::Client::new()
        .get("http://localhost:57205/api/book/404")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let result: Response = response.json().await?;
    assert!(result.message.is_empty());
    assert_eq!(result.errors, ["book not found"]);

    Ok(server.abort())
}

#[tokio::test]
async fn missing_book_delete_returns_not_found() -> Result<()> {
    let (server, _) = start_server(57209).await?;

    let response = reqwest::Client::new()
        .delete("http://localhost:57209/api/book/404")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let result: Response = response.json().await?;
    assert!(result.message.is_empty());
    assert_eq!(result.errors, ["book not found"]);

    Ok(server.abort())
}

#[tokio::test]
async fn repository_errors_return_internal_server_error() -> Result<()> {
    let temp_root = TempRoot::new("repository-error", 57206)?;
    let database_path = temp_root.0.join("books.sqlite");
    let database_url = format!("sqlite://{}", database_path.display());
    let repository: Repository = Arc::new(SqlRepository::new(&database_url, None).await?);
    let mut connection = SqliteConnection::connect(&database_url).await?;
    sqlx::query("DROP TABLE books")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let (server, _) = start_server_with_repository(
        57206,
        repository,
        Path::new(BOOK_ROOT),
        Path::new(BOOK_THUMBNAIL_ROOT),
    )
    .await?;

    let client = reqwest::Client::new();
    let list_response = client
        .get("http://localhost:57206/api/books")
        .send()
        .await?;
    assert_eq!(list_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let list_result: BookCollectionDetails = list_response.json().await?;
    assert_eq!(list_result.errors, ["internal server error"]);
    assert!(!format!("{list_result:?}").contains("no such table"));

    let get_response = client
        .get("http://localhost:57206/api/book/404")
        .send()
        .await?;
    assert_eq!(get_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let get_result: Response = get_response.json().await?;
    assert_eq!(get_result.errors, ["internal server error"]);
    assert!(!format!("{get_result:?}").contains("no such table"));

    let delete_response = client
        .delete("http://localhost:57206/api/book/404")
        .send()
        .await?;
    assert_eq!(delete_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let delete_result: Response = delete_response.json().await?;
    assert_eq!(delete_result.errors, ["internal server error"]);
    assert!(!format!("{delete_result:?}").contains("no such table"));

    server.abort();
    Ok(())
}

#[tokio::test]
async fn deletes_book_file_generated_thumbnail_and_database_row() -> Result<()> {
    let temp_root = TempRoot::new("delete", 57202)?;
    let book_root = temp_root.0.join("books");
    let book_thumbnail_root = book_root.join(".thumbnails");
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    let (server, repository) =
        start_server_with_repository(57202, repository, &book_root, &book_thumbnail_root).await?;
    let book_path = book_root.join("Delete/delete-me.epub");
    let thumbnail_path = book_thumbnail_root.join("delete-me.jpg");
    fs::create_dir_all(book_path.parent().unwrap()).await?;
    fs::create_dir_all(&book_thumbnail_root).await?;
    fs::write(&book_path, b"delete book fixture").await?;
    fs::write(&thumbnail_path, b"delete thumbnail fixture").await?;
    ensure_default_book_thumbnail(&book_thumbnail_root)?;

    let mut book = sample_book(103, "Delete", "delete-me.epub");
    book.thumbnail = "delete-me.jpg".to_string();
    repository.save_book(&book).await?;

    let response = reqwest::Client::new()
        .delete("http://localhost:57202/api/book/103")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let result: Response = response.json().await?;
    assert_eq!(result.message, "success");
    assert!(result.errors.is_empty());
    assert!(!book_path.exists());
    assert!(!thumbnail_path.exists());
    assert!(repository.retrieve_book(103).await.is_err());
    assert!(book_thumbnail_root.join(DEFAULT_BOOK_THUMBNAIL).exists());

    Ok(server.abort())
}

#[tokio::test]
async fn serves_nested_book_downloads_from_book_dir() -> Result<()> {
    let (server, _) = start_server(57203).await?;
    let client = reqwest::Client::new();
    let url = "http://localhost:57203/api/books/download/Nonfiction/Programming/static-book.epub";

    let response = client.get(url).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await?.as_ref(), b"static epub fixture\n");

    let public_response = client
        .get(url)
        .header("X-Real-IP", "8.8.8.8")
        .send()
        .await?;
    assert_eq!(public_response.status(), StatusCode::UNAUTHORIZED);

    Ok(server.abort())
}

#[tokio::test]
async fn serves_book_downloads_with_percent_encoded_url_segments() -> Result<()> {
    let temp_root = TempRoot::new("reserved-download", 57211)?;
    let book_root = temp_root.0.join("books");
    let book_thumbnail_root = book_root.join(".thumbnails");
    let collection = "Programming/C# % & Rust";
    let file_name = "100% # & Complete.epub";
    let book_path = book_root.join(collection).join(file_name);
    fs::create_dir_all(book_path.parent().unwrap()).await?;
    let epub = epub_fixture()?;
    fs::write(&book_path, &epub).await?;

    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    repository
        .save_book(&sample_book(104, collection, file_name))
        .await?;
    let (server, _) =
        start_server_with_repository(57211, repository, &book_root, &book_thumbnail_root).await?;

    let client = reqwest::Client::builder().no_proxy().build()?;
    let book: Value = client
        .get("http://localhost:57211/api/book/104")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        book["url"],
        "/api/books/download/Programming/C%23%20%25%20%26%20Rust/100%25%20%23%20%26%20Complete.epub"
    );

    let response = client
        .get(format!(
            "http://localhost:57211{}",
            book["url"].as_str().unwrap()
        ))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(response.bytes().await?.as_ref(), epub);

    Ok(server.abort())
}

#[tokio::test]
async fn serves_default_and_generated_book_thumbnails() -> Result<()> {
    let temp_root = TempRoot::new("thumbnails", 57204)?;
    let book_root = temp_root.0.join("books");
    let book_thumbnail_root = book_root.join(".thumbnails");
    fs::create_dir_all(&book_thumbnail_root).await?;
    let generated_bytes = default_book_thumbnail_bytes();
    fs::write(
        book_thumbnail_root.join("generated-cover.jpg"),
        generated_bytes,
    )
    .await?;
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    let (server, _) = start_server_with_repository(
        57204,
        repository,
        &book_root,
        &book_thumbnail_root,
    )
    .await?;

    let default_response =
        reqwest::get("http://localhost:57204/api/book-thumbnails/default-book.jpg").await?;
    assert_eq!(default_response.status(), StatusCode::OK);
    assert_eq!(default_response.headers()[CONTENT_TYPE], "image/jpeg");
    assert_eq!(
        default_response.bytes().await?.as_ref(),
        default_book_thumbnail_bytes()
    );

    let generated_response =
        reqwest::get("http://localhost:57204/api/book-thumbnails/generated-cover.jpg").await?;
    assert_eq!(generated_response.status(), StatusCode::OK);
    assert_eq!(generated_response.headers()[CONTENT_TYPE], "image/jpeg");
    assert_eq!(
        generated_response.bytes().await?.as_ref(),
        generated_bytes
    );

    Ok(server.abort())
}
