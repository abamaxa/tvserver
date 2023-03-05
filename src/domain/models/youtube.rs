use serde::{Deserialize, Serialize};

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
