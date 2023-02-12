use axum::http::StatusCode;
use async_trait::async_trait;

#[async_trait]
pub trait RemotePlayer: Send + Sync {
    async fn send_command(&self, command: &str) -> Result<StatusCode, String>;
    fn close(&self);
}