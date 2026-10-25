use crate::domain::models::{DownloadableItem, SearchResults, YoutubeResponse};
use crate::domain::traits::{JsonFetcher, MediaSearcher};
use anyhow;
use async_trait::async_trait;
use std::sync::Arc;

const SEARCH_URL: &str = "https://www.googleapis.com/youtube/v3/search";
const SEARCH_PART: &str = "snippet";
const SEARCH_MAX_RESULTS: &str = "50";
const SEARCH_TYPE: &str = "video";

pub type YoutubeResult = SearchResults<DownloadableItem>;
pub type YoutubeFetcher =
    Arc<dyn for<'a> JsonFetcher<'a, YoutubeResponse, &'a [(&'a str, &'a str)]>>;

pub struct YoutubeClient {
    key: String,
    client: YoutubeFetcher,
}

#[async_trait]
impl MediaSearcher<DownloadableItem> for YoutubeClient {
    async fn search(&self, query: &str) -> anyhow::Result<YoutubeResult> {
        if query.trim().is_empty() { return Ok(SearchResults::success(vec![])); }
        if self.key.trim().is_empty() {
            return match keyless_search(query).await {
                Ok(results) => Ok(SearchResults::success(results)),
                Err(error) => Ok(SearchResults::error(&error.to_string())),
            };
        }
        let query = [
            ("q", query),
            ("key", &self.key),
            ("part", SEARCH_PART),
            ("maxResults", SEARCH_MAX_RESULTS),
            ("type", SEARCH_TYPE),
        ];
        let q: &[(&str, &str)] = &query;
        match self.client.fetch(SEARCH_URL, &q).await {
            Ok(results) => Ok(self.make_success_response(results)),
            Err(err) => Ok(SearchResults::error(&err.to_string())),
        }
    }
}

async fn keyless_search(query: &str) -> anyhow::Result<Vec<DownloadableItem>> {
    let mut command = crate::adaptors::youtube_runtime::command().await?;
    let query = format!("ytsearch25:{}", query.trim().chars().take(200).collect::<String>());
    command.args.extend([
        "--flat-playlist", "--dump-single-json", "--skip-download", "--quiet",
        "--no-warnings", "--socket-timeout", "15", "--extractor-retries", "1", "--", &query,
    ].into_iter().map(String::from));
    let output = tokio::time::timeout(std::time::Duration::from_secs(45),
        tokio::process::Command::new(command.program).args(command.args).kill_on_drop(true).output()
    ).await.map_err(|_| anyhow::anyhow!("YouTube search timed out; please try again"))??;
    anyhow::ensure!(output.status.success(), "YouTube search failed: {}",
        String::from_utf8_lossy(&output.stderr).chars().take(600).collect::<String>());
    parse_keyless_results(&output.stdout)
}

fn parse_keyless_results(data: &[u8]) -> anyhow::Result<Vec<DownloadableItem>> {
    let response: serde_json::Value = serde_json::from_slice(data)?;
    let entries = response["entries"].as_array().ok_or_else(|| anyhow::anyhow!("YouTube returned an invalid search response"))?;
    Ok(entries.iter().filter_map(|entry| {
        let id = entry["id"].as_str()?;
        if id.len() != 11 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) { return None; }
        let title = entry["title"].as_str()?.trim();
        if title.is_empty() { return None; }
        Some(DownloadableItem {
            title: title.into(),
            description: entry["description"].as_str().or_else(|| entry["channel"].as_str()).unwrap_or_default().into(),
            link: format!("https://www.youtube.com/watch?v={id}"),
            engine: crate::domain::SearchEngineType::YouTube,
        })
    }).take(25).collect())
}

impl YoutubeClient {
    pub fn new(key: &str, client: YoutubeFetcher) -> Self {
        Self {
            key: String::from(key),
            client,
        }
    }

    fn make_success_response(&self, yt_response: YoutubeResponse) -> YoutubeResult {
        SearchResults::success(
            yt_response
                .items
                .iter()
                .filter(|item| item.id.video_id.is_some())
                .map(DownloadableItem::from)
                .collect::<Vec<DownloadableItem>>(),
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::adaptors::HTTPClient;
    use crate::domain::config::get_google_key;
    use crate::domain::models::{Id, Item, Snippet};
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
                video_id: Some(url.to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_conversion_of_response() -> anyhow::Result<()> {
        const THE_QUERY: &str = "find this";
        const THE_KEY: &str = "the key";

        let fetcher = MockFetcher {
            ..Default::default()
        };

        let client: &dyn MediaSearcher<DownloadableItem> =
            &YoutubeClient::new(THE_KEY, Arc::new(fetcher));

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

        let client: &dyn MediaSearcher<DownloadableItem> =
            &YoutubeClient::new("configured-key", Arc::new(fetcher));

        let response = client.search("test query").await;

        assert!(response.is_ok());

        let results = response.unwrap();

        assert_eq!(&results.error.unwrap().to_string(), ERROR_MESSAGE);

        Ok(())
    }

    #[test]
    fn keyless_results_ignore_non_video_entries_and_preserve_downloadable_urls() {
        let response = serde_json::json!({"entries": [
            null,
            {"id": "UC-channel", "title": "Channel"},
            {"id": "BaW_jenozKc", "title": "Test video", "channel": "Test author"},
            {"id": "bad/id!", "title": "Invalid result"}
        ]});
        let results = parse_keyless_results(&serde_json::to_vec(&response).unwrap()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test video");
        assert_eq!(results[0].link, "https://www.youtube.com/watch?v=BaW_jenozKc");
        assert_eq!(results[0].description, "Test author");
    }

    #[tokio::test]
    async fn keyless_empty_query_returns_no_results_without_a_runtime_or_api_key() {
        let client = YoutubeClient::new("", Arc::new(MockFetcher::default()));
        assert!(client.search("  ").await.unwrap().results.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_live_search_youtube() {
        let client = Arc::new(HTTPClient::new());
        let pc = YoutubeClient::new(&get_google_key(), client);

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
