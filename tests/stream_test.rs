#![cfg(feature = "webserver")]

mod common;

use crate::common::{get_context, get_repository, get_task_manager};
use anyhow::Result;
use app_lib::entrypoints::register;
use common::{get_checker, get_pirate_search};
use axum::{routing::get, Router};
use reqwest::{
    header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, RANGE},
    StatusCode,
};
use std::env;
use std::sync::Arc;
use app_lib::adaptors::{FileSystemStore, SqlRepository};
use app_lib::domain::config::{get_movie_dir, get_thumbnail_dir, MOVIE_DIR};
use app_lib::domain::traits::{FileStorer, Repository};
use app_lib::services::MediaStore;
use tower_http::services::ServeDir;

const TEST_MOVIR_DIR: &str = "tests/fixtures/media_dir";

#[tokio::test]
async fn test_video_stream_supports_byte_ranges() -> Result<()> {
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57186/api/stream/test.mp4";
    let fixture: Vec<u8> = (0_u8..=255).collect();

    let full = client.get(url).send().await?;
    assert_eq!(full.status(), StatusCode::OK);
    assert_eq!(full.headers()[ACCEPT_RANGES], "bytes");
    assert_eq!(full.headers()[CONTENT_LENGTH], "256");
    assert_eq!(full.bytes().await?.as_ref(), fixture.as_slice());

    for (range, content_range, expected) in [
        ("bytes=0-100", "bytes 0-100/256", &fixture[0..=100]),
        ("bytes=250-", "bytes 250-255/256", &fixture[250..]),
        ("bytes=-6", "bytes 250-255/256", &fixture[250..]),
    ] {
        let response = client.get(url).header(RANGE, range).send().await?;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(response.headers()[ACCEPT_RANGES], "bytes", "{range}");
        assert_eq!(response.headers()[CONTENT_RANGE], content_range, "{range}");
        assert_eq!(
            response.headers()[CONTENT_LENGTH],
            expected.len().to_string(),
            "{range}"
        );
        assert_eq!(response.bytes().await?.as_ref(), expected, "{range}");
    }

    for range in ["bytes=256-", "bytes=0-1,3-4", "bytes=invalid"] {
        let response = client.get(url).header(RANGE, range).send().await?;
        assert_eq!(
            response.status(),
            StatusCode::RANGE_NOT_SATISFIABLE,
            "{range}"
        );
        assert_eq!(response.headers()[ACCEPT_RANGES], "bytes", "{range}");
        assert_eq!(response.headers()[CONTENT_RANGE], "bytes */256", "{range}");
        if range == "bytes=256-" {
            assert!(response.bytes().await?.is_empty(), "{range}");
        }
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
