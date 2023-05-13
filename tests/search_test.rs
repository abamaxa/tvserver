mod common;

use crate::common::{get_media_store, get_pirate_search, get_task_manager, get_youtube_search};
use anyhow::Result;
use tokio::task::JoinHandle;
use tvserver::services::{MessageExchange, SearchService};
use tvserver::{
    domain::models::{DownloadableItem, SearchResults},
    domain::SearchEngineType,
    entrypoints,
};

#[tokio::test]
async fn test_youtube() -> Result<()> {
    let searcher = get_youtube_search("yt_search.json").await;

    let server = make_server(searcher, 57179).await;

    let body = reqwest::get("http://localhost:57179/api/search/youtube?q=lord+of+the+rings")
        .await?
        .text()
        .await?;

    let response: SearchResults<DownloadableItem> = serde_json::from_str(&body)?;

    assert!(response.error.is_none());
    assert!(response.results.is_some());

    let results = response.results.unwrap();

    assert_eq!(results.len(), 5);

    let first = results.first().unwrap();

    assert_eq!(
        first.title,
        "Lord Of The Rings - Soundtrack HD Complete (with links)"
    );
    assert_eq!(first.engine, SearchEngineType::YouTube);
    assert_eq!(first.link, "_SBQvd6vY9s");

    Ok(server.abort())
}

#[tokio::test]
async fn test_pirate_bay() -> Result<()> {
    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let server = make_server(searcher, 57180).await;

    let body = reqwest::get("http://localhost:57180/api/search/pirate?q=dragons+den")
        .await?
        .text()
        .await?;

    let response: SearchResults<DownloadableItem> = serde_json::from_str(&body)?;

    assert!(response.error.is_none());
    assert!(response.results.is_some());

    let results = response.results.unwrap();

    assert_eq!(results.len(), 30);

    let first = results.first().unwrap();

    assert_eq!(first.title, "Dragons Den UK S20E09 1080p HEVC x265-MeGusta");
    assert_eq!(first.engine, SearchEngineType::Torrent);
    assert_eq!(first.link, "magnet:?first-link");

    Ok(server.abort())
}

async fn make_server(searcher: SearchService, port: u16) -> JoinHandle<Result<()>> {
    let context = entrypoints::Context::from(
        get_media_store(),
        searcher,
        MessageExchange::new(),
        None,
        get_task_manager(),
    );

    common::create_server(context, port).await
}
