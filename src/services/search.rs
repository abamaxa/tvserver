use crate::adaptors::HTTPClient;
use crate::domain::config::get_google_key;
use crate::domain::messages::TaskState;
use crate::domain::traits::{Spawner, Task};
use crate::domain::SearchEngineType::{Torrent, YouTube};
use crate::domain::{SearchDownloader, SearchEngineType, Searcher};
use crate::services::{
    PirateClient, PirateFetcher, TransmissionDaemon, YoutubeClient, YoutubeFetcher,
};
use std::{collections::HashMap, sync::Arc};
use tokio::task::JoinSet;

#[derive(Clone)]
pub struct SearchEngine {
    searcher: Searcher,
    downloader: SearchDownloader,
}

pub type SearchEngineMap = HashMap<SearchEngineType, SearchEngine>;

#[derive(Default, Clone)]
pub struct SearchService {
    engines: SearchEngineMap,
}

impl SearchEngine {
    pub fn from(searcher: Searcher, downloader: SearchDownloader) -> Self {
        Self {
            searcher,
            downloader,
        }
    }
}

impl SearchService {
    pub fn new(spawner: Spawner) -> Self {
        let google_key = get_google_key();

        let youtube_fetcher: YoutubeFetcher = Arc::new(HTTPClient::new());
        let pirate_fetcher: PirateFetcher = Arc::new(HTTPClient::new());

        let youtube = Arc::new(YoutubeClient::new(&google_key, youtube_fetcher, spawner));
        let torrents: SearchDownloader = Arc::new(TransmissionDaemon::new());
        let pirate_bay: Searcher = Arc::new(PirateClient::new(pirate_fetcher, None));

        let engines: SearchEngineMap = HashMap::from([
            (YouTube, SearchEngine::from(youtube.clone(), youtube)),
            (Torrent, SearchEngine::from(pirate_bay, torrents)),
        ]);

        Self { engines }
    }

    pub fn from(engines: SearchEngineMap) -> Self {
        Self { engines }
    }

    pub fn get_search_engine(&self, engine: &SearchEngineType) -> &Searcher {
        &self
            .engines
            .get(engine)
            .expect("unrecognised search engine")
            .searcher
    }

    pub fn get_search_downloader(&self, engine: &SearchEngineType) -> &SearchDownloader {
        &self
            .engines
            .get(engine)
            .expect("unrecognised search engine")
            .downloader
    }

    pub async fn get_tasks(&self) -> Vec<Task> {
        let mut task_set = JoinSet::new();
        for engine in &self.engines {
            let downloader = engine.1.downloader.clone();
            task_set.spawn(async move { downloader.list_in_progress().await });
        }

        let mut tasks: Vec<Task> = vec![];
        while let Some(Ok(Ok(task_list))) = task_set.join_next().await {
            tasks.extend_from_slice(&task_list);
        }
        tasks
    }

    pub async fn get_task_states(&self) -> Vec<TaskState> {
        let mut task_state_set = JoinSet::new();
        for task in self.get_tasks().await {
            task_state_set.spawn(async move { task.get_state().await });
        }

        let mut task_states: Vec<TaskState> = vec![];
        while let Some(Ok(state)) = task_state_set.join_next().await {
            task_states.push(state);
        }
        task_states
    }
}
