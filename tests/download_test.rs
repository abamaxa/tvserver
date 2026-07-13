mod common;

use crate::common::{get_media_store, get_pirate_search, get_repository, get_task_manager};
use anyhow::Result;
use common::{get_checker, get_context};
use reqwest::StatusCode;
use app_lib::domain::config::MOVIE_DIR;
use std::collections::HashMap;
use std::env;
use app_lib::domain::messages::Response;

const TEST_MOVIR_DIR: &str = "tests/fixtures/media_dir";

#[tokio::test]
async fn test_pirate_download() -> Result<()> {
    env::set_var(MOVIE_DIR, TEST_MOVIR_DIR);
    
    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = get_context(
        get_media_store(),
        searcher,
        get_task_manager(),
        get_repository().await,
        get_checker()
    ).await?;

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
    assert_eq!(response.message, "download queued");

    map.insert("engine", "doesn't exist");

    let response = client
        .post("http://localhost:57185/api/tasks")
        .json(&map)
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    Ok(server.abort())
}
