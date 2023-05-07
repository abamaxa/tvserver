mod common;

use crate::common::get_task_manager;
use anyhow::Result;
use common::get_pirate_search;
use reqwest::{header::RANGE, StatusCode};
use std::sync::Arc;
use tvserver::entrypoints::Context;
use tvserver::services::{MediaStore, MessageExchange};

#[tokio::test]
async fn test_video_stream() -> Result<()> {
    let store = Arc::new(MediaStore::from("tests/fixtures/media_dir"));

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = Context::from(store, searcher, MessageExchange::new(), None, get_task_manager());

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
