use std::sync::Arc;

use crate::adaptors::{FileSystemStore, HTTPClient, SqlRepository, TelegramBot, TokioProcessSpawner, TorrentFetcher, YoutubeFetcher};
use crate::domain::config::{get_database_url, get_google_key, get_movie_dir, get_telegram_token, get_telegram_chat_id};
use crate::domain::messagebus::{LocalMessageExchange, LocalMessageExchangeError, MessageExchange, MessageFilter};
use crate::domain::messages::{LocalMessageReceiver, LocalMessageSender};
use crate::domain::services::MediaCheck;
use crate::domain::traits::FileStorer;
use crate::domain::SearchEngineType;
use crate::services::{
    MediaStore, PirateClient, SearchEngine, SearchService, TaskManager, YoutubeClient, SharingService
};
use crate::domain::traits::{Checker, ProcessSpawner, Repository, Storer};

#[derive(Clone)]
pub struct Context {
    store: Storer,
    checker: Checker,
    search: SearchService,
    messenger: MessageExchange,
    task_manager: Arc<TaskManager>,
    repository: Repository,
    local_message_exchange: LocalMessageExchange,
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

    pub async fn listen_for_messages(&self, filter: MessageFilter) -> Result<LocalMessageReceiver, LocalMessageExchangeError> {
        self.local_message_exchange.listen_for_messages(filter).await
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
}

pub async fn create_context() -> anyhow::Result<Context> {
    let local_message_exchange = LocalMessageExchange::new();
    
    let spawner = Arc::new(TokioProcessSpawner::new());

    let web_fetcher = Arc::new(HTTPClient::new());

    let task_manager = Arc::new(TaskManager::new(spawner.clone()));

    let torrent_fetcher = Arc::new(TorrentFetcher::new().await?);

    let youtube_fetcher = Arc::new(YoutubeFetcher::new(spawner.clone()));

    let repository = Arc::new(
        SqlRepository::new(&get_database_url(), Some(local_message_exchange.new_sender())).await?
    );

    let torrent_search = Arc::new(
        SearchEngine::new(
            SearchEngineType::Torrent, 
            Arc::new(PirateClient::new(web_fetcher.clone(), None)),
            torrent_fetcher,
        )
    );

    let youtube_search = Arc::new(
        SearchEngine::new(
            SearchEngineType::YouTube, 
            Arc::new(YoutubeClient::new(&get_google_key(), web_fetcher.clone())),
            youtube_fetcher,
        )
    );

    let search_service = SearchService::new(
        task_manager.clone(),
        vec![torrent_search, youtube_search],
        repository.clone()
    );

    let messenger = MessageExchange::new(
        local_message_exchange.new_sender(), 
        local_message_exchange.listen_for_messages( MessageFilter::All).await
            .map_err(|e| anyhow::anyhow!("failed to listen for messages: {}", e))?
    );

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(&get_movie_dir()));

    let checker = Arc::new(
        MediaCheck::new(file_storer.clone(), 
        repository.clone(), 
        local_message_exchange.new_sender())
    );

    let sharing = Arc::new(SharingService::new(
        Arc::new(TelegramBot::new(&get_telegram_chat_id(), &get_telegram_token())),
        repository.clone(),
        spawner.clone()
    ));
    
    Ok(Context::new(
        Arc::new(MediaStore::new(file_storer, repository.clone())),
        search_service,
        messenger,
        task_manager,
        repository,
        checker,
        local_message_exchange,
        Some(sharing)
    ))
}
