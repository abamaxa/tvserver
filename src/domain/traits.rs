use std::path::Path;
use std::sync::Arc;

use crate::domain::messages::{MediaItem, RemoteMessage, TaskState};
use crate::domain::models::{SearchResults, VideoDetails};
use anyhow;
use async_trait::async_trait;
use axum::http::StatusCode;
use mockall::automock;
use serde::de::DeserializeOwned;
use serde::Serialize;

/*
The following are higher level traits that provide polymorphism
at the service layer.
 */

/// This trait is used to provide an interface to allow the VLC player to be controlled,
/// which was the original video player. Unlike the RemotePlayer interface it doesn't
/// provide an async interface and will be removed in the future.
#[automock]
pub trait Player: Send + Sync {
    fn send_command(&self, command: &str, wait_secs: i32) -> Result<String, String>;
}

/// Interface to a allow searching of a media source, currently implemented
/// for the Youtube Data API and a PirateBay proxy scrapper.
#[async_trait]
pub trait MediaSearcher<T>: Send + Sync {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<T>>;
}

/// Interface to a repository of available media files, currently implemented
/// for the file system but could also support an S3 object store for instance.
#[automock]
#[async_trait]
pub trait MediaStorer: Send + Sync {
    async fn list(&self, collection: &str) -> anyhow::Result<MediaItem>;
    async fn add_file(&self, full_path: &Path) -> anyhow::Result<()>;
    async fn rename(&self, current: &str, new_name: &str) -> anyhow::Result<()>;
    async fn delete(&self, path: &str) -> anyhow::Result<()>;
    async fn check_video_information(&self, repo: Repository) -> anyhow::Result<()>;
    fn as_local_path(&self, collection: &str, video: &str) -> String;
}

pub type Storer = Arc<dyn MediaStorer>;

/// Provides methods for retrieving content, for instance downloading a torrent, or a URL
#[automock]
#[async_trait]
pub trait MediaDownloader: Send + Sync {
    async fn fetch(&self, name: &str, link: &str) -> Result<String, String>;
    async fn list_in_progress(&self) -> Result<Vec<Task>, String>;
    async fn remove(&self, id: &str, delete_local_data: bool) -> Result<(), String>;
}

pub type Downloader = Arc<dyn MediaDownloader>;

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
    async fn send(&self, message: RemoteMessage) -> Result<StatusCode, String>;
}

#[async_trait]
pub trait Filer: Sync + Send {
    fn is_dir(&self) -> bool;
    /*async fn read(&self, count: usize) -> anyhow::Result<Vec<u8>>;
    async fn write(&self, data: Vec<u8>) -> anyhow::Result<()>;
    async fn close(&self);*/
    async fn get_metadata(&self) -> anyhow::Result<VideoDetails>;
    async fn save_metadata(&self, details: VideoDetails) -> anyhow::Result<()>;
}

pub type StoreObject = Arc<dyn Filer>;

/// An interface to a collection of files.
///
/// Unlike the MediaStorer interface, this is a low level interface implemented
/// by an adaptor that knows nothing about videos, media etc, it is meant to be
/// a thin wrapper around a file system, S3 object store etc.
#[async_trait]
pub trait FileStore: Sync + Send {
    async fn create_folder(&self, path: &str) -> anyhow::Result<()>;
    async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)>;
    async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()>;
    async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()>;
    async fn get(&self, path: &str) -> anyhow::Result<StoreObject>;
    async fn delete(&self, path: &str) -> anyhow::Result<()>;
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
    async fn save_video(&self, details: &VideoDetails) -> Result<i64, sqlx::Error>;
    async fn list_collection(&self, collection: &str)  -> Result<Vec<MediaItem>, sqlx::Error>;
    async fn retrieve_video(&self, checksum: i64) -> Result<VideoDetails, sqlx::Error>;
    async fn update_video(&self, details: &VideoDetails) -> Result<u64, sqlx::Error>;
    async fn delete_video(&self, checksum: i64) -> Result<u64, sqlx::Error>;
}

pub type Repository = Arc<dyn Databaser>;

#[async_trait]
pub trait ChannelReceiver<T>: Sync + Send {
    async fn recv(&mut self) -> anyhow::Result<T>;
}

pub type Receiver<T> = Arc<dyn ChannelReceiver<T>>;

pub trait ChannelBroadcaster<T>: Sync + Send + Clone {
    fn subscribe(&self) -> Receiver<T>;
    fn send(&self, message: T) -> anyhow::Result<()>;
}

pub type Broadcaster<T> = Arc<dyn ChannelBroadcaster<T>>;
