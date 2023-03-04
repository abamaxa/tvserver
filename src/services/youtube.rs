use async_trait::async_trait;
use anyhow;
use html_escape::decode_html_entities;
use crate::adaptors::subprocess::AsyncCommand;
use crate::domain::config::get_movie_dir;
use crate::domain::SearchEngineType::YouTube;
use crate::domain::models::{SearchResults, DownloadableItem, DownloadListResults, YoutubeResponse};
use crate::domain::traits::{DownloadClient, JsonFetcher, SearchEngine};

const SEARCH_URL: &str = "https://www.googleapis.com/youtube/v3/search";
const SEARCH_PART: &str = "snippet";
const SEARCH_MAX_RESULTS: &str = "50";
const SEARCH_TYPE: &str = "video";


type YoutubeResult = SearchResults<DownloadableItem>;
type Fetcher = dyn JsonFetcher<YoutubeResponse>;

pub struct YoutubeClient<'a> {
    key: String,
    client: &'a Fetcher,
}

#[async_trait]
impl<'a> SearchEngine<DownloadableItem> for YoutubeClient<'a> {
    async fn search(&self, query: &str) -> anyhow::Result<YoutubeResult> {
        let query = [
            ("q", query),
            ("key", &self.key),
            ("part", SEARCH_PART),
            ("maxResults", SEARCH_MAX_RESULTS),
            ("type", SEARCH_TYPE)
        ];

        match self.client.get_json(SEARCH_URL, &query).await {
            Ok(results) => Ok(self.make_success_response(results)),
            Err(err) => Ok(SearchResults::error(&err.to_string())),
        }
    }
}

impl<'a> YoutubeClient<'a> {
    pub fn new(key: &str, client: &'a Fetcher) -> Self {
        Self{ key: String::from(key), client }
    }

    fn make_success_response(&self, yt_response: YoutubeResponse) -> YoutubeResult {
        SearchResults::success(
            yt_response.items
                .iter()
                .map(|r| {
                    DownloadableItem{
                        title: decode_html_entities(&r.snippet.title).to_string(),
                        description: r.snippet.description.clone(),
                        link: r.id.video_id.to_string(),
                        engine: YouTube,
                    }
                })
                .collect::<Vec<DownloadableItem>>()
        )
    }
}

#[async_trait]
impl<'a> DownloadClient for YoutubeClient<'a> {
    async fn add(&self, link: &str) -> Result<String, String> {
        let output_dir = format!("home:{}/New", get_movie_dir());
        AsyncCommand::execute("yt-dlp", vec![
            "--no-update",
            "--sponsorblock-remove", "all",
            "--paths",
            output_dir.as_str(),
            link]);

        Ok(String::from("queued"))
    }

    async fn list(&self) -> Result<DownloadListResults, DownloadListResults> {
        todo!()
    }

    async fn delete(&self, _id: i64, _delete_local_data: bool) -> Result<(), String> {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use std::env;
    use anyhow::anyhow;
    use crate::adaptors::HTTPClient;
    use crate::domain::GOOGLE_KEY;
    use crate::domain::models::youtube::{Id, Item, Snippet};
    use super::*;

    #[derive(Default, Debug, Clone)]
    struct MockFetcher{
        response: Option<YoutubeResponse>,
        error: Option<String>
    }

    #[async_trait]
    impl JsonFetcher<YoutubeResponse> for MockFetcher {
        async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> anyhow::Result<YoutubeResponse> {
            match &self.response {
                Some(response) => Ok(response.clone()),
                None => match &self.error {
                    Some(error) => Err(anyhow!(error.clone())),
                    None => {
                        let items = query
                            .iter()
                            .map(|(k, v)| Item{
                                snippet: Snippet{
                                    title: k.to_string(),
                                    description: v.to_string(),
                                    ..Default::default()
                                },
                                id: Id{ video_id: url.to_string(), ..Default::default()},
                                ..Default::default()
                            })
                            .collect();

                        Ok(YoutubeResponse{ items, ..Default::default() })
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_conversion_of_response() -> anyhow::Result<()> {

        const THE_QUERY: &str = "find this";
        const THE_KEY: &str = "the key";

        let fetcher = MockFetcher{..Default::default()};

        let client: &dyn SearchEngine<DownloadableItem> = &YoutubeClient::new(THE_KEY, &fetcher);

        let response = client.search(THE_QUERY).await?;

        let results = response.results.ok_or(anyhow!("expected results"))?;

        assert_eq!(results.len(), 5);

        for item in results {
            assert_eq!(item.description, match item.title.as_str() {
                "q" =>THE_QUERY,
                "key" => THE_KEY,
                "part" => SEARCH_PART,
                "maxResults" => SEARCH_MAX_RESULTS,
                "type" => SEARCH_TYPE,
                _ => panic!("unexpected query parameter: {}: {}", item.title, item.description)
            });

            assert_eq!(item.link, SEARCH_URL);
        }

        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_live_search_youtube() {
        let client: &Fetcher = &HTTPClient::new();
        let pc = YoutubeClient::new(&env::var(GOOGLE_KEY).unwrap(), client);

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
            },
            Err(e) => panic!("error: {}", e.to_string()),
        };
    }


}