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
use crate::domain::traits::{Checker, ProcessSpawner, Repository, Storer};
use crate::domain::SearchEngineType;
use crate::services::{
    BookStore, MediaStore, PirateClient, SearchEngine, SearchService, SharingService, TaskManager,
    YoutubeClient,
};

#[derive(Clone)]
pub struct Context {
    store: Storer,
    checker: Checker,
    search: SearchService,
    messenger: MessageExchange,
    task_manager: Arc<TaskManager>,
    repository: Repository,
    local_message_exchange: LocalMessageExchange,
    book_store: Arc<BookStore>,
    book_file_storer: FileStorer,
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
        book_store: Arc<BookStore>,
        book_file_storer: FileStorer,
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
            book_store,
            book_file_storer,
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

    pub fn get_book_store(&self) -> Arc<BookStore> {
        self.book_store.clone()
    }

    pub fn get_book_file_storer(&self) -> FileStorer {
        self.book_file_storer.clone()
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
    let book_file_storer: FileStorer = Arc::new(FileSystemStore::new(&book_dir));
    let book_thumbnail_dir_string = book_thumbnail_dir.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "configured book thumbnail directory is not valid UTF-8: {}",
            book_thumbnail_dir.display()
        )
    })?;
    let book_thumbnail_file_storer: FileStorer =
        Arc::new(FileSystemStore::new(book_thumbnail_dir_string));
    let book_store = Arc::new(BookStore::new_with_roots(
        book_file_storer.clone(),
        book_thumbnail_file_storer,
        repository.clone(),
        &book_dir,
        &book_thumbnail_dir,
    ));

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
        book_store,
        book_file_storer,
        Some(sharing),
    ))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use crate::{
        adaptors::{FileSystemStore, SqlRepository},
        domain::{
            messagebus::{LocalMessageExchange, MessageExchange, MessageFilter},
            traits::{FileStorer, MockMediaChecker, MockMediaStorer, Repository, Storer},
            NoSpawner,
        },
        services::{BookStore, SearchService, TaskManager},
    };

    use super::Context;

    #[tokio::test]
    async fn context_exposes_injected_book_store_and_book_file_store() {
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
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
        let book_files: FileStorer = Arc::new(FileSystemStore::new("/tmp/context-books"));
        let thumbnail_files: FileStorer =
            Arc::new(FileSystemStore::new("/tmp/context-book-thumbnails"));
        let book_store = Arc::new(BookStore::new_with_roots(
            book_files.clone(),
            thumbnail_files,
            repository.clone(),
            Path::new("/tmp/context-books"),
            Path::new("/tmp/context-book-thumbnails"),
        ));

        let context = Context::new(
            media_store,
            search,
            messenger,
            task_manager,
            repository,
            checker,
            local_message_exchange,
            book_store.clone(),
            book_files.clone(),
            None,
        );

        assert!(Arc::ptr_eq(&context.get_book_store(), &book_store));
        assert!(Arc::ptr_eq(&context.get_book_file_storer(), &book_files));
    }
}
