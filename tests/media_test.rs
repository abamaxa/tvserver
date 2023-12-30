mod common;

use crate::common::{get_repository, get_task_manager};
use anyhow::Result;
use common::get_pirate_search;
use reqwest::StatusCode;
use tvserver::domain::config::MOVIE_DIR;
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::broadcast;
use tvserver::adaptors::{FileSystemStore, SqlRepository};
use tvserver::domain::messages::Response;
use tvserver::domain::messagebus::MessageExchange;
use tvserver::domain::traits::{FileStorer, Repository};
use tvserver::entrypoints::Context;
use tvserver::services::MediaStore;

const TEST_MOVIR_DIR: &str = "tests/fixtures/media_dir";


#[tokio::test]
async fn test_rename_video() -> Result<()> {
    env::set_var(MOVIE_DIR, TEST_MOVIR_DIR);
    
    let file_storer: FileStorer = Arc::new(FileSystemStore::new("tests/fixtures/media_dir"));

    let (tx, _rx1) = broadcast::channel(16);

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

    let server = common::create_server(context, 57190).await;

    let client = reqwest::Client::new();

    let old_path = Path::new("tests/fixtures/media_dir/collection1/old_name_99.mp4");

    fs::write(old_path, "some data").await?;

    let mut params = HashMap::new();
    params.insert("newName", "collection1/new_name_123.mp4");

    let response = client
        .put("http://localhost:57190/api/media/collection1/old_name_99.mp4")
        .json(&params)
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.text().await?;

    let result: Response = serde_json::from_str(&body)?;

    assert!(result.errors.is_empty());
    assert_eq!(result.message, "collection1/new_name_123.mp4");

    let new_path = Path::new("tests/fixtures/media_dir/collection1/new_name_123.mp4");

    assert!(new_path.exists());

    let response = client
        .delete("http://localhost:57190/api/media/collection1/new_name_123.mp4")
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.text().await?;

    let result: Response = serde_json::from_str(&body)?;
    assert!(result.errors.is_empty());
    assert_eq!(result.message, "collection1/new_name_123.mp4");

    assert!(!new_path.exists());

    Ok(server.abort())
}
