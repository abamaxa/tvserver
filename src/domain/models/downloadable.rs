use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use transmission_rpc::types::{Torrent, TorrentStatus};
use crate::domain::config::get_torrents_dir;

use crate::domain::models::SearchResults;
use crate::domain::traits::VideoStore;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileDetails {
    length: i64,
    bytes_completed: i64,
    name: String,
    filepath: PathBuf,
}

impl FileDetails {
    fn is_video(&self) -> bool {
        match self.filepath.extension() {
            Some(extension) => match extension.to_str().unwrap_or_default() {
                "mpeg" | "mpg" | "mp4" | "avi" | "mkv" => true,
                _ => false,
            }
            None => false,
        }
    }

    fn should_convert_to_mp4(&self) -> bool {
        match self.filepath.extension() {
            Some(extension) => match extension.to_str().unwrap_or_default() {
                "avi" => true,
                _ => false,
            }
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
    eta: i64,
    error: i64,
    peers_connected: i64,
    size_when_done: i64,
    download_dir: String,
    hash_string: String,
    files: Vec<FileDetails>,

    pub id: i64,
    pub name: String,
    pub total_size: i64,
    pub percent_done: f32,
    pub left_until_done: i64,
    pub peers_sending_to_us: i64,
    pub peers_getting_from_us: i64,
    pub rate_download: i64,
    pub rate_upload: i64,
    pub download_finished: bool,
}

impl DownloadProgress {

    pub fn from(t: &Torrent) -> Self {

        let download_finished = match t.status {
            Some(TorrentStatus::QueuedToSeed) | Some(TorrentStatus::Seeding) => true,
            _ => false,
        };

        let download_dir = match t.download_dir.as_ref() {
            Some(val) => format!("{}{}", get_torrents_dir(), val),
            None => String::new(),
        };

        let files = match &t.files {
            Some(files) => files.iter().map(|item| {
                let filepath = PathBuf::from(&download_dir)
                    .join(item.name.clone());

                FileDetails{
                    length: item.length,
                    bytes_completed: item.bytes_completed,
                    name: item.name.clone(),
                    filepath: filepath,
                }
            }).collect(),
            None => vec![],
        };

        Self {
            download_finished: download_finished,
            activity_date: t.activity_date.unwrap_or(0),
            added_date: t.added_date.unwrap_or(0),
            done_date: t.done_date.unwrap_or(0),
            edit_date: t.edit_date.unwrap_or(0),
            eta: t.eta.unwrap_or(0),
            error: t.error.unwrap_or(0),
            id: t.id.unwrap_or(0),
            left_until_done: t.left_until_done.unwrap_or(0),
            percent_done: t.percent_done.unwrap_or(0f32),
            peers_connected: t.peers_connected.unwrap_or(0),
            peers_getting_from_us: t.peers_getting_from_us.unwrap_or(0),
            peers_sending_to_us: t.peers_sending_to_us.unwrap_or(0),
            rate_download: t.rate_download.unwrap_or(0),
            rate_upload: t.rate_upload.unwrap_or(0),
            total_size: t.total_size.unwrap_or(0),
            size_when_done: t.size_when_done.unwrap_or(0),
            files: files,
            download_dir: download_dir,
            hash_string: match t.hash_string.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
            name: match t.name.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
        }
    }

    pub fn has_finished_downloading(&self) -> bool {
        self.download_finished
    }

    pub async fn move_videos(&self, store: &Arc<dyn VideoStore>) -> Result<()> {
        for item in &self.files {
            if !item.is_video() {
                println!("not moving {} as it it not a video file", item.name);
                continue;
            }

            if item.should_convert_to_mp4() {
                store.convert_to_mp4(&item.filepath).await?;
            } else {
                store.move_file(&item.filepath)?;
            }
        }
        Ok(())
    }

}

pub type TorrentListResults = SearchResults<DownloadProgress>;
