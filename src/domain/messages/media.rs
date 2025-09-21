use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use chrono::{NaiveDate, Utc};
use crate::domain::models::{CollectionDetails, VideoDetails};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAdded {
    pub full_path: PathBuf,
    pub search: Option<String>,
    pub date: Option<NaiveDate>,
}

impl MediaAdded {
    pub fn new(path: &Path, search: Option<String>) -> Self {
        Self {
            full_path: PathBuf::from(path),
            search: search,
            date: Some(NaiveDate::from(Utc::now().naive_utc())),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMoved {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
}

/*
This event is generated when a file is downloaded, renamed, deleted and is used to trigger
copying the file into the MediaStore, metadata generation and notifications to remote clients.
 */
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaEvent {
    MediaAvailable(MediaAdded),
    MediaMoved(MediaMoved),
    MediaDeleted(PathBuf),
}

impl MediaEvent {
    pub fn new_media(path: &Path, search: Option<String>) -> Self {
        Self::MediaAvailable(MediaAdded::new(path, search))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MediaItem {
    Collection(CollectionDetails),
    Video(VideoDetails),
    Error(String),
}

impl MediaItem {
    pub fn error(message: &str) -> Self {
        Self::Error(message.to_string())
    }
}

impl From<std::io::Error> for MediaItem {
    fn from(value: std::io::Error) -> Self {
        Self::Error(value.to_string())
    }
}

impl From<anyhow::Error> for MediaItem {
    fn from(value: anyhow::Error) -> Self {
        Self::Error(value.to_string())
    }
}