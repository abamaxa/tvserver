use std::io;
use std::path::Path;
use std::sync::Arc;

use crate::domain::messages::{RemoteMessage, TaskState};
use crate::domain::models::{SearchResults, VideoEntry};
use anyhow;
use async_trait::async_trait;
use axum::http::StatusCode;
use mockall::automock;
use serde::de::DeserializeOwned;

/*
The following are higher level traits that provide polymorphism
at the service layer.
 */
#[automock]
pub trait Player: Send + Sync {
    /*
    This trait is used to provide an interface to allow the VLC player to be controlled,
    which was the original video player. Unlike the RemotePlayer interface it doesn't
    provide an async interface and will be removed in the future.
    */
    fn send_command(&self, command: &str, wait_secs: i32) -> Result<String, String>;
}

#[async_trait]
pub trait MediaSearcher<T>: Send + Sync {
    /*
    Interface to a allow searching of a media source, currently implemented
    for the Youtube Data API and a PirateBay proxy scrapper.
     */
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<T>>;
}

#[automock]
#[async_trait]
pub trait MediaStorer: Send + Sync {
    /*
    Interface to a repository of available media files, currently implemented
    for the file system but could also support an S3 object store for instance.
     */
    async fn list(&self, collection: &str) -> Result<VideoEntry, io::Error>;
    async fn move_file(&self, path: &Path) -> io::Result<()>;
    async fn rename(&self, current: &str, new_name: &str) -> io::Result<()>;
    async fn delete(&self, path: &str) -> io::Result<bool>;
    fn as_path(&self, collection: &str, video: &str) -> String;

    async fn convert_to_mp4(&self, path: &Path) -> anyhow::Result<bool>;
}

pub type Storer = Arc<dyn MediaStorer>;

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

#[automock]
#[async_trait]
pub trait TextFetcher: Send + Sync {
    /*
    Provides an interface to retrieving text in the form of a String
    e.g. by executing an HTTP GET on a url, or opening and reading a text file.
     */
    async fn get_text(&self, url: &str) -> anyhow::Result<String>;
}

#[async_trait]
pub trait JsonFetcher<T: DeserializeOwned>: Send + Sync {
    /*
    Provides an interface to retrieve JSON data and return a struct containing
    that data. The interface is parameterized by the type of struct to return,
    with much implement the DeserializeOwned trait (e.g. by deriving serde Deserialize)
     */
    async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> anyhow::Result<T>;
}

#[automock]
#[async_trait]
pub trait RemotePlayer: Send + Sync {
    /*
    Interface to control the browser based video player via a websocket.
     */
    async fn send(&self, message: RemoteMessage) -> Result<StatusCode, String>;
}

#[async_trait]
pub trait StoreReaderWriter {
    /*
    An interface to a collection of files.

    Unlike the MediaStorer interface, this is a low level interface implemented
    by an adaptor that knows nothing about videos, media etc, it is meant to be
    a thin wrapper around a file system, S3 object store etc.
     */
    async fn list_directory(&self, path: &Path) -> anyhow::Result<(Vec<String>, Vec<String>)>;
    async fn ensure_path_exists(&self, path: &Path) -> anyhow::Result<()>;
    async fn rename(&self, old_path: &Path, new_path: &Path) -> anyhow::Result<()>;
}

#[automock]
#[async_trait]
pub trait TaskMonitor: Sync + Send {
    /*
    Provides a common interface to obtain the state of a task and terminate it.

    In this context, a task is a long running task, like a downloading or converting
    a video
     */
    async fn get_state(&self) -> TaskState;
    fn get_key(&self) -> String;
    fn get_seconds_since_finished(&self) -> i64;
    fn terminate(&self);
    fn has_finished(&self) -> bool;
    async fn cleanup(&self, store: &Arc<dyn MediaStorer>) -> anyhow::Result<()>;
}

pub type Task = Arc<dyn TaskMonitor>;

#[async_trait]
pub trait ProcessSpawner: Sync + Send {
    /*
    Spawns a new os process.
     */
    async fn execute(&self, name: &str, cmd: &str, args: Vec<&str>) -> Task;
}

pub type Spawner = Arc<dyn ProcessSpawner>;
