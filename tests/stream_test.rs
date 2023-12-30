mod common;

use crate::common::{get_repository, get_task_manager};
use anyhow::Result;
use common::get_pirate_search;
use reqwest::{header::RANGE, StatusCode};
use std::env;
use std::sync::Arc;
use tokio::sync::broadcast;
use tvserver::adaptors::{FileSystemStore, SqlRepository};
use tvserver::domain::config::MOVIE_DIR;
use tvserver::domain::messagebus::MessageExchange;
use tvserver::domain::traits::{FileStorer, Repository};
use tvserver::entrypoints::Context;
use tvserver::services::MediaStore;

const TEST_MOVIR_DIR: &str = "tests/fixtures/media_dir";

#[tokio::test]
async fn test_video_stream() -> Result<()> {
    env::set_var(MOVIE_DIR, TEST_MOVIR_DIR);

    let (tx, _rx1) = broadcast::channel(16);

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(TEST_MOVIR_DIR));

    let repo: Repository = Arc::new(SqlRepository::new(":memory:").await.unwrap());

    let store = Arc::new(MediaStore::new(file_storer, repo, tx));

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = Context::new(
        store,
        searcher,
        MessageExchange::new(),
        None,
        get_task_manager(),
        get_repository().await,
    );

    let server = common::create_server(context, 57186).await;

    let client = reqwest::Client::new();

    let response = client
        .get("http://localhost:57186/api/alt-stream/test.mp4")
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
