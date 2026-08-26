#![cfg(feature = "webserver")]

mod common;

use crate::common::{get_context, get_repository, get_task_manager};
use anyhow::Result;
use app_lib::adaptors::{FileSystemStore, SqlRepository};
use app_lib::domain::config::{get_movie_dir, get_thumbnail_dir, MOVIE_DIR};
use app_lib::domain::traits::{FileStorer, Repository};
use app_lib::entrypoints::{register, webserver::build_http_router};
use app_lib::services::MediaStore;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode as AxumStatusCode},
    routing::get,
    Router,
};
use common::{get_checker, get_pirate_search};
use reqwest::{
    header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, IF_RANGE, RANGE},
    StatusCode,
};
use std::{env, fs::File, path::PathBuf, sync::Arc};
use tower::ServiceExt;
use tower_http::services::ServeDir;

const TEST_MOVIR_DIR: &str = "tests/fixtures/media_dir";
const MAX_VIDEO_RANGE_BYTES: u64 = 8 * 1024 * 1024;

struct TestFileGuard(PathBuf);

impl Drop for TestFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

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
async fn test_video_stream_caps_oversized_byte_ranges() -> Result<()> {
    env::set_var(MOVIE_DIR, TEST_MOVIR_DIR);
    let fixture_path = PathBuf::from(TEST_MOVIR_DIR).join("large-range-test.mp4");
    let _fixture = TestFileGuard(fixture_path.clone());
    let fixture_size = MAX_VIDEO_RANGE_BYTES + 10;
    let file = File::create(&fixture_path)?;
    file.set_len(fixture_size)?;

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
    )
    .await?;
    let server = common::create_server(context, 57193).await;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57193/api/stream/large-range-test.mp4";

    for range in ["bytes=0-", "bytes=0-8388617"] {
        let response = client.get(url).header(RANGE, range).send().await?;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(
            response.headers()[CONTENT_RANGE],
            format!("bytes 0-{}/{}", MAX_VIDEO_RANGE_BYTES - 1, fixture_size),
            "{range}"
        );
        assert_eq!(
            response.headers()[CONTENT_LENGTH],
            MAX_VIDEO_RANGE_BYTES.to_string(),
            "{range}"
        );
        assert_eq!(
            response.bytes().await?.len(),
            MAX_VIDEO_RANGE_BYTES as usize,
            "{range}"
        );
    }

    let suffix = client
        .get(url)
        .header(RANGE, "bytes=-8388618")
        .send()
        .await?;
    assert_eq!(suffix.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        suffix.headers()[CONTENT_RANGE],
        format!(
            "bytes {}-{}/{}",
            fixture_size - MAX_VIDEO_RANGE_BYTES,
            fixture_size - 1,
            fixture_size
        )
    );
    assert_eq!(
        suffix.headers()[CONTENT_LENGTH],
        MAX_VIDEO_RANGE_BYTES.to_string()
    );
    assert_eq!(suffix.bytes().await?.len(), MAX_VIDEO_RANGE_BYTES as usize);

    Ok(server.abort())
}

#[tokio::test]
async fn test_video_stream_ignores_ranges_on_head() -> Result<()> {
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
    )
    .await?;
    let server = common::create_server(context, 57191).await;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57191/api/stream/test.mp4";

    let response = client.head(url).header(RANGE, "bytes=0-9").send().await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
    assert_eq!(response.headers()[CONTENT_LENGTH], "256");
    assert!(response.headers().get(CONTENT_RANGE).is_none());
    assert!(response.bytes().await?.is_empty());

    Ok(server.abort())
}

#[tokio::test]
async fn test_video_stream_conservatively_ignores_if_range() -> Result<()> {
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
    )
    .await?;
    let server = common::create_server(context, 57192).await;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = "http://localhost:57192/api/stream/test.mp4";
    let fixture: Vec<u8> = (0_u8..=255).collect();

    let response = client
        .get(url)
        .header(RANGE, "bytes=0-9")
        .header(IF_RANGE, "\"stale-validator\"")
        .header("X-Real-IP", "8.8.8.8")
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[ACCEPT_RANGES], "bytes");
    assert_eq!(response.headers()[CONTENT_LENGTH], "256");
    assert!(response.headers().get(CONTENT_RANGE).is_none());
    assert_eq!(response.bytes().await?.as_ref(), fixture.as_slice());

    Ok(server.abort())
}

#[tokio::test]
async fn test_video_stream_router_preserves_prefix_and_normalizes_ranges() -> Result<()> {
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
    )
    .await?;
    let app = build_http_router(context)?;
    let fixture: Vec<u8> = (0_u8..=255).collect();

    let head = app
        .clone()
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/api/stream/test.mp4")
                .header("range", "bytes=0-9")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(head.status(), AxumStatusCode::OK);
    assert_eq!(head.headers()["accept-ranges"], "bytes");
    assert_eq!(head.headers()["content-length"], "256");
    assert!(head.headers().get("content-range").is_none());
    assert!(to_bytes(head.into_body(), usize::MAX).await?.is_empty());

    let if_range = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/stream/test.mp4")
                .header("range", "bytes=0-9")
                .header("if-range", "\"stale-validator\"")
                .header("x-real-ip", "8.8.8.8")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(if_range.status(), AxumStatusCode::OK);
    assert_eq!(if_range.headers()["content-length"], "256");
    assert!(if_range.headers().get("content-range").is_none());
    assert_eq!(
        to_bytes(if_range.into_body(), usize::MAX).await?.as_ref(),
        fixture.as_slice()
    );

    for uri in ["/api/stream", "/api/stream/"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("x-real-ip", "8.8.8.8")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), AxumStatusCode::NOT_FOUND, "{uri}");
    }

    Ok(())
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
