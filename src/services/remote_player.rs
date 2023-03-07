use crate::domain::messages::{Command, Response};
use crate::domain::traits::RemotePlayer;
use axum::{http::StatusCode, Json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct RemotePlayerService {
    client_map: HashMap<String, Arc<dyn RemotePlayer>>,
    remote_player: Option<Arc<dyn RemotePlayer>>,
}

impl RemotePlayerService {
    pub fn new() -> Self {
        Self {
            client_map: HashMap::<String, Arc<dyn RemotePlayer>>::new(),
            remote_player: None,
        }
    }

    pub fn add(&mut self, key: String, client: Arc<dyn RemotePlayer>) {
        self.client_map.insert(key, client.clone());
        self.remote_player = Some(client);
    }

    pub fn get(&self, key: &str) -> Option<Arc<dyn RemotePlayer>> {
        match self.client_map.get(key) {
            Some(client) => Some(client.clone()),
            _ => self.remote_player.clone(),
        }
    }

    pub fn remove(&mut self, _key: &str) {}

    pub async fn execute_remote(
        remote_players: &RwLock<Self>,
        command: Command,
    ) -> (StatusCode, Json<Response>) {
        let key = command.remote_address.unwrap_or_default();

        let remote_client = match remote_players.read().await.get(&key) {
            Some(client) => client,
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(Response::error("missing remote_address".to_string())),
                )
            }
        };

        match remote_client.send(command.message).await {
            Ok(result) => (result, Json(Response::success("todo".to_string()))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e))),
        }
    }
}
