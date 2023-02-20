use tokio::task::{self, JoinHandle};
use tokio::time::{sleep, Duration};
use crate::adaptors::{TorrentDaemon, TransmissionDaemon};
use crate::domain::models::{DownloadProgress, TorrentListResults};


pub fn monitor_downloads() -> JoinHandle<()> {
    task::spawn(async move {
        println!("starting download monitor");
        let torrent_service: &dyn TorrentDaemon  = &TransmissionDaemon::new();

        loop {
            match torrent_service.list().await {
                Ok(results) => handle_results(&results),
                Err(e) => println!("download monitor could not read daemon: {:?}", e.error),
            }

            sleep(Duration::from_secs(3)).await;
        }
    })
}

fn handle_results(results: &TorrentListResults) {
    if let Some(err) = &results.error {
        println!("download monitor got an error from the daemon: {}", err);
        return;
    }

    if let Some(items) = &results.results {
        move_completed_downloads(items);
    }
}

fn move_completed_downloads(items: &Vec<DownloadProgress>) {
    for item in items {
        if item.has_finished_downloading() {
            item.move_files_to_movie_folder();
            item.delete();
        }
    }
}