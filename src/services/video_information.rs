use crate::domain::algorithm::{classify_media_kind, MediaKind};
use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender, MediaEvent};
use crate::domain::services::{generate_book_metadata, generate_video_metadatas};
use crate::domain::traits::{FileStorer, ProcessSpawner, Repository, Storer};
use async_trait::async_trait;
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

#[async_trait]
trait MetadataProcessor: Send + Sync {
    async fn process_video(
        &self,
        path: PathBuf,
        storer: Storer,
        repo: Repository,
        search: Option<String>,
        spawner: Arc<dyn ProcessSpawner>,
    ) -> Result<(), String>;

    async fn process_book(
        &self,
        path: PathBuf,
        storer: FileStorer,
        repo: Repository,
        search: Option<String>,
    ) -> Result<(), String>;
}

struct ProductionMetadataProcessor;

#[async_trait]
impl MetadataProcessor for ProductionMetadataProcessor {
    async fn process_video(
        &self,
        path: PathBuf,
        storer: Storer,
        repo: Repository,
        search: Option<String>,
        spawner: Arc<dyn ProcessSpawner>,
    ) -> Result<(), String> {
        generate_video_metadatas(path, storer, repo, search, spawner)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    async fn process_book(
        &self,
        path: PathBuf,
        storer: FileStorer,
        repo: Repository,
        search: Option<String>,
    ) -> Result<(), String> {
        generate_book_metadata(path, storer, repo, search)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

pub struct MetaDataManager {
    repo: Repository,
    storer: Storer,
    book_storer: FileStorer,
    receiver: LocalMessageReceiver,
    _sender: LocalMessageSender,
    processing_paths: Arc<Mutex<HashSet<PathBuf>>>,
    spawner: Arc<dyn ProcessSpawner>,
    processor: Arc<dyn MetadataProcessor>,
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
        Self::new_with_processor(
            repo,
            storer,
            book_storer,
            receiver,
            sender,
            spawner,
            Arc::new(ProductionMetadataProcessor),
        )
    }

    fn new_with_processor(
        repo: Repository,
        storer: Storer,
        book_storer: FileStorer,
        receiver: LocalMessageReceiver,
        sender: LocalMessageSender,
        spawner: Arc<dyn ProcessSpawner>,
        processor: Arc<dyn MetadataProcessor>,
    ) -> Self {
        Self {
            repo,
            storer,
            book_storer,
            receiver,
            _sender: sender,
            processing_paths: Arc::new(Mutex::new(HashSet::new())),
            spawner,
            processor,
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
                let processor = self.processor.clone();
                // Spawn a new task to process the media event
                tokio::spawn(async move {
                    // Acquire a permit from the semaphore, which will limit concurrent tasks
                    let permit = semaphore.acquire().await.unwrap();

                    // Process the media event on its media-specific path.
                    match route {
                        MediaKind::Video => {
                            if let Err(err) = processor
                                .process_video(full_path, storer, repo, search, spawner)
                                .await
                            {
                                tracing::error!("processing MediaAvailable: {}", err);
                            }
                        }
                        MediaKind::Book => {
                            if let Err(err) = processor
                                .process_book(full_path, book_storer, repo, search)
                                .await
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
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Notify;

    use crate::{
        adaptors::{FileSystemStore, SqlRepository},
        domain::{
            messagebus::{LocalMessageExchange, MessageFilter},
            traits::{MockMediaStorer, Repository},
            NoSpawner,
        },
    };

    struct RecordingProcessor {
        calls: StdMutex<Vec<MediaKind>>,
        started: Notify,
        releases: Semaphore,
    }

    impl RecordingProcessor {
        fn new(blocked: bool) -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
                started: Notify::new(),
                releases: Semaphore::new(if blocked { 0 } else { 16 }),
            }
        }

        async fn record(&self, kind: MediaKind) {
            self.calls.lock().unwrap().push(kind);
            self.started.notify_one();
            self.releases.acquire().await.unwrap().forget();
        }

        fn calls(&self) -> Vec<MediaKind> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl MetadataProcessor for RecordingProcessor {
        async fn process_video(
            &self,
            _path: PathBuf,
            _storer: Storer,
            _repo: Repository,
            _search: Option<String>,
            _spawner: Arc<dyn ProcessSpawner>,
        ) -> Result<(), String> {
            self.record(MediaKind::Video).await;
            Ok(())
        }

        async fn process_book(
            &self,
            _path: PathBuf,
            _storer: FileStorer,
            _repo: Repository,
            _search: Option<String>,
        ) -> Result<(), String> {
            self.record(MediaKind::Book).await;
            Ok(())
        }
    }

    async fn manager_with_processor(processor: Arc<dyn MetadataProcessor>) -> MetaDataManager {
        let exchange = LocalMessageExchange::new();
        let receiver = exchange
            .listen_for_messages(MessageFilter::All)
            .await
            .unwrap();
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let storer: Storer = Arc::new(MockMediaStorer::new());
        let book_storer: FileStorer = Arc::new(FileSystemStore::new(
            std::env::temp_dir()
                .join("tvserver-routing-test-books")
                .to_str()
                .unwrap(),
        ));
        MetaDataManager::new_with_processor(
            repository,
            storer,
            book_storer,
            receiver,
            exchange.new_sender(),
            Arc::new(NoSpawner::new()),
            processor,
        )
    }

    async fn dispatch_and_wait(
        manager: &MetaDataManager,
        processor: &RecordingProcessor,
        path: &str,
    ) {
        let started = processor.started.notified();
        manager
            .handle_media_event(MediaEvent::new_media(std::path::Path::new(path), None))
            .await;
        tokio::time::timeout(std::time::Duration::from_secs(2), started)
            .await
            .expect("metadata processor should be dispatched");
    }

    #[tokio::test]
    async fn routes_completed_pdf_and_epub_events_to_book_processing() {
        let processor = Arc::new(RecordingProcessor::new(false));
        let manager = manager_with_processor(processor.clone()).await;

        dispatch_and_wait(&manager, &processor, "library/book.pdf").await;
        dispatch_and_wait(&manager, &processor, "library/BOOK.EPUB").await;

        assert_eq!(processor.calls(), [MediaKind::Book, MediaKind::Book]);
    }

    #[tokio::test]
    async fn keeps_existing_completed_video_events_on_video_processing() {
        let processor = Arc::new(RecordingProcessor::new(false));
        let manager = manager_with_processor(processor.clone()).await;

        dispatch_and_wait(&manager, &processor, "movie.mp4").await;

        assert_eq!(processor.calls(), [MediaKind::Video]);
    }

    #[tokio::test]
    async fn unsupported_completed_events_dispatch_no_processor() {
        let processor = Arc::new(RecordingProcessor::new(false));
        let manager = manager_with_processor(processor.clone()).await;

        for path in ["cover.jpg", "notes.txt", "README", ".hidden.epub"] {
            manager
                .handle_media_event(MediaEvent::new_media(std::path::Path::new(path), None))
                .await;
        }

        assert!(processor.calls().is_empty());
    }

    #[tokio::test]
    async fn duplicate_completed_events_dispatch_only_once_while_processing() {
        let processor = Arc::new(RecordingProcessor::new(true));
        let manager = manager_with_processor(processor.clone()).await;

        dispatch_and_wait(&manager, &processor, "movie.mp4").await;
        manager
            .handle_media_event(MediaEvent::new_media(std::path::Path::new("movie.mp4"), None))
            .await;

        assert_eq!(processor.calls(), [MediaKind::Video]);
        processor.releases.add_permits(1);
    }
}
