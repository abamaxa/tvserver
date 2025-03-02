use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::{sync::RwLock, fs};

use crate::domain::config::get_downloads_dir;
use crate::domain::traits::{DownloadProgress, DownloadProgressMonitor, Download, Spawner, Task};
use crate::domain::messages::{DownloadInfo, DownloadRequest, TaskState};

pub struct YoutubeTask {
    spawner: Spawner,
    request: DownloadRequest,
    monitor: RwLock<Option<Task>>,
    destination: PathBuf,
}

#[async_trait]
impl DownloadProgress for YoutubeTask {
    fn terminate(&self) {
        todo!()
    }

    async fn observe(&self) -> DownloadInfo {
        self.get_download_info().await
    }
}

impl YoutubeTask {
    pub fn new(spawner: Spawner, request: DownloadRequest) -> Self {
        let destination = PathBuf::from(get_downloads_dir())
            .join("YouTube")
            .join(format!("{}.mp4", request.name));

        Self { spawner, request, monitor: RwLock::new(None), destination: destination }
    }

    async fn start(&self) -> Result<(), anyhow::Error> {

        let output_path = self.destination
            .to_string_lossy()
            .to_string();

        let monitor =self.spawner
            .execute(
                &self.request.name,
                "yt-dlp",
                vec![
                    "--no-update",
                    "--sponsorblock-remove",
                    "all",
                    "-f", 
                    "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
                    "-o",
                    &output_path,
                    &self.request.link,
                ],
            )
            .await;

        self.monitor.write().await.replace(monitor);

        Ok(())  
    }

    /// Get current download info
    async fn get_download_info(&self) -> DownloadInfo { 
        // Get downloaded size
        let downloaded_size = self.get_file_size().await;
        let state = match self.monitor.read().await.as_ref() {
            Some(monitor) => monitor.get_state().await,
            None => TaskState {..Default::default()},
        };
        
        DownloadInfo {
            total_size: None,
            downloaded_size,
            uploaded_size: None,
            finished: state.finished,
            error_message: state.error_string,
            progress_message: state.process_details,
            files: vec![self.destination.to_string_lossy().to_string()],
        }
    }

    async fn get_file_size(&self) -> i64 {
        let base_file = self.destination.with_extension("");
        let dir = self.destination.parent().unwrap_or_else(|| Path::new(""));

        let Ok(mut entries) = fs::read_dir(dir).await else {
            tracing::error!("Failed to read directory: {:?}", dir);
            return 0;
        };

        let base_str = base_file.to_string_lossy().to_string();
        let mut total_size = 0;

        // Iterate through directory entries one by one
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() && path.to_string_lossy().to_string().starts_with(&base_str) {
                if let Ok(metadata) = fs::metadata(&path).await {
                    total_size += metadata.len() as i64;
                }
            }
        }

        total_size
    }

}

pub struct YoutubeFetcher {
    spawner: Spawner,
}

#[async_trait]
impl Download for YoutubeFetcher {
    async fn download(&self, request: DownloadRequest) -> Result<DownloadProgressMonitor, anyhow::Error> {
        let task = YoutubeTask::new(self.spawner.clone(), request);
        task.start().await?;
        Ok(Arc::new(task))
    }
}

impl YoutubeFetcher {
    pub fn new(spawner: Spawner) -> Self {
        Self { spawner }
    }
}


/*
#[cfg(test)]
mod test {
    use super::*;
    use crate::adaptors::{HTTPClient, TokioProcessSpawner};
    use crate::domain::config::get_google_key;
    use crate::domain::models::{Id, Item, Snippet};
    use crate::domain::traits::{MockTaskMonitor, ProcessSpawner, Task};
    use anyhow::anyhow;

    #[derive(Default, Debug, Clone)]
    struct MockFetcher {
        response: Option<YoutubeResponse>,
        error: Option<String>,
    }

    #[async_trait]
    impl<'a> JsonFetcher<'a, YoutubeResponse, &'a [(&'a str, &'a str)]> for MockFetcher {
        async fn fetch(
            &self,
            url: &str,
            query: &'a &'a [(&'a str, &'a str)],
        ) -> anyhow::Result<YoutubeResponse> {
            match &self.response {
                Some(response) => Ok(response.clone()),
                None => match &self.error {
                    Some(error) => Err(anyhow!(error.clone())),
                    None => {
                        let items = query
                            .iter()
                            .map(|(k, v)| store_query_parameter_in_an_item(k, v, url))
                            .collect();
                        Ok(YoutubeResponse {
                            items,
                            ..Default::default()
                        })
                    }
                },
            }
        }
    }

    fn store_query_parameter_in_an_item(key: &str, value: &str, url: &str) -> Item {
        Item {
            snippet: Snippet {
                title: key.to_string(),
                description: value.to_string(),
                ..Default::default()
            },
            id: Id {
                video_id: url.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[derive(Default, Debug, Clone)]
    struct MockProcessSpawner {}

    #[async_trait]
    impl ProcessSpawner for MockProcessSpawner {
        async fn execute(&self, _name: &str, _cmd: &str, _args: Vec<&str>) -> Task {
            Arc::new(MockTaskMonitor::new())
        }
    }

    #[tokio::test]
    async fn test_conversion_of_response() -> anyhow::Result<()> {
        const THE_QUERY: &str = "find this";
        const THE_KEY: &str = "the key";

        let fetcher = MockFetcher {
            ..Default::default()
        };
        let spawner = MockProcessSpawner {};

        let client: &dyn MediaSearcher<DownloadableItem> =
            &YoutubeClient::new(THE_KEY, Arc::new(fetcher), Arc::new(spawner));

        let response = client.search(THE_QUERY).await?;

        let results = response.results.ok_or(anyhow!("expected results"))?;

        assert_eq!(results.len(), 5);

        for item in &results {
            assert_eq!(
                item.description,
                match item.title.as_str() {
                    "q" => THE_QUERY,
                    "key" => THE_KEY,
                    "part" => SEARCH_PART,
                    "maxResults" => SEARCH_MAX_RESULTS,
                    "type" => SEARCH_TYPE,
                    _ => panic!(
                        "unexpected query parameter: {}: {}",
                        item.title, item.description
                    ),
                }
            );

            assert_eq!(item.link, SEARCH_URL);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_youtube_error_handling() -> anyhow::Result<()> {
        const ERROR_MESSAGE: &str = "test error message";

        let fetcher = MockFetcher {
            error: Some(ERROR_MESSAGE.to_string()),
            ..Default::default()
        };

        let spawner = MockProcessSpawner {};

        let client: &dyn MediaSearcher<DownloadableItem> =
            &YoutubeClient::new("", Arc::new(fetcher), Arc::new(spawner));

        let response = client.search("").await;

        assert!(response.is_ok());

        let results = response.unwrap();

        assert_eq!(&results.error.unwrap().to_string(), ERROR_MESSAGE);

        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_live_search_youtube() {
        let client = Arc::new(HTTPClient::new());
        let spawner = Arc::new(TokioProcessSpawner::new());
        let pc = YoutubeClient::new(&get_google_key(), client, spawner);

        match pc.search("Dragons Den 2023").await {
            Ok(response) => {
                if let Some(err) = response.error {
                    panic!("failed: {}", err)
                }

                if let Some(results) = response.results {
                    for result in &results {
                        println!("({}):{} - {}", result.link, result.title, result.description);
                    }
                }
            }
            Err(e) => panic!("error: {}", e.to_string()),
        };
    }
}
*/
