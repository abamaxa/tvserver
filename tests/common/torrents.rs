use anyhow::Result;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
use tokio::fs;
use transmission_rpc::types::Torrent;

use tvserver::domain::models::TorrentTask;
use tvserver::domain::traits::{Downloader, MockMediaDownloader, Task};

#[derive(Deserialize)]
pub struct TorrentGetResult {
    pub torrents: Vec<Torrent>,
}

pub async fn get_torrent_downloader(fixture: &str) -> Downloader {
    let mut fetcher = MockMediaDownloader::new();
    let list_results = results_from_fixture(fixture).await.unwrap();

    fetcher
        .expect_fetch()
        .returning(|_, _| Ok("response: ok".to_string()));

    fetcher
        .expect_list_in_progress()
        .returning(move || Ok(list_results.clone()));

    fetcher.expect_remove().return_const(Ok(()));

    Arc::new(fetcher)
}

async fn results_from_fixture(name: &str) -> Result<Vec<Task>> {
    let mut path = PathBuf::from("tests/fixtures");
    path.push(name);

    let data = fs::read(&path).await?;

    let result: TorrentGetResult = serde_json::from_slice(&data)?;

    let items = result
        .torrents
        .iter()
        .map(|t| Arc::new(TorrentTask::from(t)) as Task)
        .collect();

    Ok(items)
}
