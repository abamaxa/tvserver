use std::sync::Arc;

use crate::adaptors::{
    FileSystemStore, HTTPClient, SqlRepository, TelegramBot, TokioProcessSpawner, TorrentFetcher,
    YoutubeFetcher,
};
use crate::domain::config::{
    get_book_dir, get_book_thumbnail_dir, get_database_url, get_google_key, get_movie_dir,
    get_telegram_chat_id, get_telegram_token,
};
use crate::domain::messagebus::{
    LocalMessageExchange, LocalMessageExchangeError, MessageExchange, MessageFilter,
};
use crate::domain::messages::{LocalMessageReceiver, LocalMessageSender};
use crate::domain::services::MediaCheck;
use crate::domain::traits::FileStorer;
use crate::domain::traits::{BookCheckerHandle, Checker, ProcessSpawner, Repository, Storer};
use crate::domain::SearchEngineType;
use crate::services::{
    MediaStore, PirateClient, SearchEngine, SearchService, SharingService, TaskManager, YoutubeClient,
};

use super::book_runtime::{unavailable_book_checker, AvailableBookRuntime, BookRuntime};

#[derive(Clone)]
pub struct Context {
    store: Storer,
    checker: Checker,
    search: SearchService,
    messenger: MessageExchange,
    task_manager: Arc<TaskManager>,
    repository: Repository,
    local_message_exchange: LocalMessageExchange,
    book_runtime: BookRuntime,
    sharing: Option<Arc<SharingService>>,
}

impl Context {
    pub fn new(
        store: Storer,
        search: SearchService,
        messenger: MessageExchange,
        task_manager: Arc<TaskManager>,
        repository: Repository,
        checker: Checker,
        local_message_exchange: LocalMessageExchange,
        book_runtime: BookRuntime,
        sharing: Option<Arc<SharingService>>,
    ) -> Context {
        Context {
            store,
            checker,
            search,
            messenger,
            task_manager,
            repository,
            local_message_exchange,
            book_runtime,
            sharing,
        }
    }

    pub fn get_store(&self) -> Storer {
        self.store.clone()
    }

    pub fn get_task_manager(&self) -> Arc<TaskManager> {
        self.task_manager.clone()
    }

    pub fn get_spawner(&self) -> Arc<dyn ProcessSpawner> {
        self.task_manager.clone()
    }

    pub fn get_repository(&self) -> Repository {
        self.repository.clone()
    }

    pub fn get_local_sender(&self) -> LocalMessageSender {
        self.local_message_exchange.new_sender()
    }

    pub async fn listen_for_messages(
        &self,
        filter: MessageFilter,
    ) -> Result<LocalMessageReceiver, LocalMessageExchangeError> {
        self.local_message_exchange
            .listen_for_messages(filter)
            .await
    }

    pub fn get_checker(&self) -> Checker {
        self.checker.clone()
    }

    pub fn get_book_checker(&self) -> BookCheckerHandle {
        self.book_runtime
            .available()
            .map(|runtime| runtime.checker.clone())
            .unwrap_or_else(unavailable_book_checker)
    }

    pub fn get_storer(&self) -> Storer {
        self.store.clone()
    }

    pub fn get_search(&self) -> SearchService {
        self.search.clone()
    }

    /*pub async fn execute(&self, key: SocketAddr, command: RemoteMessage) -> (StatusCode, Json<Response>) {
        self.messenger.execute(key, command).await
    }*/

    pub fn get_messenger(&self) -> &MessageExchange {
        &self.messenger
    }

    pub fn get_sharing(&self) -> Option<Arc<SharingService>> {
        self.sharing.clone()
    }

    pub fn get_book_runtime(&self) -> BookRuntime {
        self.book_runtime.clone()
    }

    pub fn get_available_book_runtime(&self) -> Option<Arc<AvailableBookRuntime>> {
        self.book_runtime.available()
    }
}

pub async fn create_context() -> anyhow::Result<Context> {
    let local_message_exchange = LocalMessageExchange::new();

    let spawner = Arc::new(TokioProcessSpawner::new());

    let web_fetcher = Arc::new(HTTPClient::new());

    let task_manager = Arc::new(TaskManager::new(spawner.clone()));

    let torrent_fetcher = Arc::new(TorrentFetcher::new().await?);

    let youtube_fetcher = Arc::new(YoutubeFetcher::new(spawner.clone()));

    let repository = Arc::new(
        SqlRepository::new(&get_database_url(), Some(local_message_exchange.new_sender())).await?,
    );

    let torrent_search = Arc::new(SearchEngine::new(
        SearchEngineType::Torrent,
        Arc::new(PirateClient::new(web_fetcher.clone(), None)),
        torrent_fetcher,
    ));

    let youtube_search = Arc::new(SearchEngine::new(
        SearchEngineType::YouTube,
        Arc::new(YoutubeClient::new(&get_google_key(), web_fetcher.clone())),
        youtube_fetcher,
    ));

    let search_service = SearchService::new(
        task_manager.clone(),
        vec![torrent_search, youtube_search],
        repository.clone(),
    );

    let messenger = MessageExchange::new(
        local_message_exchange.new_sender(),
        local_message_exchange
            .listen_for_messages(MessageFilter::All)
            .await
            .map_err(|e| anyhow::anyhow!("failed to listen for messages: {}", e))?,
    );

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(&get_movie_dir()));
    let book_dir = get_book_dir();
    let book_thumbnail_dir = get_book_thumbnail_dir(&book_dir);
    let book_runtime = BookRuntime::initialize(
        repository.clone(),
        local_message_exchange.new_sender(),
        &book_dir,
        &book_thumbnail_dir,
    )
    .await;

    let checker = Arc::new(MediaCheck::new(
        file_storer.clone(),
        repository.clone(),
        local_message_exchange.new_sender(),
    ));
    let sharing = Arc::new(SharingService::new(
        Arc::new(TelegramBot::new(&get_telegram_chat_id(), &get_telegram_token())),
        repository.clone(),
        spawner.clone(),
    ));

    Ok(Context::new(
        Arc::new(MediaStore::new(file_storer, repository.clone())),
        search_service,
        messenger,
        task_manager,
        repository,
        checker,
        local_message_exchange,
        book_runtime,
        Some(sharing),
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    };

    use crate::{
        adaptors::SqlRepository,
        domain::{
            messagebus::{LocalMessageExchange, MessageExchange, MessageFilter},
            traits::{MockMediaChecker, MockMediaStorer, Repository, Storer},
            NoSpawner,
        },
        services::{SearchService, TaskManager},
    };

    use super::{BookRuntime, Context};
    use crate::entrypoints::BOOK_LIBRARY_UNAVAILABLE;

    static NEXT_CONTEXT_ROOT: AtomicU64 = AtomicU64::new(0);

    fn temp_context_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tvserver-{name}-{}-{}",
            std::process::id(),
            NEXT_CONTEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[tokio::test]
    async fn unavailable_book_root_builds_disabled_runtime_without_error() {
        let base = temp_context_root("unavailable-books");
        std::fs::create_dir_all(&base).unwrap();
        let blocked = base.join("not-a-directory");
        std::fs::write(&blocked, b"file").unwrap();
        let repository: Repository =
            Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let exchange = LocalMessageExchange::new();

        let runtime = BookRuntime::initialize(
            repository,
            exchange.new_sender(),
            &blocked,
            blocked.join("covers"),
        )
        .await;

        assert!(runtime.available().is_none());
        assert!(runtime.ingestion().is_none());
        assert!(runtime.static_roots().is_none());
        assert_eq!(
            runtime.unavailable_message(),
            Some(BOOK_LIBRARY_UNAVAILABLE)
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn writable_book_root_builds_available_runtime() {
        let base = temp_context_root("available-books");
        let books = base.join("books");
        let covers = base.join("covers");
        let repository: Repository =
            Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let exchange = LocalMessageExchange::new();

        let runtime =
            BookRuntime::initialize(repository, exchange.new_sender(), &books, &covers).await;
        let available = runtime.available().expect("book runtime should be available");
        let static_roots = runtime
            .static_roots()
            .expect("book static roots should be retained");

        assert!(runtime.ingestion().is_some());
        assert!(runtime.unavailable_message().is_none());
        assert!(static_roots.downloads.metadata(".").unwrap().is_dir());
        assert!(static_roots.thumbnails.metadata(".").unwrap().is_dir());
        assert!(covers.join("default-book.jpg").is_file());
        drop(static_roots);
        drop(available);
        drop(runtime);
        std::fs::remove_dir_all(base).unwrap();
    }

    async fn test_context(repository: Repository, book_runtime: BookRuntime) -> Context {
        let task_manager = Arc::new(TaskManager::new(Arc::new(NoSpawner::new())));
        let search = SearchService::new(task_manager.clone(), Vec::new(), repository.clone());
        let local_message_exchange = LocalMessageExchange::new();
        let messenger = MessageExchange::new(
            local_message_exchange.new_sender(),
            local_message_exchange
                .listen_for_messages(MessageFilter::All)
                .await
                .unwrap(),
        );
        let media_store: Storer = Arc::new(MockMediaStorer::new());
        let checker = Arc::new(MockMediaChecker::new());

        Context::new(
            media_store,
            search,
            messenger,
            task_manager,
            repository,
            checker,
            local_message_exchange,
            book_runtime,
            None,
        )
    }

    #[tokio::test]
    async fn context_exposes_injected_available_book_runtime() {
        let base = temp_context_root("context-available-books");
        let books = base.join("books");
        let covers = base.join("covers");
        let repository: Repository =
            Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let exchange = LocalMessageExchange::new();
        let runtime =
            BookRuntime::initialize(repository.clone(), exchange.new_sender(), &books, &covers)
                .await;
        let available = runtime.available().unwrap();
        let leases = available.ingestion.leases.clone();

        let context = test_context(repository, runtime).await;

        assert!(Arc::ptr_eq(
            &context.get_available_book_runtime().unwrap(),
            &available
        ));
        assert!(Arc::ptr_eq(
            &context.get_book_checker(),
            &available.checker
        ));
        let leased_path = books.join("leased.epub");
        let _processing = leases.acquire_processing(&leased_path).await;
        assert!(available
            .ingestion
            .leases
            .try_acquire_reconciling(&leased_path)
            .is_none());
        drop(_processing);
        drop(context);
        drop(available);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn unavailable_context_exposes_shared_no_op_book_checker() {
        let repository: Repository =
            Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let runtime = BookRuntime::Unavailable {
            message: Arc::from(BOOK_LIBRARY_UNAVAILABLE),
        };
        let context = test_context(repository, runtime).await;

        let first = context.get_book_checker();
        let second = context.get_book_checker();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.check_book_information().await.is_ok());
    }
}
