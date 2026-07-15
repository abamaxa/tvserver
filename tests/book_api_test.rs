mod common;

use std::{env, path::Path, sync::Arc};

use anyhow::Result;
use app_lib::{
    adaptors::SqlRepository,
    domain::{
        config::{BOOK_DIR, BOOK_THUMBNAIL_DIR, MOVIE_DIR},
        messages::Response,
        models::{
            default_book_thumbnail_bytes, ensure_default_book_thumbnail, BookCollectionDetails,
            BookDetails, BookFormat, BookState, DEFAULT_BOOK_THUMBNAIL,
        },
        traits::Repository,
    },
};
use chrono::Local;
use reqwest::StatusCode;
use serde_json::Value;
use tokio::{fs, task::JoinHandle};

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

async fn start_server(port: u16) -> Result<(JoinHandle<Result<()>>, Repository)> {
    env::set_var(MOVIE_DIR, MOVIE_ROOT);
    env::set_var(BOOK_DIR, BOOK_ROOT);
    env::set_var(BOOK_THUMBNAIL_DIR, BOOK_THUMBNAIL_ROOT);

    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    let (book_store, book_file_storer) = get_book_services_at(
        repository.clone(),
        Path::new(BOOK_ROOT),
        Path::new(BOOK_THUMBNAIL_ROOT),
    );
    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;
    let context = get_context_with_book_services(
        get_media_store(),
        searcher,
        get_task_manager(),
        repository.clone(),
        get_checker(),
        book_store,
        book_file_storer,
    )
    .await?;

    Ok((common::create_server(context, port).await, repository))
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
async fn deletes_book_file_generated_thumbnail_and_database_row() -> Result<()> {
    let (server, repository) = start_server(57202).await?;
    let book_path = Path::new(BOOK_ROOT).join("Delete/delete-me.epub");
    let thumbnail_path = Path::new(BOOK_THUMBNAIL_ROOT).join("delete-me.jpg");
    fs::create_dir_all(book_path.parent().unwrap()).await?;
    fs::create_dir_all(BOOK_THUMBNAIL_ROOT).await?;
    fs::write(&book_path, b"delete book fixture").await?;
    fs::write(&thumbnail_path, b"delete thumbnail fixture").await?;
    ensure_default_book_thumbnail(BOOK_THUMBNAIL_ROOT)?;

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
    assert!(Path::new(BOOK_THUMBNAIL_ROOT)
        .join(DEFAULT_BOOK_THUMBNAIL)
        .exists());
    fs::remove_dir(book_path.parent().unwrap()).await?;

    Ok(server.abort())
}

#[tokio::test]
async fn serves_nested_book_downloads_from_book_dir() -> Result<()> {
    let (server, _) = start_server(57203).await?;

    let response = reqwest::get(
        "http://localhost:57203/api/books/download/Nonfiction/Programming/static-book.epub",
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await?.as_ref(), b"static epub fixture\n");

    Ok(server.abort())
}

#[tokio::test]
async fn serves_default_and_generated_book_thumbnails() -> Result<()> {
    let (server, _) = start_server(57204).await?;
    ensure_default_book_thumbnail(BOOK_THUMBNAIL_ROOT)?;

    let default_response =
        reqwest::get("http://localhost:57204/api/book-thumbnails/default-book.jpg").await?;
    assert_eq!(default_response.status(), StatusCode::OK);
    assert_eq!(
        default_response.bytes().await?.as_ref(),
        default_book_thumbnail_bytes()
    );

    let generated_response =
        reqwest::get("http://localhost:57204/api/book-thumbnails/generated-cover.jpg").await?;
    assert_eq!(generated_response.status(), StatusCode::OK);
    assert_eq!(
        generated_response.bytes().await?.as_ref(),
        b"generated thumbnail fixture\n"
    );

    Ok(server.abort())
}
