use anyhow::Context;
use async_trait::async_trait;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, TorrentMetadata};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
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
        let client = self.client.clone();
        Ok(PendingTorrentDownload::start(async move {
            let fetcher = TorrentFetcher { client };
            let handle = fetcher.get_handle(&request.link).await?;
            let downloader = Arc::new(TorrentDownload::new(handle));
            // add_torrent has resolved metadata before returning the handle.
            let metadata = downloader.handle.with_metadata(Arc::clone)?;
            *downloader.metadata.write().await = Some(metadata);
            Ok(downloader as DownloadProgressMonitor)
        }, Duration::from_secs(300)))
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


// Register the task before magnet metadata resolution, which can wait indefinitely
// for peers. Unknown size prevents the monitor from treating 0/0 as completion.
struct PendingTorrentDownload {
    state: Mutex<Result<Option<DownloadProgressMonitor>, String>>,
    cancellation: CancellationToken,
}

impl PendingTorrentDownload {
    fn start(
        initialize: impl Future<Output = anyhow::Result<DownloadProgressMonitor>> + Send + 'static,
        timeout: Duration,
    ) -> Arc<Self> {
        let download = Arc::new(Self {
            state: Mutex::new(Ok(None)),
            cancellation: CancellationToken::new(),
        });
        let worker = download.clone();
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = worker.cancellation.cancelled() => Err("Torrent initialization cancelled".to_string()),
                result = tokio::time::timeout(timeout, initialize) => match result {
                    Ok(Ok(monitor)) => Ok(Some(monitor)),
                    Ok(Err(err)) => Err(format!("Torrent initialization failed: {err:#}")),
                    Err(_) => Err("Torrent metadata lookup timed out; no metadata received from peers. Check peer availability and network connectivity, then retry.".to_string()),
                }
            };
            if let Err(err) = &result {
                tracing::warn!("{err}");
            }
            let mut state = worker.state.lock().unwrap();
            if worker.cancellation.is_cancelled() {
                if let Ok(Some(monitor)) = result {
                    monitor.terminate();
                }
            } else {
                *state = result;
            }
        });
        download
    }
}

#[async_trait]
impl DownloadProgress for PendingTorrentDownload {
    fn terminate(&self) {
        self.cancellation.cancel();
        let mut state = self.state.lock().unwrap();
        if let Ok(Some(monitor)) = &*state {
            monitor.terminate();
        }
        *state = Err("Torrent download cancelled".into());
    }

    async fn observe(&self) -> DownloadInfo {
        let state = self.state.lock().unwrap().clone();
        if let Ok(Some(monitor)) = &state {
            return monitor.observe().await;
        }
        DownloadInfo {
            total_size: None,
            downloaded_size: 0,
            uploaded_size: None,
            finished: false,
            error_message: state.err().unwrap_or_default(),
            progress_message: "Resolving torrent metadata; waiting for peers".into(),
            files: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unresolved_magnet_is_observable_and_can_be_cancelled() {
        let download = PendingTorrentDownload::start(std::future::pending(), Duration::from_secs(300));
        let info = download.observe().await;
        assert!(!info.finished);
        assert_eq!(info.total_size, None);
        assert!(info.progress_message.contains("waiting for peers"));
        download.terminate();
        assert!(download.observe().await.error_message.contains("cancelled"));
    }

    #[tokio::test]
    async fn unresolved_magnet_reports_timeout() {
        let download = PendingTorrentDownload::start(std::future::pending(), Duration::from_millis(1));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if download.observe().await.error_message.contains("timed out") { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("timeout must be reported");
    }

    #[tokio::test]
    async fn resolved_magnet_delegates_to_download_progress() {
        use crate::domain::traits::MockDownloadProgress;
        let mut progress = MockDownloadProgress::new();
        progress.expect_observe().returning(|| DownloadInfo {
            total_size: Some(100), downloaded_size: 25, uploaded_size: None,
            finished: false, error_message: String::new(), progress_message: "downloading".into(), files: vec![],
        });
        let download = PendingTorrentDownload::start(async move {
            Ok(Arc::new(progress) as DownloadProgressMonitor)
        }, Duration::from_secs(1));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let info = download.observe().await;
                if info.total_size == Some(100) {
                    assert_eq!(info.downloaded_size, 25);
                    break;
                }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
    }

    #[tokio::test]
    async fn initialization_failure_is_reported() {
        let download = PendingTorrentDownload::start(async { anyhow::bail!("invalid magnet") }, Duration::from_secs(1));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if download.observe().await.error_message.contains("invalid magnet") { break; }
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
    }
}
