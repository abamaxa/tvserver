use anyhow::Result;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
use tokio::fs;
use transmission_rpc::types::Torrent;

use app_lib::domain::{messages::DownloadInfo, traits::{Downloader, MockDownload, MockDownloadProgress}};

#[derive(Deserialize)]
pub struct TorrentGetResult {
    pub torrents: Vec<Torrent>,
}

pub async fn get_torrent_downloader(fixture: &str) -> Downloader {
    let mut fetcher = MockDownload::new();
    let mut downloader = MockDownloadProgress::new();

    let download_info = results_from_fixture(fixture).await.unwrap();

    downloader.expect_observe().returning(move || download_info.clone());

    let downloader = Arc::new(downloader);
    
    fetcher
        .expect_download()
        .returning(move |_| Ok(downloader.clone()));

    Arc::new(fetcher)
}


async fn results_from_fixture(name: &str) -> Result<DownloadInfo> {
    let mut path = PathBuf::from("tests/fixtures");
    path.push(name);

    let data = fs::read(&path).await?;

    let result: TorrentGetResult = serde_json::from_slice(&data)?;

    let items: Vec<DownloadInfo> = result
        .torrents
        .iter()
        .map(|t| {
            DownloadInfo {
                //request: DownloadRequest::new(t.name.clone(), t.magnet_link.clone()),
                total_size: t.total_size,
                downloaded_size: 0,
                uploaded_size: Some(0),
                finished: false,
                error_message: t.error_string.clone().unwrap_or("".to_string()),
                progress_message: "".to_string(),
                files: vec![],
            }
        })
        .collect();

    Ok(items.get(0).unwrap().clone())
}
