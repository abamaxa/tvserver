use crate::common::{get_json_fetcher, get_no_spawner, get_text_fetcher, get_torrent_downloader};
use std::path::PathBuf;
use std::{collections::HashMap, sync::Arc};
use tvserver::domain::config::get_google_key;
use tvserver::domain::SearchEngineType::{Torrent, YouTube};
use tvserver::domain::{SearchEngineType, Searcher};
use tvserver::services::{
    PirateClient, SearchEngine, SearchEngineMap, SearchService, YoutubeClient,
};

pub fn get_search_service(engine_type: SearchEngineType, engine: SearchEngine) -> SearchService {
    let engines: SearchEngineMap = HashMap::from([(engine_type, engine)]);

    SearchService::from(engines)
}

pub async fn get_youtube_search(fixture: &str) -> SearchService {
    let youtube_fetcher = get_json_fetcher(fixture).await;

    let google_key = get_google_key();

    let spawner = get_no_spawner();

    let youtube = Arc::new(YoutubeClient::new(&google_key, youtube_fetcher, spawner));

    get_search_service(YouTube, SearchEngine::from(youtube.clone(), youtube.clone()))
}

pub async fn get_pirate_search(torrent_fixture: &str, pirate_fixture: &str) -> SearchService {
    let torrents = get_torrent_downloader(torrent_fixture).await;

    let mut path = PathBuf::from("tests/fixtures");
    path.push(pirate_fixture);

    let fetcher = get_text_fetcher(&path).await;

    let pirate_bay: Searcher = Arc::new(PirateClient::new(fetcher, None));

    get_search_service(Torrent, SearchEngine::from(pirate_bay, torrents))
}
