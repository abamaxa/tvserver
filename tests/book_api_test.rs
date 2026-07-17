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
