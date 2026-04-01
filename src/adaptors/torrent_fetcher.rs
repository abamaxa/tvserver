use anyhow::Context;
use async_trait::async_trait;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, TorrentMetadata};
use std::path::{Path, PathBuf};
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
        tracing::info!("terminating torrent download");
        // Note: librqbit does not expose a synchronous cancel API on ManagedTorrent.
        // Logging the termination request; the download will be cleaned up when dropped.
    }

    async fn observe(&self) -> DownloadInfo {
        let stats = self.handle.stats();
        let metadata = self.metadata.read().await;  
        let download_dir = config::get_downloads_dir();
        let files = match metadata.as_ref() {
            Some(m) => {
                let torrent_name = m.name.clone().unwrap_or_default();
                let potential_subdir = Path::new(&download_dir).join(&torrent_name);

                let base_path = if !torrent_name.is_empty() && potential_subdir.is_dir() {
                    potential_subdir
                } else {
                    PathBuf::from(&download_dir)
                };

                m.file_infos.iter().map(|f| {
                    base_path.join(&f.relative_filename).to_string_lossy().to_string()
                }).collect()
            },
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
        {
            let mut meta_data_store = self.metadata.write().await;
            self.handle.with_metadata(|r| {
                meta_data_store.replace(r.clone());
            })?;
        }

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
    pub async fn new() -> Result<Self, anyhow::Error> {
        let downloads_dir = config::get_downloads_dir();
        tracing::info!("Downloads directory: {}", downloads_dir);

        let client = Session::new(PathBuf::from(downloads_dir))
            .await
            .context("failed to create torrent session")?;

        Ok(TorrentFetcher {client})
    }

    async fn get_handle(&self, link: &str) -> Result<Arc<ManagedTorrent>, anyhow::Error> {
        // Add the torrent to the session
        tracing::info!("Attempting to add torrent from link: {}", link);
        
        let result = self.client
            .add_torrent(
                AddTorrent::from_url(link),
                Some(AddTorrentOptions {
                    // Allow writing on top of existing files.
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await;
        
        match result {
            Ok(response) => {
                match response {
                    AddTorrentResponse::Added(_, handle) => {
                        tracing::info!("Torrent added successfully");
                        Ok(handle)
                    },
                    AddTorrentResponse::AlreadyManaged(_, handle) => {
                        tracing::info!("Torrent already exists");
                        Ok(handle)
                    },
                    _ => {
                        tracing::error!("Unexpected response from add_torrent");
                        anyhow::bail!("Unexpected response from add_torrent")
                    }
                }
            },
            Err(err) => {
                tracing::error!("Failed to add torrent from {}: {}", link, err);
                Err(err).context(format!("error adding torrent from link: {}", link))
            }
        }
    }
}
