use crate::domain::algorithm::{classify_media_kind, MediaKind};
use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender, MediaEvent};
use crate::domain::services::{generate_book_metadata, generate_video_metadatas};
use crate::domain::traits::{FileStorer, ProcessSpawner, Repository, Storer};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;

// Create a static semaphore with a capacity of half the number of CPUs
static WORKER_SEMAPHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| {
    let num_cpus = num_cpus::get();
    let concurrent_limit = std::cmp::max(1, num_cpus / 2);
    tracing::info!(
        "Using {} CPU cores, limiting to {} concurrent metadata tasks",
        num_cpus,
        concurrent_limit
    );
    Arc::new(Semaphore::new(concurrent_limit))
});

pub struct MetaDataManager {
    repo: Repository,
    storer: Storer,
    book_storer: FileStorer,
    receiver: LocalMessageReceiver,
    _sender: LocalMessageSender,
    processing_paths: Arc<Mutex<HashSet<PathBuf>>>,
    spawner: Arc<dyn ProcessSpawner>,
}

impl MetaDataManager {
    fn new(
        repo: Repository,
        storer: Storer,
        book_storer: FileStorer,
        receiver: LocalMessageReceiver,
        sender: LocalMessageSender,
        spawner: Arc<dyn ProcessSpawner>,
    ) -> Self {
        Self {
            repo,
            storer,
            book_storer,
            receiver,
            _sender: sender,
            processing_paths: Arc::new(Mutex::new(HashSet::new())),
            spawner,
        }
    }

    pub fn consume(
        repo: Repository,
        storer: Storer,
        book_storer: FileStorer,
        receiver: LocalMessageReceiver,
        sender: LocalMessageSender,
        spawner: Arc<dyn ProcessSpawner>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut manager = Self::new(repo, storer, book_storer, receiver, sender, spawner);
            manager.event_loop().await;
            eprintln!("local event loop exiting");
        })
    }

    async fn event_loop(&mut self) {
        loop {
            match self.receiver.recv().await {
                Ok(msg) => match msg {
                    LocalMessage::Media(event) => self.handle_media_event(event).await,
                    _ => continue,
                },
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("event loop channel closed, shutting down");
                    break;
                }
                Err(e) => {
                    tracing::error!("event loop got an error: {}", e);
                }
            }
        }
    }

    async fn handle_media_event(&self, event: MediaEvent) {
        match event {
            MediaEvent::MediaAvailable(event) => {
                let full_path = event.full_path;
                let route = processing_route(&full_path);
                if route == MediaKind::Unsupported {
                    tracing::warn!(
                        path = %full_path.display(),
                        "Skipping unsupported completed download"
                    );
                    return;
                }

                // Check if this path is already being processed
                {
                    let mut processing = self.processing_paths.lock().await;
                    if processing.contains(&full_path) {
                        tracing::debug!("Skipping duplicate media event for path: {:?}", full_path);
                        return;
                    }
                    // Add to the set of paths being processed
                    processing.insert(full_path.clone());
                }

                // Clone the values needed for the task
                let search = event.search;
                let repo = self.repo.clone();
                let storer = self.storer.clone();
                let book_storer = self.book_storer.clone();
                let semaphore = WORKER_SEMAPHORE.clone();
                let processing_paths = self.processing_paths.clone();
                let path_for_cleanup = full_path.clone();
                let spawner = self.spawner.clone();
                // Spawn a new task to process the media event
                tokio::spawn(async move {
                    // Acquire a permit from the semaphore, which will limit concurrent tasks
                    let permit = semaphore.acquire().await.unwrap();

                    // Process the media event on its media-specific path.
                    match route {
                        MediaKind::Video => {
                            if let Err(err) =
                                generate_video_metadatas(full_path, storer, repo, search, spawner)
                                    .await
                            {
                                tracing::error!("processing MediaAvailable: {}", err);
                            }
                        }
                        MediaKind::Book => {
                            if let Err(err) =
                                generate_book_metadata(full_path, book_storer, repo, search).await
                            {
                                tracing::error!("processing book MediaAvailable: {}", err);
                            }
                        }
                        MediaKind::Unsupported => unreachable!("unsupported paths return early"),
                    }

                    // Remove the path from the processing set once done
                    {
                        let mut processing = processing_paths.lock().await;
                        processing.remove(&path_for_cleanup);
                        tracing::debug!(
                            "Completed processing and removed path from tracking: {:?}",
                            path_for_cleanup
                        );
                    }

                    // Permit is automatically dropped when it goes out of scope
                    drop(permit);
                });
            }
            _ => return,
        };
    }
}

fn processing_route(path: impl AsRef<std::path::Path>) -> MediaKind {
    classify_media_kind(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::algorithm::MediaKind;

    #[test]
    fn routes_completed_pdf_and_epub_files_to_book_processing() {
        assert_eq!(processing_route("library/book.pdf"), MediaKind::Book);
        assert_eq!(processing_route("library/BOOK.EPUB"), MediaKind::Book);
    }

    #[test]
    fn keeps_existing_completed_video_files_on_video_processing() {
        for path in ["movie.mp4", "episode.MKV", "clip.webm", "archive.avi"] {
            assert_eq!(processing_route(path), MediaKind::Video);
        }
    }

    #[test]
    fn routes_unrelated_completed_files_to_unsupported() {
        for path in ["cover.jpg", "notes.txt", "README", ".hidden.epub"] {
            assert_eq!(processing_route(path), MediaKind::Unsupported);
        }
    }
}
