use crate::domain::messages::{DownloadRequest, LocalMessageSender};
use crate::domain::services::DownloadMonitor;
use crate::domain::traits::{Downloader, Searcher};
use crate::domain::SearchEngineType;
use std::collections::HashMap;
use std::sync::Arc;

use super::TaskManager;

#[derive(Clone)]
pub struct SearchEngine {
    engine_type: SearchEngineType,
    searcher: Searcher,
    downloader: Downloader,
}

pub type SearchEngineMap = HashMap<SearchEngineType, Arc<SearchEngine>>;

#[derive(Clone)]
pub struct SearchService {
    engines: SearchEngineMap,
    task_manager: Arc<TaskManager>,
}

impl SearchEngine {
    pub fn new(engine_type: SearchEngineType, searcher: Searcher, downloader: Downloader) -> Self {
        Self {
            engine_type,
            searcher,
            downloader,
        }
    }
}

impl SearchService {
    pub fn new(task_manager: Arc<TaskManager>, engines: Vec<Arc<SearchEngine>>) -> Self {
        let mut engines_map = HashMap::new();
        
        for engine in engines {
            engines_map.insert(engine.engine_type.clone(), engine);
        }
        
        Self {
            engines: engines_map,
            task_manager: task_manager,
        }
    }

    pub fn get_search_engine(&self, engine: &SearchEngineType) -> &Searcher {
        &self
            .engines
            .get(engine)
            .expect("unrecognised search engine")
            .searcher
    }

    pub fn get_search_downloader(&self, engine: &SearchEngineType) -> &Downloader {
        &self
            .engines
            .get(engine)
            .expect("unrecognised search engine")
            .downloader
    }
    
    pub async fn download(&self, request: DownloadRequest, sender: LocalMessageSender) -> Result<(), Box<dyn std::error::Error>> {
        // Get the search engine from the map
        let search_engine = self.engines.get(&request.engine)
            .ok_or_else(|| "unknown search engine".to_string())?;
        
        // Call the downloader's Download method
        let progresser = search_engine.downloader.download(request.clone()).await?;
        
        // Create a new download monitor task
        let task = Arc::new(DownloadMonitor::new(request, progresser));

        DownloadMonitor::monitor(task.clone(), sender);
        
        // Add the task to the task manager
        self.task_manager.add(task.clone()).await;
        
        Ok(())
    }
}
