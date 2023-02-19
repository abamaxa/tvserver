use axum::http::StatusCode;
use async_trait::async_trait;
use crate::domain::models::SearchResults;

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
    async fn search(&self, query: &str) -> Result<SearchResults<T>, reqwest::Error>;
}
