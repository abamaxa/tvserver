mod common;

use crate::common::get_media_store;
use anyhow::Result;
use std::env;
use tvserver::services::remote_player::RemotePlayerService;
use tvserver::{
    domain::models::{DownloadableItem, SearchResults},
    domain::{enums::SearchEngineType, GOOGLE_KEY},
    entrypoints,
};

#[tokio::test]
async fn test_search() -> Result<()> {
    env::set_var(GOOGLE_KEY, "");

    let context = entrypoints::Context::from(
        get_media_store(),
        common::get_json_fetcher("tests/fixtures/yt_search.json").await,
        common::get_text_fetcher("tests/fixtures/pb_search.html").await,
        RemotePlayerService::new(),
        None,
    );

    let server = common::create_server(context, 57180).await;

    test_youtube().await?;

    test_pirate_bay().await?;

    server.abort();

    Ok(())
}

async fn test_youtube() -> Result<()> {
    let body = reqwest::get("http://localhost:57180/search/youtube?q=lord+of+the+rings")
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

    Ok(())
}

async fn test_pirate_bay() -> Result<()> {
    let body = reqwest::get("http://localhost:57180/search/pirate?q=dragons+den")
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

    Ok(())
}
