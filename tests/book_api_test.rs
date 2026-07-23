#![cfg(feature = "webserver")]

mod common;

use std::{
    env,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
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
use reqwest::{
    header::{
        ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, IF_MODIFIED_SINCE, IF_RANGE,
        IF_UNMODIFIED_SINCE, LAST_MODIFIED, RANGE,
    },
    Method, StatusCode,
};
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

#[tokio::test]
async fn book_progress_put_returns_no_content_and_is_read_through_the_book() -> Result<()> {
    let (server, repository) = start_server(57219).await?;
    repository
        .save_book(&sample_book(42, "", "embedded.epub"))
        .await?;
    let client = reqwest::Client::builder().no_proxy().build()?;

    let saved = client
        .put("http://localhost:57219/api/book/42/progress")
        .json(&serde_json::json!({
            "locator": { "type": "epub-cfi", "value": "epubcfi(/6/4)" },
            "progression": 0.5
        }))
        .send()
        .await?;
    assert_eq!(saved.status(), StatusCode::NO_CONTENT);
    assert!(saved.bytes().await?.is_empty());

    let book: Value = client
        .get("http://localhost:57219/api/book/42")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(
        book["progress"]["locator"],
        serde_json::json!({ "type": "epub-cfi", "value": "epubcfi(/6/4)" })
    );
    assert_eq!(book["progress"]["progression"], 0.5);
    assert!(book["progress"]["updatedOn"]
        .as_str()
        .is_some_and(|value| value.ends_with('Z')));

    for (method, path, expected) in [
        (Method::GET, "/api/book-progress", StatusCode::NOT_FOUND),
        (
            Method::GET,
            "/api/book/42/progress",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            Method::DELETE,
            "/api/book/42/progress",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ] {
        let response = client
            .request(method, format!("http://localhost:57219{path}"))
            .send()
            .await?;
        assert_eq!(response.status(), expected, "{path}");
    }

    Ok(server.abort())
}

#[tokio::test]
async fn book_progress_put_rejects_invalid_locator_and_progression() -> Result<()> {
    let (server, _) = start_server(57220).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;

    for (body, expected_error) in [
        (
            serde_json::json!({
                "locator": { "type": "epub-cfi", "value": " \t" }
            }),
            "book locator value must not be blank",
        ),
        (
            serde_json::json!({
                "locator": { "type": "pdf-page", "value": "1" },
                "progression": -0.01
            }),
            "book progression must be finite and between 0 and 1",
        ),
        (
            serde_json::json!({
                "locator": { "type": "pdf-page", "value": "1" },
                "progression": 1.01
            }),
            "book progression must be finite and between 0 and 1",
        ),
    ] {
        let response = client
            .put("http://localhost:57220/api/book/42/progress")
            .json(&body)
            .send()
            .await?;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
        let result: Response = response.json().await?;
        assert!(result.message.is_empty());
        assert_eq!(result.errors, [expected_error]);
    }

    Ok(server.abort())
}

#[tokio::test]
async fn book_progress_put_returns_not_found_for_an_unknown_book() -> Result<()> {
    let (server, _) = start_server(57221).await?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .put("http://localhost:57221/api/book/404/progress")
        .json(&serde_json::json!({
            "locator": { "type": "pdf-page", "value": "1" }
        }))
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
    let result: Response = response.json().await?;
    assert!(result.message.is_empty());
    assert_eq!(result.errors, ["book not found"]);

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

    let progress_response = client
        .put("http://localhost:57206/api/book/404/progress")
        .json(&serde_json::json!({
            "locator": { "type": "pdf-page", "value": "1" }
        }))
        .send()
        .await?;
    assert_eq!(progress_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let progress_body = progress_response.text().await?;
    assert_eq!(
        serde_json::from_str::<Value>(&progress_body)?,
        serde_json::json!({
            "message": "",
            "errors": ["internal server error"]
        })
    );
    assert!(!progress_body.contains("no such table"));
    assert!(!progress_body.contains("books"));

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
async fn serves_book_download_byte_ranges() -> Result<()> {
    let (server, _) = start_server(57214).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57214/api/books/download/Nonfiction/Programming/static-book.epub";
    let full_bytes = b"static epub fixture\n";

    let full = client.get(url).send().await?;
    assert_eq!(full.status(), StatusCode::OK);
    assert_eq!(full.headers()[ACCEPT_RANGES], "bytes");
    assert_eq!(full.headers()[CONTENT_LENGTH], full_bytes.len().to_string());
    assert_eq!(full.bytes().await?.as_ref(), full_bytes);

    for (range, expected_content_range, expected) in [
        ("bytes=1-3", "bytes 1-3/20", &full_bytes[1..=3]),
        ("bytes=7-", "bytes 7-19/20", &full_bytes[7..]),
        ("bytes=-8", "bytes 12-19/20", &full_bytes[12..]),
    ] {
        let response = client.get(url).header(RANGE, range).send().await?;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(response.headers()[ACCEPT_RANGES], "bytes", "{range}");
        assert_eq!(response.headers()[CONTENT_RANGE], expected_content_range, "{range}");
        assert_eq!(response.headers()[CONTENT_LENGTH], expected.len().to_string(), "{range}");
        assert_eq!(response.bytes().await?.as_ref(), expected, "{range}");
    }

    for range in ["bytes=999-", "bytes=0-1,3-4", "bytes=invalid"] {
        let response = client.get(url).header(RANGE, range).send().await?;
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
        assert_eq!(response.headers()[ACCEPT_RANGES], "bytes", "{range}");
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */20", "{range}");
        assert!(response.bytes().await?.is_empty(), "{range}");
    }

    Ok(server.abort())
}

#[tokio::test]
async fn book_download_ignores_ranges_on_head() -> Result<()> {
    let (server, _) = start_server(57222).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57222/api/books/download/Nonfiction/Programming/static-book.epub";
    let full_bytes = b"static epub fixture\n";

    let response = client.head(url).header(RANGE, "bytes=1-3").send().await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
    assert_eq!(response.headers()[CONTENT_LENGTH], full_bytes.len().to_string());
    assert!(response.headers().get(CONTENT_RANGE).is_none());
    assert!(response.bytes().await?.is_empty());

    Ok(server.abort())
}

#[tokio::test]
async fn book_download_conservatively_ignores_if_range() -> Result<()> {
    let (server, _) = start_server(57223).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57223/api/books/download/Nonfiction/Programming/static-book.epub";
    let full_bytes = b"static epub fixture\n";

    let response = client
        .get(url)
        .header(RANGE, "bytes=1-3")
        .header(IF_RANGE, "\"stale-validator\"")
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
    assert_eq!(response.headers()[CONTENT_LENGTH], full_bytes.len().to_string());
    assert!(response.headers().get(CONTENT_RANGE).is_none());
    assert_eq!(response.bytes().await?.as_ref(), full_bytes);

    Ok(server.abort())
}

#[tokio::test]
async fn book_download_honors_modification_preconditions() -> Result<()> {
    let temp_root = TempRoot::new("conditional-download", 57224)?;
    let book_root = temp_root.0.join("books");
    let book_thumbnail_root = book_root.join(".thumbnails");
    let book_path = book_root.join("Test/conditional.epub");
    fs::create_dir_all(book_path.parent().unwrap()).await?;
    let full_bytes = b"conditional book fixture";
    fs::write(&book_path, full_bytes).await?;

    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    let (server, _) =
        start_server_with_repository(57224, repository, &book_root, &book_thumbnail_root).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57224/api/books/download/Test/conditional.epub";

    let full = client.get(url).send().await?;
    assert_eq!(full.status(), StatusCode::OK);
    let last_modified = full
        .headers()
        .get(LAST_MODIFIED)
        .expect("book response must include Last-Modified")
        .clone();
    assert_eq!(full.bytes().await?.as_ref(), full_bytes);

    let not_modified = client
        .get(url)
        .header(IF_MODIFIED_SINCE, last_modified.clone())
        .send()
        .await?;
    assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    assert!(not_modified.bytes().await?.is_empty());

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    fs::write(&book_path, b"updated conditional book fixture").await?;

    let precondition_failed = client
        .get(url)
        .header(IF_UNMODIFIED_SINCE, last_modified)
        .send()
        .await?;
    assert_eq!(precondition_failed.status(), StatusCode::PRECONDITION_FAILED);
    assert!(precondition_failed.bytes().await?.is_empty());

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
