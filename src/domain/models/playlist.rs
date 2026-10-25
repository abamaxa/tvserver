use serde::Serialize;

use super::VideoDetails;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub item_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetails {
    pub id: String,
    pub name: String,
    pub videos: Vec<VideoDetails>,
}
