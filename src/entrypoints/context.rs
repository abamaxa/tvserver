use sqlx::Error;
use std::sync::Arc;

use crate::adaptors::{FileSystemStore, HTTPClient, SqlRepository, TokioProcessSpawner, TorrentFetcher, YoutubeFetcher};
use crate::domain::config::{get_database_url, get_google_key, get_movie_dir};
use crate::domain::messagebus::MessageExchange;
use crate::domain::services::MediaCheck;
use crate::domain::traits::FileStorer;
use crate::domain::SearchEngineType;
use crate::services::{
    MediaStore, PirateClient, SearchEngine, SearchService, TaskManager, YoutubeClient
};

use super::api::Context;

pub async fn create_context() -> Result<Context, Error> {
    let spawner = Arc::new(TokioProcessSpawner::new());

    let web_fetcher = Arc::new(HTTPClient::new());

    let task_manager = Arc::new(TaskManager::new(spawner.clone()));

    let torrent_fetcher = Arc::new(TorrentFetcher::new().await);

    let youtube_fetcher = Arc::new(YoutubeFetcher::new(spawner.clone()));

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

    let search = SearchService::new(
        task_manager.clone(),
        vec![torrent_search, youtube_search]
    );

    let messenger = MessageExchange::new();

    let repository = Arc::new(SqlRepository::new(&get_database_url()).await?);

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(&get_movie_dir()));

    let checker = Arc::new(MediaCheck::new(file_storer.clone(), repository.clone(), messenger.get_local_sender()));
    Ok(Context::new(
        Arc::new(MediaStore::new(file_storer, repository.clone())),
        search,
        messenger,
        task_manager,
        repository,
        checker,
    ))
}
