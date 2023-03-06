use crate::domain::models::{DownloadListResults, DownloadProgress};
use crate::domain::traits::{DownloadClient, MediaStorer};
use std::sync::Arc;
use tokio::task::{self, JoinHandle};
use tokio::time::{sleep, Duration};

pub struct Monitor {
    store: Arc<dyn MediaStorer>,
    downloads: Arc<dyn DownloadClient>,
}

impl Monitor {
    pub fn start(
        store: Arc<dyn MediaStorer>,
        downloads: Arc<dyn DownloadClient>,
    ) -> JoinHandle<()> {
        task::spawn(async move {
            tracing::info!("starting download monitor");
            let monitor = Self { store, downloads };

            loop {
                let results = monitor.downloads.list_in_progress().await;
                if results.results.is_some() {
                    monitor.handle_results(&results).await;
                } else {
                    tracing::error!(
                        "download monitor could not read daemon: {:?}",
                        results.error
                    );
                }

                sleep(Duration::from_secs(3)).await;
            }
        })
    }

    async fn handle_results(&self, results: &DownloadListResults) {
        if let Some(err) = &results.error {
            tracing::error!("download monitor got an error from the daemon: {}", err);
            return;
        }

        if let Some(items) = &results.results {
            self.move_completed_downloads(items).await;
        }
    }

    async fn move_completed_downloads(&self, items: &[DownloadProgress]) {
        for item in items.iter().filter(|item| item.has_finished_downloading()) {
            match item.move_videos(&self.store).await {
                Ok(_) => {
                    if let Err(e) = self.downloads.remove(item.id, true).await {
                        tracing::error!("could not delete torrent {}, error: {}", item.name, e);
                    }
                }
                Err(e) => tracing::error!("could not move videos: {}", e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use mockall::predicate;
    use serde::Deserialize;
    use std::path::PathBuf;
    use tokio::{fs, time};
    use transmission_rpc::types::Torrent;

    use crate::domain::traits::{MockDownloadClient, MockMediaStorer};

    #[derive(Deserialize)]
    pub struct TorrentGetResult {
        pub torrents: Vec<Torrent>,
    }

    #[tokio::test]
    async fn test_download_monitor() -> Result<()> {
        let mut mock_downloader = MockDownloadClient::new();
        let mut counter = 0;
        let list_results = results_from_fixture("torrents_get.json").await?;

        mock_downloader
            .expect_list_in_progress()
            .returning(move || match counter {
                0 => {
                    counter += 1;
                    list_results.clone()
                }
                _ => DownloadListResults::success(vec![]),
            });

        mock_downloader
            .expect_remove()
            .times(1)
            .with(predicate::eq(2i64), predicate::eq(true))
            .returning(|_, _| Ok(()));

        let downloader: Arc<dyn DownloadClient> = Arc::new(mock_downloader);

        let mut mock_store = MockMediaStorer::new();

        mock_store.expect_move_file().times(1).returning(|_| Ok(()));

        let store: Arc<dyn MediaStorer> = Arc::new(mock_store);

        let monitor_handle = Monitor::start(store, downloader);

        // wait for monitor to finish a cycle, if it hasn't finished by then it ought
        // to be a test fail
        time::sleep(time::Duration::from_millis(100)).await;

        monitor_handle.abort();

        Ok(())
    }

    async fn results_from_fixture(name: &str) -> Result<DownloadListResults> {
        let mut path = PathBuf::from("tests/fixtures");
        path.push(name);

        let data = fs::read(&path).await?;

        let result: TorrentGetResult = serde_json::from_slice(&data)?;

        let items = result.torrents.iter().map(DownloadProgress::from).collect();

        Ok(DownloadListResults::success(items))
    }
}
