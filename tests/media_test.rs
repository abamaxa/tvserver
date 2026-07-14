mod common;

use crate::common::{get_context, get_repository, get_task_manager};
use anyhow::Result;
use common::{get_checker, get_pirate_search};
use reqwest::StatusCode;
use app_lib::domain::config::MOVIE_DIR;
use std::env;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use app_lib::adaptors::{FileSystemStore, SqlRepository};
use app_lib::domain::messages::Response;
use app_lib::domain::models::VideoDetails;
use app_lib::domain::traits::{FileStorer, Repository};
use app_lib::services::MediaStore;

const TEST_MOVIR_DIR: &str = "tests/fixtures/media_dir";


#[tokio::test]
async fn test_delete_video() -> Result<()> {
    env::set_var(MOVIE_DIR, TEST_MOVIR_DIR);
    
    let file_storer: FileStorer = Arc::new(FileSystemStore::new("tests/fixtures/media_dir"));

    let repo: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());

    let store = Arc::new(MediaStore::new(file_storer, repo.clone()));

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let context = get_context(
        store,
        searcher,
        get_task_manager(),
        get_repository().await,
        get_checker()
    ).await?;

    let server = common::create_server(context, 57190).await;

    let client = reqwest::Client::new();

    let video_path = Path::new("tests/fixtures/media_dir/collection1/delete_me_99.mp4");

    fs::write(video_path, "some data").await?;

    let mut video = VideoDetails::new(
        "delete_me_99.mp4".to_string(),
        "collection1".to_string(),
        &video_path.to_path_buf(),
        None,
    );
    video.checksum = 99;
    repo.save_video(&video).await?;

    let response = client
        .delete("http://localhost:57190/api/media/99")
        .send()
        .await?;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.text().await?;

    let result: Response = serde_json::from_str(&body)?;

    assert!(result.errors.is_empty());
    assert_eq!(result.message, "success");

    assert!(!video_path.exists());

    Ok(server.abort())
}
