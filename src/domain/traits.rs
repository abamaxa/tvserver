use std::io;
use std::path::Path;

use axum::http::StatusCode;
use async_trait::async_trait;
use anyhow;
use serde::de::DeserializeOwned;
use crate::domain::models::{DownloadListResults, SearchResults, VideoEntry};

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

#[async_trait]
pub trait MediaStorer: Send + Sync {
    fn list(&self, collection: String) -> Result<VideoEntry, io::Error>;
    fn move_file(&self, path: &Path) -> io::Result<()>;
    fn delete(&self, path: String) -> io::Result<bool>;
    fn as_path(&self, collection: String, video: String) -> String;

    async fn convert_to_mp4(&self, path: &Path) -> anyhow::Result<bool>;
}

#[async_trait]
pub trait DownloadClient: Send + Sync {
    async fn add(&self, link: &str) -> Result<String, String>;
    async fn list(&self) -> Result<DownloadListResults, DownloadListResults>;
    async fn delete(&self, id: i64, delete_local_data: bool) -> Result<(), String>;
}

#[async_trait]
pub trait TextFetcher: Send + Sync {
    async fn get_text(&self, url: &str) -> anyhow::Result<String>;
}

#[async_trait]
pub trait JsonFetcher<T: DeserializeOwned>: Send + Sync {
    async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> anyhow::Result<T>;
}
