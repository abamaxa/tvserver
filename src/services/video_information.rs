use crate::domain::algorithm::{classify_media_kind, MediaKind};
use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender, MediaEvent};
use crate::domain::services::{generate_book_metadata, generate_video_metadatas};
use crate::domain::traits::{FileStorer, ProcessSpawner, Repository, Storer};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context, Poll};
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};

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
    processing_paths: Arc<StdMutex<HashSet<PathBuf>>>,
    spawner: Arc<dyn ProcessSpawner>,
    processor: Arc<dyn MetadataProcessor>,
    workers: JoinSet<()>,
}

struct ProcessingPathGuard {
    paths: Arc<StdMutex<HashSet<PathBuf>>>,
    path: PathBuf,
}

impl ProcessingPathGuard {
    fn reserve(paths: Arc<StdMutex<HashSet<PathBuf>>>, path: PathBuf) -> Option<Self> {
        let inserted = paths
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path.clone());
        inserted.then_some(Self { paths, path })
    }
}

impl Drop for ProcessingPathGuard {
    fn drop(&mut self) {
        self.paths
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.path);
        tracing::debug!(path = ?self.path, "Released metadata path reservation");
    }
}

pub struct MetaDataManagerHandle {
    task: JoinHandle<()>,
}

impl MetaDataManagerHandle {
    pub fn abort(&self) {
        self.task.abort();
    }
}

impl Future for MetaDataManagerHandle {
    type Output = Result<(), tokio::task::JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.task).poll(cx)
    }
}

impl Drop for MetaDataManagerHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MetaDataManager {
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
            processing_paths: Arc::new(StdMutex::new(HashSet::new())),
            spawner,
            processor,
            workers: JoinSet::new(),
        }
    }

    pub fn consume(
        repo: Repository,
        storer: Storer,
        book_storer: FileStorer,
        receiver: LocalMessageReceiver,
        sender: LocalMessageSender,
        spawner: Arc<dyn ProcessSpawner>,
    ) -> MetaDataManagerHandle {
        Self::consume_with_processor(
            repo,
            storer,
            book_storer,
            receiver,
            sender,
            spawner,
            Arc::new(ProductionMetadataProcessor),
        )
    }

    fn consume_with_processor(
        repo: Repository,
        storer: Storer,
        book_storer: FileStorer,
        receiver: LocalMessageReceiver,
        sender: LocalMessageSender,
        spawner: Arc<dyn ProcessSpawner>,
        processor: Arc<dyn MetadataProcessor>,
    ) -> MetaDataManagerHandle {
        let task = tokio::spawn(async move {
            let mut manager = Self::new_with_processor(
                repo,
                storer,
                book_storer,
                receiver,
                sender,
                spawner,
                processor,
            );
            manager.event_loop().await;
            eprintln!("local event loop exiting");
        });
        MetaDataManagerHandle { task }
    }

    async fn event_loop(&mut self) {
        loop {
            tokio::select! {
                worker = self.workers.join_next(), if !self.workers.is_empty() => {
                    if let Some(Err(error)) = worker {
                        tracing::error!("metadata worker terminated unexpectedly: {error}");
                    }
                }
                message = self.receiver.recv() => match message {
                    Ok(LocalMessage::Media(event)) => self.handle_media_event(event).await,
                    Ok(_) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("event loop channel closed, shutting down");
                        break;
                    }
                    Err(error) => tracing::error!("event loop got an error: {error}"),
                }
            }
        }
        self.workers.shutdown().await;
    }

    async fn handle_media_event(&mut self, event: MediaEvent) {
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

                let Some(path_guard) = ProcessingPathGuard::reserve(
                    self.processing_paths.clone(),
                    full_path.clone(),
                ) else {
                    tracing::debug!("Skipping duplicate media event for path: {:?}", full_path);
                    return;
                };

                // Clone the values needed for the task
                let search = event.search;
                let repo = self.repo.clone();
                let storer = self.storer.clone();
                let book_storer = self.book_storer.clone();
                let semaphore = WORKER_SEMAPHORE.clone();
                let spawner = self.spawner.clone();
                let processor = self.processor.clone();
                self.workers.spawn(async move {
                    let _path_guard = path_guard;
                    let Ok(permit) = semaphore.acquire_owned().await else {
                        tracing::error!("metadata worker semaphore was closed");
                        return;
                    };

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
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex as StdMutex,
    };
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
        completed: AtomicUsize,
    }

    impl RecordingProcessor {
        fn new(blocked: bool) -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
                started: Notify::new(),
                releases: Semaphore::new(if blocked { 0 } else { 16 }),
                completed: AtomicUsize::new(0),
            }
        }

        async fn record(&self, kind: MediaKind) {
            self.calls.lock().unwrap().push(kind);
            self.started.notify_one();
            self.releases.acquire().await.unwrap().forget();
            self.completed.fetch_add(1, Ordering::SeqCst);
        }

        fn calls(&self) -> Vec<MediaKind> {
            self.calls.lock().unwrap().clone()
        }

        fn completed(&self) -> usize {
            self.completed.load(Ordering::SeqCst)
        }
    }

    struct PanicOnceProcessor {
        calls: AtomicUsize,
        first_started: Notify,
        second_started: Notify,
    }

    impl PanicOnceProcessor {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                first_started: Notify::new(),
                second_started: Notify::new(),
            }
        }
    }

    #[async_trait]
    impl MetadataProcessor for PanicOnceProcessor {
        async fn process_video(
            &self,
            _path: PathBuf,
            _storer: Storer,
            _repo: Repository,
            _search: Option<String>,
            _spawner: Arc<dyn ProcessSpawner>,
        ) -> Result<(), String> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                self.first_started.notify_one();
                panic!("forced metadata processor panic");
            }
            self.second_started.notify_one();
            Ok(())
        }

        async fn process_book(
            &self,
            _path: PathBuf,
            _storer: FileStorer,
            _repo: Repository,
            _search: Option<String>,
        ) -> Result<(), String> {
            unreachable!("lifecycle regression uses a video path")
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
        manager: &mut MetaDataManager,
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
        let mut manager = manager_with_processor(processor.clone()).await;

        dispatch_and_wait(&mut manager, &processor, "library/book.pdf").await;
        dispatch_and_wait(&mut manager, &processor, "library/BOOK.EPUB").await;

        assert_eq!(processor.calls(), [MediaKind::Book, MediaKind::Book]);
    }

    #[tokio::test]
    async fn keeps_existing_completed_video_events_on_video_processing() {
        let processor = Arc::new(RecordingProcessor::new(false));
        let mut manager = manager_with_processor(processor.clone()).await;

        dispatch_and_wait(&mut manager, &processor, "movie.mp4").await;

        assert_eq!(processor.calls(), [MediaKind::Video]);
    }

    #[tokio::test]
    async fn unsupported_completed_events_dispatch_no_processor() {
        let processor = Arc::new(RecordingProcessor::new(false));
        let mut manager = manager_with_processor(processor.clone()).await;

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
        let mut manager = manager_with_processor(processor.clone()).await;

        dispatch_and_wait(&mut manager, &processor, "movie.mp4").await;
        manager
            .handle_media_event(MediaEvent::new_media(std::path::Path::new("movie.mp4"), None))
            .await;

        assert_eq!(processor.calls(), [MediaKind::Video]);
        processor.releases.add_permits(1);
    }

    #[tokio::test]
    async fn dropping_manager_cancels_blocked_workers_before_they_can_complete() {
        let processor = Arc::new(RecordingProcessor::new(true));
        let mut manager = manager_with_processor(processor.clone()).await;

        dispatch_and_wait(&mut manager, &processor, "shutdown.mp4").await;
        drop(manager);
        processor.releases.add_permits(1);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(processor.completed(), 0);
    }

    #[tokio::test]
    async fn aborting_manager_handle_cancels_workers_before_later_persistence_work() {
        let processor = Arc::new(RecordingProcessor::new(true));
        let exchange = LocalMessageExchange::new();
        let receiver = exchange
            .listen_for_messages(MessageFilter::All)
            .await
            .unwrap();
        let repository: Repository =
            Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let storer: Storer = Arc::new(MockMediaStorer::new());
        let book_storer: FileStorer = Arc::new(FileSystemStore::new(
            std::env::temp_dir()
                .join("tvserver-routing-abort-books")
                .to_str()
                .unwrap(),
        ));
        let handle = MetaDataManager::consume_with_processor(
            repository,
            storer,
            book_storer,
            receiver,
            exchange.new_sender(),
            Arc::new(NoSpawner::new()),
            processor.clone(),
        );
        let started = processor.started.notified();
        exchange
            .new_sender()
            .send(LocalMessage::Media(MediaEvent::new_media(
                std::path::Path::new("shutdown-handle.mp4"),
                None,
            )))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), started)
            .await
            .unwrap();

        handle.abort();
        let error = handle.await.unwrap_err();
        assert!(error.is_cancelled());
        processor.releases.add_permits(1);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(processor.completed(), 0);
    }

    #[tokio::test]
    async fn processor_panic_releases_path_reservation_for_a_later_event() {
        let processor = Arc::new(PanicOnceProcessor::new());
        let mut manager = manager_with_processor(processor.clone()).await;
        let first_started = processor.first_started.notified();

        manager
            .handle_media_event(MediaEvent::new_media(std::path::Path::new("panic.mp4"), None))
            .await;
        tokio::time::timeout(std::time::Duration::from_secs(2), first_started)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let second_started = processor.second_started.notified();
        manager
            .handle_media_event(MediaEvent::new_media(std::path::Path::new("panic.mp4"), None))
            .await;

        tokio::time::timeout(std::time::Duration::from_secs(2), second_started)
            .await
            .expect("panic cleanup must allow the same path to be processed again");
    }
}
