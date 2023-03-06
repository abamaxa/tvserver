use std::io;
use std::path::Path;

use crate::domain::models::{DownloadListResults, SearchResults, VideoEntry};
use anyhow;
use async_trait::async_trait;
use axum::http::StatusCode;
use mockall::automock;
use serde::de::DeserializeOwned;

#[async_trait]
pub trait RemotePlayer: Send + Sync {
    async fn send_command(&self, command: &str) -> Result<StatusCode, String>;
    fn close(&self);
}

pub trait Player: Send + Sync {
    fn send_command(&self, command: &str, wait_secs: i32) -> Result<String, String>;
}

#[async_trait]
pub trait SearchEngine<T>: Send + Sync {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<T>>;
}

#[automock]
#[async_trait]
pub trait MediaStorer: Send + Sync {
    async fn list(&self, collection: String) -> Result<VideoEntry, io::Error>;
    async fn move_file(&self, path: &Path) -> io::Result<()>;
    fn delete(&self, path: String) -> io::Result<bool>;
    fn as_path(&self, collection: String, video: String) -> String;

    async fn convert_to_mp4(&self, path: &Path) -> anyhow::Result<bool>;
}

#[automock]
#[async_trait]
pub trait DownloadClient: Send + Sync {
    async fn fetch(&self, link: &str) -> Result<String, String>;
    async fn list_in_progress(&self) -> DownloadListResults;
    async fn remove(&self, id: i64, delete_local_data: bool) -> Result<(), String>;
}

#[async_trait]
pub trait TextFetcher: Send + Sync {
    async fn get_text(&self, url: &str) -> anyhow::Result<String>;
}

#[async_trait]
pub trait JsonFetcher<T: DeserializeOwned>: Send + Sync {
    async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> anyhow::Result<T>;
}

#[async_trait]
pub trait StoreReaderWriter {
    async fn list_directory(&self, path: &Path) -> anyhow::Result<(Vec<String>, Vec<String>)>;
    async fn ensure_path_exists(&self, path: &Path) -> anyhow::Result<()>;
    async fn rename(&self, old_path: &Path, new_path: &Path) -> anyhow::Result<()>;
}
