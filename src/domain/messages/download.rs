use crate::domain::SearchEngineType;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::LocalMessageSender;
use super::messages::{LocalMessage, MediaEvent};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub name: String,
    pub link: String,
    pub engine: SearchEngineType,
    pub series: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    // All in MB, MB/s
    pub total_size: Option<i64>,
    pub downloaded_size: i64,
    pub uploaded_size: Option<i64>,
    pub finished: bool,
    pub error_message: String,
    pub progress_message: String,
    pub files: Vec<String>,
}

impl DownloadInfo {
    pub async fn send_media_messages(&self, sender: &LocalMessageSender, skip_file: impl Fn(&str) -> bool, series: Option<String>) {
        tracing::info!("Sending media messages for {} files", self.files.len());
        for file in &self.files {
            if skip_file(file) {
                continue;
            }
            
            let media_event = MediaEvent::new_media(Path::new(file), series.clone());
            tracing::info!("Sending media event: {:?}", media_event);
            sender.send(LocalMessage::Media(media_event)).await.unwrap();
        }
    }
}
