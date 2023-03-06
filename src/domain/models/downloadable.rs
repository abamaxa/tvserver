use anyhow::Result;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use bytesize::ByteSize;
use serde::{Deserialize, Serialize};
use transmission_rpc::types::{Torrent, TorrentStatus};

use crate::domain::models::SearchResults;
use crate::domain::traits::MediaStorer;
use crate::domain::TORRENT_DIR;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileDetails {
    length: i64,
    bytes_completed: i64,
    name: String,
    filepath: PathBuf,
}

impl FileDetails {
    fn is_media(&self) -> bool {
        match self.filepath.extension() {
            Some(extension) => matches!(
                extension.to_str().unwrap_or_default(),
                "mpeg" | "mpg" | "mp4" | "avi" | "mkv" | "mp3" | "webm"
            ),
            None => false,
        }
    }

    fn should_convert_to_mp4(&self) -> bool {
        match self.filepath.extension() {
            Some(extension) => matches!(extension.to_str().unwrap_or_default(), "avi"),
            None => false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    activity_date: i64,
    added_date: i64,
    done_date: i64,
    edit_date: i64,
    error: i64,
    download_dir: String,
    hash_string: String,
    files: Vec<FileDetails>,

    pub download_finished: bool,
    pub downloaded_size: String,
    pub error_string: String,
    pub eta: i64,
    pub id: i64,
    pub left_until_done: String,
    pub name: String,
    pub peers_connected: i64,
    pub peers_sending_to_us: i64,
    pub peers_getting_from_us: i64,
    pub percent_done: f32,
    pub rate_download: String,
    pub rate_upload: String,
    pub total_size: String,
}

impl DownloadProgress {
    pub fn from(t: &Torrent) -> Self {
        let download_finished = matches!(
            t.status,
            Some(TorrentStatus::QueuedToSeed) | Some(TorrentStatus::Seeding)
        );

        let download_dir = match env::var(TORRENT_DIR) {
            Ok(val) => val,
            Err(_) => match t.download_dir.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
        };

        let files = match &t.files {
            Some(files) => files
                .iter()
                .map(|item| {
                    let filepath = PathBuf::from(&download_dir).join(item.name.clone());

                    FileDetails {
                        length: item.length,
                        bytes_completed: item.bytes_completed,
                        name: item.name.clone(),
                        filepath,
                    }
                })
                .collect(),
            None => vec![],
        };

        let downloaded_size = t.total_size.unwrap_or(0) - t.left_until_done.unwrap_or(0);

        Self {
            download_finished,
            download_dir,
            files,
            downloaded_size: DownloadProgress::make_byte_size(Some(downloaded_size)),
            activity_date: t.activity_date.unwrap_or(0),
            added_date: t.added_date.unwrap_or(0),
            done_date: t.done_date.unwrap_or(0),
            edit_date: t.edit_date.unwrap_or(0),
            eta: t.eta.unwrap_or(0),
            error: t.error.unwrap_or(0),
            id: t.id.unwrap_or(0),
            left_until_done: DownloadProgress::make_byte_size(t.left_until_done),
            percent_done: t.percent_done.unwrap_or(0f32),
            peers_connected: t.peers_connected.unwrap_or(0),
            peers_getting_from_us: t.peers_getting_from_us.unwrap_or(0),
            peers_sending_to_us: t.peers_sending_to_us.unwrap_or(0),
            rate_download: DownloadProgress::make_byte_size(t.rate_download),
            rate_upload: DownloadProgress::make_byte_size(t.rate_upload),
            total_size: DownloadProgress::make_byte_size(t.total_size),
            hash_string: match t.hash_string.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
            name: match t.name.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
            error_string: match t.error_string.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
        }
    }

    pub fn has_finished_downloading(&self) -> bool {
        self.download_finished
    }

    pub async fn move_videos(&self, store: &Arc<dyn MediaStorer>) -> Result<()> {
        for item in &self.files {
            if !item.is_media() {
                tracing::debug!("not moving {} as it it not a video file", item.name);
                continue;
            }

            if item.should_convert_to_mp4() {
                store.convert_to_mp4(&item.filepath).await?;
            } else {
                store.move_file(&item.filepath).await?;
            }
        }
        Ok(())
    }

    fn make_byte_size(value: Option<i64>) -> String {
        let mut uval: u64 = 0;
        if let Some(v) = value {
            if let Ok(v) = v.try_into() {
                uval = v;
            }
        }
        ByteSize(uval).to_string()
    }
}

pub type DownloadListResults = SearchResults<DownloadProgress>;
