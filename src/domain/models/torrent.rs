use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::algorithm::generate_display_name;
use crate::domain::config::{delay_reaping_tasks, get_torrent_dir};
use crate::domain::messages::TaskState;
use bytesize::ByteSize;
use serde::{Deserialize, Serialize};
use transmission_rpc::types::{Torrent, TorrentStatus};

use crate::domain::models::SearchResults;
use crate::domain::traits::{Storer, TaskMonitor};
use crate::domain::TaskType;

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
                "mpeg" | "mpg" | "mp4" | "avi" | "mkv" | "mp3" | "webm" | "ogg"
            ),
            None => false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TorrentTask {
    /*
    Represents a request to download media. It may represent 1 or more files which
    have been queued for downloading; in the process of downloading; or finished downloading.
     */
    activity_date: i64,
    added_date: i64,
    done_date: i64,
    edit_date: i64,
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
    pub display_name: String,
}

impl From<&Torrent> for TorrentTask {
    fn from(t: &Torrent) -> Self {
        let download_finished = matches!(
            t.status,
            Some(TorrentStatus::QueuedToSeed) | Some(TorrentStatus::Seeding)
        );

        let download_dir = get_torrent_dir(t.download_dir.as_ref());

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
            downloaded_size: TorrentTask::make_byte_size(Some(downloaded_size)),
            activity_date: t.activity_date.unwrap_or(0),
            added_date: t.added_date.unwrap_or(0),
            done_date: t.done_date.unwrap_or(0),
            edit_date: t.edit_date.unwrap_or(0),
            eta: t.eta.unwrap_or(0),
            id: t.id.unwrap_or(0),
            left_until_done: TorrentTask::make_byte_size(t.left_until_done),
            percent_done: t.percent_done.unwrap_or(0f32),
            peers_connected: t.peers_connected.unwrap_or(0),
            peers_getting_from_us: t.peers_getting_from_us.unwrap_or(0),
            peers_sending_to_us: t.peers_sending_to_us.unwrap_or(0),
            rate_download: TorrentTask::make_byte_size(t.rate_download),
            rate_upload: TorrentTask::make_byte_size(t.rate_upload),
            total_size: TorrentTask::make_byte_size(t.total_size),
            hash_string: match t.hash_string.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
            name: match t.name.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
            display_name: generate_display_name(&t.name),
            error_string: match t.error_string.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
        }
    }
}

impl TorrentTask {
    pub fn has_finished_downloading(&self) -> bool {
        self.download_finished
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

#[async_trait]
impl TaskMonitor for TorrentTask {
    async fn get_state(&self) -> TaskState {
        TaskState {
            key: self.id.to_string(),
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            finished: self.has_finished(),
            eta: self.eta,
            percent_done: self.percent_done,
            size_details: format!("{}/{}", self.downloaded_size, self.total_size),
            error_string: self.error_string.clone(),
            rate_details: format!("{}/{}", self.rate_download, self.rate_upload),
            process_details: format!(
                "Peers: {} connected (\u{2193}{}/\u{2191}{})",
                self.peers_connected, self.peers_sending_to_us, self.peers_getting_from_us
            ),
            task_type: TaskType::Transmission,
        }
    }

    fn get_key(&self) -> String {
        format!("{}", self.id)
    }

    fn get_seconds_since_finished(&self) -> i64 {
        if self.download_finished {
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs()
                .try_into()
                .unwrap_or(self.done_date + delay_reaping_tasks() + 1);

            now_secs - self.done_date
        } else {
            0
        }
    }

    fn terminate(&self) {
        // could simply do TransmissionDaemon::new().remove(self.get_key()) but prefer to
        // avoid a hard coded dependency on TransmissionDaemon, prefer to receive an instance
        // of MediaDownloader and call remove(self.get_key()) on that.
        todo!()
    }

    fn has_finished(&self) -> bool {
        self.download_finished
    }

    async fn cleanup(&self, store: &Storer, force_delete: bool) -> Result<()> {
        // TODO: we don't delay cleanup because we want to make the downloaded video
        // available asap, however that means the completed task never appears on the task
        // list. Investigate sending a notification over the web socket when a task completes
        // and avoid delaying cleanup.
        if !force_delete && !self.has_finished_downloading() {
            return Err(anyhow!("download hasn't finished yet"));
        }

        for item in &self.files {
            if !item.is_media() {
                tracing::debug!("not moving {} as it it not a video file", item.name);
                continue;
            }

            store.move_file(&item.filepath).await?;
        }
        Ok(())
    }
}

pub type TaskListResults = SearchResults<TaskState>;

#[cfg(test)]
pub mod test {
    use crate::domain::models::TorrentTask;
    use crate::domain::traits::{MockMediaStorer, Storer, TaskMonitor};

    use anyhow::Result;
    use serde::Deserialize;
    use std::{path::PathBuf, sync::Arc};
    use tokio::fs;
    use transmission_rpc::types::Torrent;

    #[derive(Deserialize)]
    struct TorrentGetResult {
        pub torrents: Vec<Torrent>,
    }

    #[test]
    fn test_make_bytes_size() {
        let test_cases = [
            (1, "1 B"),
            (1024, "1.0 KB"),
            (1000, "1.0 KB"),
            (1000000, "1000.0 KB"),
            (500, "500 B"),
            (15000000, "15.0 MB"),
        ];

        for (num, expected) in test_cases {
            assert_eq!(&TorrentTask::make_byte_size(Some(num)), expected);
        }
    }

    #[tokio::test]
    async fn test_move_videos() -> Result<()> {
        let completed_torrent = torrents_from_fixture("completed_torrent_get.json")
            .await?
            .first()
            .expect("couldn't find torrent in fixture")
            .to_owned();

        let download = TorrentTask::from(&completed_torrent);

        assert!(download.has_finished_downloading());

        let mut store = MockMediaStorer::new();

        store.expect_move_file().times(1).returning(|_| Ok(()));

        let store: Storer = Arc::new(store);

        assert!(download.cleanup(&store, true).await.is_ok());

        Ok(())
    }

    pub async fn torrents_from_fixture(name: &str) -> Result<Vec<Torrent>> {
        let mut path = PathBuf::from("tests/fixtures");
        path.push(name);

        let data = fs::read(&path).await?;

        let result: TorrentGetResult = serde_json::from_slice(&data)?;

        Ok(result.torrents)
    }
}
