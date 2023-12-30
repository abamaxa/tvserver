mod common;

use crate::common::{
    get_media_store, get_pirate_search, get_repository, get_task_manager, get_youtube_search,
};
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use tvserver::domain::messagebus::MessageExchange;
use tvserver::{domain::messages::Response, entrypoints};

#[tokio::test]
async fn test_local_play() -> Result<()> {
    let search = get_youtube_search("yt_search.json").await;

    let context = entrypoints::Context::new(
        get_media_store(),
        search,
        MessageExchange::new(),
        Some(common::get_player()),
        get_task_manager(),
        get_repository().await,
    );

    let server = common::create_server(context, 57181).await;

    let client = reqwest::Client::new();

    let mut map = HashMap::new();
    map.insert("collection", "some collection");
    map.insert("video", "video.mp4");
    map.insert("remote_address", "");

    let body = client
        .post("http://localhost:57181/api/vlc/play")
        .json(&map)
        .send()
        .await?
        .text()
        .await?;

    let response: Response = serde_json::from_str(&body)?;

    assert!(response.errors.is_empty());
    assert_eq!(response.message, "add file:///some collection/video.mp4");

    Ok(server.abort())
}

#[tokio::test]
async fn test_remote_play() -> Result<()> {
    let exchange = MessageExchange::new();

    let key = SocketAddr::from_str("0.0.0.0:456").unwrap();

    exchange.add_player(key, common::get_remote_player()).await;

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = entrypoints::Context::new(
        get_media_store(),
        searcher,
        exchange,
        None,
        get_task_manager(),
        get_repository().await,
    );

    let server = common::create_server(context, 57182).await;

    let client = reqwest::Client::new();

    let mut map = HashMap::new();
    map.insert("collection", "");
    map.insert("video", "test.mp4");
    map.insert("remote_address", "");

    let body = client
        .post("http://localhost:57182/api/remote/play")
        .json(&map)
        .send()
        .await?
        .text()
        .await?;

    let response: Response = serde_json::from_str(&body)?;

    assert!(response.errors.is_empty());
    assert_eq!(response.message, "success");

    Ok(server.abort())
}
