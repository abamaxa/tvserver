use serde::{Deserialize, Serialize};
use transmission_rpc::types::{Torrent, TorrentStatus};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DownloadableItem {
    pub title: String,
    pub description: String,
    pub link: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SearchResults<T> {
    pub results: Option<Vec<T>>,
    pub error: Option<String>,
}


impl<T> SearchResults<T> {
    pub fn success(results: Vec<T>) -> Self {
        SearchResults{results: Some(results), error: None}
    }

    pub fn error(message: &str) -> Self {
        SearchResults{error: Some(message.to_string()), results: None}
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileDetails {
    length: i64,
    bytes_completed: i64,
    name: String,
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
    id: i64,
    peers_connected: i64,
    size_when_done: i64,
    download_dir: String,
    hash_string: String,
    files: Vec<FileDetails>,

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

        let files = match &t.files {
            Some(files) => files.iter().map(|item| {
                FileDetails{
                    length: item.length,
                    bytes_completed: item.bytes_completed,
                    name: item.name.clone(),
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
            download_dir: match t.download_dir.as_ref() {
                Some(val) => val.clone(),
                None => String::new(),
            },
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

    pub fn move_files_to_movie_folder(&self) {

    }

    pub fn delete(&self) {

    }
}

pub type TorrentListResults = SearchResults<DownloadProgress>;


/*use serde::{Deserialize, Serialize};


#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
}

#[derive(Debug, Serialize,Deserialize, Clone, Eq, Hash, PartialEq, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
}


impl User {
    pub fn laugh(&self) -> String {
        String::from("ha ha")
    }
}*/

