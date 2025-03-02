use anyhow::Context;
use async_trait::async_trait;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, TorrentMetadata};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::domain::config;
use crate::domain::messages::{DownloadInfo, DownloadRequest};
use crate::domain::traits::{DownloadProgress, DownloadProgressMonitor, Download};

pub struct TorrentDownload {
    handle: Arc<ManagedTorrent>,
    metadata: RwLock<Option<Arc<TorrentMetadata>>>,
}

#[async_trait]
impl DownloadProgress for TorrentDownload {
    fn terminate(&self) {
        //self.handle.
        todo!()
    }

    async fn observe(&self) -> DownloadInfo {
        let stats = self.handle.stats();
        let metadata = self.metadata.read().await;  
        let files = match metadata.as_ref() {
            Some(metadata) => metadata.file_infos.iter().map(|f| f.relative_filename.to_string_lossy().to_string()).collect(),
            None => vec![],
        };

        let progress_message = match stats.live {
            Some(live) => format!("{}", live),
            None => "".to_string(),
        };
        DownloadInfo {
            total_size: Some(stats.total_bytes as i64),
            downloaded_size: stats.progress_bytes as i64,
            uploaded_size: Some(stats.uploaded_bytes as i64),
            finished: stats.finished,
            error_message: stats.error.unwrap_or("".to_string()),
            progress_message: progress_message,
            files: files,
        }
    }
}

impl TorrentDownload {
    pub fn new(handle: Arc<ManagedTorrent>) -> Self {
        Self { handle, metadata: RwLock::new(None) }
    }

    pub async fn download(&self) -> Result<(), anyhow::Error> {
        self.handle.with_metadata(|r| {
            tracing::info!("Details: {:?}", &r.info);
        })?;

        // Wait until the download is completed
        self.handle.wait_until_completed().await?;
        tracing::info!("torrent downloaded");

        Ok(())
    }
}

pub struct TorrentFetcher {
    client: Arc<Session>,
}

#[async_trait]
impl Download for TorrentFetcher {
    async fn download(
        &self,
        request: DownloadRequest,
    ) -> Result<DownloadProgressMonitor, anyhow::Error> {
        let handle = self.get_handle(&request.link).await?;

        let downloader = Arc::new(TorrentDownload::new(handle));

        tokio::spawn({
            let downloader = downloader.clone();
            async move {
                if let Err(err) = downloader.download().await {
                    tracing::error!("error downloading torrent: {:?}", err);
                }
            }
        });

        Ok(downloader)
    }
}

impl TorrentFetcher {
    #[allow(clippy::new_without_default)]
    pub async fn new() -> Self {
        let client = Session::new(PathBuf::from(config::get_downloads_dir()))
            .await
            .unwrap();
        TorrentFetcher {client}
    }

    async fn get_handle(&self, link: &str) -> Result<Arc<ManagedTorrent>, anyhow::Error> {
        // Add the torrent to the session
        match self.client
            .add_torrent(
                AddTorrent::from_url(link),
                Some(AddTorrentOptions {
                    // Allow writing on top of existing files.
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .context("error adding torrent")?
        {
            AddTorrentResponse::Added(_, handle) => Ok(handle),
            // For a brand new session other variants won't happen.
            _ => unreachable!(),
        }
    }
}
