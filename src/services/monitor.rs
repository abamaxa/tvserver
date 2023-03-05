use super::torrents::TransmissionDaemon;
use crate::domain::models::{DownloadListResults, DownloadProgress};
use crate::domain::traits::{DownloadClient, MediaStorer};
use std::sync::Arc;
use tokio::task::{self, JoinHandle};
use tokio::time::{sleep, Duration};

pub fn monitor_downloads(store: Arc<dyn MediaStorer>) -> JoinHandle<()> {
    task::spawn(async move {
        println!("starting download monitor");
        let torrent_service: &dyn DownloadClient = &TransmissionDaemon::new();

        loop {
            match torrent_service.list().await {
                Ok(results) => handle_results(&results, &store).await,
                Err(e) => println!("download monitor could not read daemon: {:?}", e.error),
            }

            sleep(Duration::from_secs(3)).await;
        }
    })
}

async fn handle_results(results: &DownloadListResults, store: &Arc<dyn MediaStorer>) {
    if let Some(err) = &results.error {
        println!("download monitor got an error from the daemon: {}", err);
        return;
    }

    if let Some(items) = &results.results {
        move_completed_downloads(items, store).await;
    }
}

async fn move_completed_downloads(items: &[DownloadProgress], store: &Arc<dyn MediaStorer>) {
    let torrent_daemon: &dyn DownloadClient = &TransmissionDaemon::new();
    for item in items.iter().filter(|item| item.has_finished_downloading()) {
        match item.move_videos(store).await {
            Ok(_) => {
                if let Err(e) = torrent_daemon.delete(item.id, true).await {
                    println!("could not delete torrent {}, error: {}", item.name, e);
                }
            }
            Err(e) => println!("could not move videos: {}", e),
        }
    }
}
