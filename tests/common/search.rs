use crate::common::{get_json_fetcher, get_no_spawner, get_text_fetcher, get_torrent_downloader};
use std::path::PathBuf;
use std::sync::Arc;
use app_lib::adaptors::YoutubeFetcher;
use app_lib::domain::config::get_google_key;
use app_lib::domain::SearchEngineType::{Torrent, YouTube};
use app_lib::domain::traits::Searcher;
use app_lib::services::{
    PirateClient, SearchEngine, SearchService, YoutubeClient,
};
use super::get_task_manager;

pub fn get_search_service(engine: SearchEngine) -> SearchService {
    let task_manager = get_task_manager();

    let engines = vec![Arc::new(engine)];

    SearchService::new(task_manager, engines)
}

pub async fn get_youtube_search(fixture: &str) -> SearchService {
    let youtube_fetcher = get_json_fetcher(fixture).await;

    let google_key = get_google_key();

    let spawner = get_no_spawner();

    let youtube = Arc::new(YoutubeClient::new(&google_key, youtube_fetcher));

    let downloader = Arc::new(YoutubeFetcher::new(spawner.clone()));

    get_search_service( SearchEngine::new(YouTube, youtube, downloader))
}

pub async fn get_pirate_search(torrent_fixture: &str, pirate_fixture: &str) -> SearchService {
    let torrents = get_torrent_downloader(torrent_fixture).await;

    let mut path = PathBuf::from("tests/fixtures");

    path.push(pirate_fixture);

    let fetcher = get_text_fetcher(&path).await;

    let pirate_bay: Searcher = Arc::new(PirateClient::new(fetcher, None));

    get_search_service(SearchEngine::new(Torrent, pirate_bay, torrents))
}
