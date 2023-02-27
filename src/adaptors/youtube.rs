use async_trait::async_trait;
use reqwest::{header::{CONTENT_TYPE, ACCEPT}, StatusCode};
use serde::{Deserialize, Serialize};
use html_escape::decode_html_entities;
use crate::adaptors::subprocess::AsyncCommand;
use crate::domain::config::get_movie_dir;
use crate::domain::SearchEngineType::YOUTUBE;
use crate::domain::models::{SearchResults, DownloadableItem, DownloadListResults};
use crate::domain::traits::{DownloadClient, SearchEngine};

const SEARCH_URL: &str = "https://www.googleapis.com/youtube/v3/search";

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeResponse {
    pub kind: String,
    pub etag: String,
    pub next_page_token: String,
    pub region_code: String,
    pub page_info: PageInfo,
    pub items: Vec<Item>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub total_results: i64,
    pub results_per_page: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub kind: String,
    pub etag: String,
    pub id: Id,
    pub snippet: Snippet,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id {
    pub kind: String,
    pub video_id: String,
    //pub channel_id: Option<String>,
    //pub playlist_id: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub published_at: String,
    pub channel_id: String,
    pub title: String,
    pub description: String,
    pub thumbnails: Thumbnails,
    pub channel_title: String,
    pub live_broadcast_content: String,
    pub publish_time: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnails {
    pub default: Option<ThumbnailDetails>,
    pub medium: Option<ThumbnailDetails>,
    pub high: Option<ThumbnailDetails>,
    pub standard: Option<ThumbnailDetails>,
    pub maxres: Option<ThumbnailDetails>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailDetails {
    pub url: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

pub struct YoutubeClient {
    key: String,
}

#[async_trait]
impl SearchEngine<DownloadableItem> for YoutubeClient {
    async fn search(&self, query: &str) -> Result<SearchResults<DownloadableItem>, reqwest::Error> {
        let client = reqwest::Client::new();

        let response = client.get(SEARCH_URL)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .query(&[
                ("q", query),
                ("key", &self.key),
                ("part", "snippet"),
                ("maxResults", "50"),
                ("type", "video")
            ])
            .send()
            .await?;

        //let html = response.text().await?;
        //println!("{}", html);

        return match response.status() {
            StatusCode::OK => match response.json::<YoutubeResponse>().await {
                Ok(parsed) => Ok(self.make_success_response(parsed)),
                Err(err) => Ok(SearchResults::error(&err.to_string())),
            },
            StatusCode::UNAUTHORIZED => Ok(SearchResults::error("need to grab a new token")),
            _ => Ok(SearchResults::error(&format!("error, code: {}", response.status()))),
        }
    }
}

impl YoutubeClient {
    pub fn new(key: &str) -> Self {
        Self{ key: String::from(key) }
    }

    fn make_success_response(&self, yt_response: YoutubeResponse) -> SearchResults<DownloadableItem> {
        SearchResults::success(
            yt_response.items
                .iter()
                .map(|r| {
                    DownloadableItem{
                        title: decode_html_entities(&r.snippet.title).to_string(),
                        description: r.snippet.description.clone(),
                        link: r.id.video_id.to_string(),
                        engine: YOUTUBE,
                    }
                })
                .collect::<Vec<DownloadableItem>>()
        )
    }
}

#[async_trait]
impl DownloadClient for YoutubeClient {
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
    use crate::domain::GOOGLE_KEY;
    use super::*;

    #[tokio::test]
    async fn test_search_youtube() {
        let pc = YoutubeClient::new(&env::var(GOOGLE_KEY).unwrap());

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