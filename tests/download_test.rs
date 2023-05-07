mod common;

use crate::common::{get_media_store, get_pirate_search, get_task_manager};
use anyhow::Result;
use reqwest::StatusCode;
use std::collections::HashMap;
use tvserver::services::MessageExchange;
use tvserver::{domain::messages::Response, entrypoints::Context};

#[tokio::test]
async fn test_pirate_download() -> Result<()> {
    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = Context::from(
        get_media_store(),
        searcher,
        MessageExchange::new(),
        None,
        get_task_manager(),
    );

    let server = common::create_server(context, 57185).await;

    let client = reqwest::Client::new();

    let mut map = HashMap::new();
    map.insert("link", "magnet:");
    map.insert("engine", "Torrent");
    map.insert("name", "test");

    let body = client
        .post("http://localhost:57185/api/tasks")
        .json(&map)
        .send()
        .await?
        .text()
        .await?;

    let response: Response = serde_json::from_str(&body)?;

    assert!(response.errors.is_empty());
    assert_eq!(response.message, "response: ok");

    map.insert("engine", "doesn't exist");

    let response = client
        .post("http://localhost:57185/api/tasks")
        .json(&map)
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    Ok(server.abort())
}
