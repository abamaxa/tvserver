use crate::domain::models::youtube::Item;
use crate::domain::SearchEngineType;
use crate::domain::SearchEngineType::YouTube;
use html_escape::decode_html_entities;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DownloadableItem {
    pub title: String,
    pub description: String,
    pub link: String,
    pub engine: SearchEngineType,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SearchResults<T> {
    pub results: Option<Vec<T>>,
    pub error: Option<String>,
}

impl<T> SearchResults<T> {
    pub fn success(results: Vec<T>) -> Self {
        SearchResults {
            results: Some(results),
            error: None,
        }
    }

    pub fn error(message: &str) -> Self {
        SearchResults {
            error: Some(message.to_string()),
            results: None,
        }
    }
}

impl From<&Item> for DownloadableItem {
    fn from(item: &Item) -> Self {
        Self {
            title: decode_html_entities(&item.snippet.title).to_string(),
            description: item.snippet.description.clone(),
            link: item.id.video_id.to_string(),
            engine: YouTube,
        }
    }
}
