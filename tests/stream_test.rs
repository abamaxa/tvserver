#![cfg(feature = "webserver")]

mod common;

use crate::common::{get_context, get_repository, get_task_manager};
use anyhow::Result;
use app_lib::entrypoints::register;
use common::{get_checker, get_pirate_search};
use axum::{routing::get, Router};
use reqwest::{header::RANGE, StatusCode};
use std::env;
use std::sync::Arc;
use app_lib::adaptors::{FileSystemStore, SqlRepository};
use app_lib::domain::config::{get_movie_dir, get_thumbnail_dir, MOVIE_DIR};
use app_lib::domain::traits::{FileStorer, Repository};
use app_lib::services::MediaStore;
use tower_http::services::ServeDir;

const TEST_MOVIR_DIR: &str = "tests/fixtures/media_dir";

#[tokio::test]
async fn test_video_stream() -> Result<()> {
    env::set_var(MOVIE_DIR, TEST_MOVIR_DIR);

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(TEST_MOVIR_DIR));

    let repo: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());

    let store = Arc::new(MediaStore::new(file_storer, repo));

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = get_context(
        store,
        searcher,
        get_task_manager(),
        get_repository().await,
        get_checker(),
    ).await?;

    let server = common::create_server(context, 57186).await;

    let client = reqwest::Client::new();

    let response = client
        .get("http://localhost:57186/api/stream/test.mp4")
        .header(RANGE, "bytes=0-100")
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);

    let data = response.bytes().await?;

    for c in 0..data.len() {
        assert_eq!(data[c], c as u8);
    }

    Ok(server.abort())
}

#[tokio::test]
async fn test_webserver_routes_merge_without_duplicate_stream_audio() -> Result<()> {
    env::set_var(MOVIE_DIR, TEST_MOVIR_DIR);

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(TEST_MOVIR_DIR));

    let repo: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());

    let store = Arc::new(MediaStore::new(file_storer, repo));

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = get_context(
        store,
        searcher,
        get_task_manager(),
        get_repository().await,
        get_checker(),
    ).await?;

    let protected_routes = register(Arc::new(context));
    let unprotected_routes = Router::new()
        .route(
            "/api/stream-audio/{audio_index}/{*path}",
            get(app_lib::entrypoints::api::stream_audio),
        )
        .nest_service("/api/stream", ServeDir::new(get_movie_dir()))
        .nest_service(
            "/api/thumbnails",
            ServeDir::new(get_thumbnail_dir(&get_movie_dir())),
        );

    let _app = Router::new()
        .merge(unprotected_routes)
        .merge(protected_routes);

    Ok(())
}
