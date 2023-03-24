use crate::domain::config::get_movie_dir;
use crate::domain::models::{DownloadableItem, SearchResults, YoutubeResponse};
use crate::domain::traits::{JsonFetcher, MediaDownloader, MediaSearcher, Spawner, Task};
use anyhow;
use async_trait::async_trait;
use std::sync::Arc;

const SEARCH_URL: &str = "https://www.googleapis.com/youtube/v3/search";
const SEARCH_PART: &str = "snippet";
const SEARCH_MAX_RESULTS: &str = "50";
const SEARCH_TYPE: &str = "video";

pub type YoutubeResult = SearchResults<DownloadableItem>;
pub type YoutubeFetcher = Arc<dyn JsonFetcher<YoutubeResponse>>;

pub struct YoutubeClient {
    key: String,
    client: YoutubeFetcher,
    spawner: Spawner,
}

#[async_trait]
impl MediaSearcher<DownloadableItem> for YoutubeClient {
    async fn search(&self, query: &str) -> anyhow::Result<YoutubeResult> {
        let query = [
            ("q", query),
            ("key", &self.key),
            ("part", SEARCH_PART),
            ("maxResults", SEARCH_MAX_RESULTS),
            ("type", SEARCH_TYPE),
        ];

        match self.client.get_json(SEARCH_URL, &query).await {
            Ok(results) => Ok(self.make_success_response(results)),
            Err(err) => Ok(SearchResults::error(&err.to_string())),
        }
    }
}

impl YoutubeClient {
    pub fn new(key: &str, client: YoutubeFetcher, spawner: Spawner) -> Self {
        Self {
            key: String::from(key),
            client,
            spawner,
        }
    }

    fn make_success_response(&self, yt_response: YoutubeResponse) -> YoutubeResult {
        SearchResults::success(
            yt_response
                .items
                .iter()
                .map(DownloadableItem::from)
                .collect::<Vec<DownloadableItem>>(),
        )
    }
}

#[async_trait]
impl MediaDownloader for YoutubeClient {
    async fn fetch(&self, name: &str, link: &str) -> Result<String, String> {
        let output_dir = format!("home:{}/YouTube", get_movie_dir());
        self.spawner
            .execute(
                name,
                "yt-dlp",
                vec![
                    "--no-update",
                    "--sponsorblock-remove",
                    "all",
                    "--paths",
                    output_dir.as_str(),
                    "-o",
                    "%(title)s.%(ext)s",
                    link,
                ],
            )
            .await;

        Ok(String::from("queued"))
    }

    async fn list_in_progress(&self) -> Result<Vec<Task>, String> {
        Ok(vec![])
    }

    async fn remove(&self, _id: &str, _delete_local_data: bool) -> Result<(), String> {
        Ok(())
    }
}

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
    impl JsonFetcher<YoutubeResponse> for MockFetcher {
        async fn get_json(
            &self,
            url: &str,
            query: &[(&str, &str)],
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
