use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::messages::{MediaItem, RemoteMessage, TaskState};
use super::models::{BookDetails, CollectionItem, DownloadableItem, SearchResults, VideoDetails};
use anyhow;
use async_trait::async_trait;
use axum::http::StatusCode;
use mockall::automock;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::algorithm::file_integrity::FileSeal;

/*
The following are higher level traits that provide polymorphism
at the service layer.
 */

/// Interface to a allow searching of a media source, currently implemented
/// for the Youtube Data API and a PirateBay proxy scrapper.
#[async_trait]
pub trait MediaSearcher<T>: Send + Sync {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<T>>;
}

pub type Searcher = Arc<dyn MediaSearcher<DownloadableItem>>;

/// Interface to a repository of available media files, currently implemented
/// for the file system but could also support an S3 object store for instance.
#[automock]
#[async_trait]
pub trait MediaStorer: Send + Sync {
    async fn list(&self, collection: &str) -> anyhow::Result<MediaItem>;
    async fn add_file(&self, full_path: &Path, suggested_series: Option<String>) -> anyhow::Result<PathBuf>;
    async fn delete(&self, video_id: String) -> anyhow::Result<()>;
}

pub type Storer = Arc<dyn MediaStorer>;

/// Provides methods for checking the integrity of a video file.
#[automock]
#[async_trait]
pub trait MediaChecker: Send + Sync {
    async fn check_video_information(&self) -> anyhow::Result<()>;
}

pub type Checker = Arc<dyn MediaChecker>;

/// Provides a separate boundary for checking book files and metadata state.
#[automock]
#[async_trait]
pub trait BookChecker: Send + Sync {
    async fn check_book_information(&self) -> anyhow::Result<()>;
}

pub type BookCheckerHandle = Arc<dyn BookChecker>;


/// Provides an interface to observe the progress of a download
#[automock]
#[async_trait]
pub trait DownloadProgress: Send + Sync {
    fn terminate(&self);
    async fn observe(&self) -> super::messages::DownloadInfo;
}

pub type DownloadProgressMonitor = Arc<dyn DownloadProgress>;


#[automock]
#[async_trait]
pub trait Download: Send + Sync {
    async fn download(&self, request: super::messages::DownloadRequest) -> Result<DownloadProgressMonitor, anyhow::Error>;
}

pub type Downloader = Arc<dyn Download>;

/*
The following are low level traits implemented at the adaptor layer
 */

/// Provides an interface to retrieving text in the form of a String
/// e.g. by executing an HTTP GET on a url, or opening and reading a text file.
#[automock]
#[async_trait]
pub trait TextFetcher: Send + Sync {
    async fn get_text(&self, url: &str) -> anyhow::Result<String>;
}

/// Provides an interface to retrieve JSON data and return a struct containing
/// that data. The interface is parameterized by the type of struct to return,
/// with much implement the DeserializeOwned trait (e.g. by deriving serde Deserialize)
#[async_trait]
pub trait JsonFetcher<'a, T: DeserializeOwned, Q: Serialize>: Send + Sync {
    async fn fetch(&self, url: &str, query: &'a Q) -> anyhow::Result<T>;
}

// &[(&str, &str)]

/// Interface to control the browser based video player via a websocket.
#[automock]
#[async_trait]
pub trait RemotePlayer: Send + Sync {
    async fn send(&self, message: RemoteMessage) -> Result<StatusCode, SendError>;
}

/// Error type for the RemotePlayer trait's send method
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("Channel is disconnected: {0}")]
    Disconnected(String),
    
    #[error("Other error: {0}")]
    Other(String),
}

#[async_trait]
pub trait Filer: Sync + Send {
    fn is_dir(&self) -> bool;
    async fn get_metadata(&self) -> anyhow::Result<VideoDetails>;
    async fn save_metadata(&self, details: VideoDetails) -> anyhow::Result<()>;
}

pub type StoreObject = Arc<dyn Filer>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedFile {
    pub original_path: PathBuf,
    pub staged_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateSnapshot {
    pub path: PathBuf,
    pub(crate) id: u128,
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateSnapshotFingerprint {
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
}

impl PrivateSnapshot {
    pub(crate) fn new(path: PathBuf, id: u128, device: u64, inode: u64) -> Self {
        Self {
            path,
            id,
            device,
            inode,
        }
    }
}

fn unsupported_no_follow_listing(path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    anyhow::bail!("strict no-follow listing is not supported for {path}")
}

/// An interface to a collection of files.
///
/// Unlike the MediaStorer interface, this is a low level interface implemented
/// by an adaptor that knows nothing about videos, media etc, it is meant to be
/// a thin wrapper around a file system, S3 object store etc.
#[async_trait]
pub trait FileStore: Sync + Send {
    async fn create_folder(&self, path: &Path) -> anyhow::Result<()>;
    async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)>;
    async fn list_folder_no_follow(
        &self,
        path: &str,
    ) -> anyhow::Result<(Vec<String>, Vec<String>)> {
        unsupported_no_follow_listing(path)
    }
    async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()>;
    async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()>;
    async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()>;
    async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile>;
    async fn create_private_snapshot(
        &self,
        staged: &StagedFile,
    ) -> anyhow::Result<PrivateSnapshot>;
    async fn private_snapshot_fingerprint(
        &self,
        _snapshot: &PrivateSnapshot,
    ) -> anyhow::Result<PrivateSnapshotFingerprint> {
        anyhow::bail!("private snapshot fingerprinting is not supported by this file store")
    }
    async fn seal_private_snapshot(
        &self,
        snapshot: &PrivateSnapshot,
    ) -> anyhow::Result<FileSeal>;
    async fn private_snapshot_matches_regular_no_follow(
        &self,
        _snapshot: &PrivateSnapshot,
        _path: &Path,
    ) -> anyhow::Result<bool> {
        anyhow::bail!("private snapshot comparison is not supported by this file store")
    }
    async fn publish_private_snapshot_no_replace(
        &self,
        snapshot: &PrivateSnapshot,
        destination: &str,
        expected_seal: &FileSeal,
    ) -> anyhow::Result<()>;
    async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> anyhow::Result<()>;
    async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool>;
    async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()>;
    async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()>;
    async fn publish_staged_no_replace(
        &self,
        staged: &StagedFile,
        destination: &str,
    ) -> anyhow::Result<()>;
    async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()>;
    async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()>;
    async fn get(&self, path: &str) -> anyhow::Result<StoreObject>;
    async fn delete(&self, path: &str) -> anyhow::Result<()>;
    async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_follow_listing_default_is_explicitly_unsupported() {
        let error = unsupported_no_follow_listing("Shelf").unwrap_err();

        assert!(error
            .to_string()
            .contains("strict no-follow listing is not supported"));
    }
}

pub type FileStorer = Arc<dyn FileStore>;

/// Provides a common interface to obtain the state of a task and terminate it.
///
/// In this context, a task is a long running task, like a downloading or converting
/// a video
#[automock]
#[async_trait]
pub trait TaskMonitor: Sync + Send {
    async fn get_state(&self) -> TaskState;
    fn get_key(&self) -> String;
    fn get_seconds_since_finished(&self) -> i64;
    fn terminate(&self);
    fn has_finished(&self) -> bool;
    async fn cleanup(&self, store: &Arc<dyn MediaStorer>, force_delete: bool)
        -> anyhow::Result<()>;
    /// Wait until the task finishes. Default implementation polls every 2 seconds.
    async fn wait_finished(&self) {
        while !self.has_finished() {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }
}

pub type Task = Arc<dyn TaskMonitor>;

/// Spawns a new os process.
#[async_trait]
pub trait ProcessSpawner: Sync + Send {
    async fn execute(&self, name: &str, cmd: &str, args: Vec<&str>) -> Task;
}

pub type Spawner = Arc<dyn ProcessSpawner>;

#[async_trait]
pub trait Databaser: Sync + Send {
    async fn save_book(&self, details: &BookDetails) -> Result<i64, sqlx::Error>;
    async fn list_book_collections(
        &self,
        collection: &str,
    ) -> Result<Vec<String>, sqlx::Error>;
    async fn list_books(&self, collection: &str) -> Result<Vec<BookDetails>, sqlx::Error>;
    async fn list_all_books(&self) -> Result<Vec<BookDetails>, sqlx::Error>;
    async fn retrieve_book(&self, checksum: i64) -> Result<BookDetails, sqlx::Error>;
    async fn delete_book(&self, checksum: i64) -> Result<u64, sqlx::Error>;
    async fn delete_book_if_path_matches(
        &self,
        checksum: i64,
        collection: &str,
        file_name: &str,
    ) -> Result<u64, sqlx::Error>;
    async fn save_video(&self, details: &VideoDetails) -> Result<i64, sqlx::Error>;
    async fn list_collection(&self, collection: &str)  -> Result<Vec<String>, sqlx::Error>;
    async fn list_videos(&self, collection: &str)  -> Result<Vec<VideoDetails>, sqlx::Error>;
    async fn list_all_series(&self) -> Result<Vec<CollectionItem>, sqlx::Error>;
    async fn list_series_details(&self, series: &str, season: Option<&str>) -> Result<Vec<VideoDetails>, sqlx::Error>;
    async fn retrieve_video(&self, checksum: i64) -> Result<VideoDetails, sqlx::Error>;
    async fn delete_video(&self, checksum: i64) -> Result<u64, sqlx::Error>;
    async fn update_watched_video(&self, checksum: i64, current_time: f64) -> Result<(), sqlx::Error>;
    async fn get_history(&self, offset: i32, limit: i32) -> Result<Vec<VideoDetails>, sqlx::Error>;
    async fn list_all_videos(&self) -> Result<Vec<VideoDetails>, sqlx::Error>;
}

pub type Repository = Arc<dyn Databaser>;


#[async_trait]
pub trait MediaSharer: Sync + Send {
    async fn share(&self, series_or_id: &str) -> anyhow::Result<()>;
}

pub type Sharer = Arc<dyn MediaSharer>;

#[async_trait]
pub trait InstantMessegeService: Sync + Send {
	async fn send_link(&self, title: &str, link: &str) -> anyhow::Result<()>;
	async fn send_error(&self, video: String, error: &str) -> anyhow::Result<()>;
}

pub type InstantMessenger = Arc<dyn InstantMessegeService>;
