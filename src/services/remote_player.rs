use crate::domain::messages::{Command, Response};
use crate::domain::traits::RemotePlayer;
use axum::{http::StatusCode, Json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default, Clone)]
pub struct RemotePlayerService {
    /*
    Tracks clients that are available to play media, e.g. Samsung TVs.
     */
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

    pub fn remove(&mut self, key: &str) {
        self.client_map.remove(key);
        if self.client_map.is_empty() {
            self.remote_player = None;
        }
    }

    pub fn list(&self) -> Vec<String> {
        self.client_map.keys().cloned().collect()
    }

    pub async fn execute(
        remote_players: &RwLock<Self>,
        command: Command,
    ) -> (StatusCode, Json<Response>) {
        // This function does not take self as we don't want to hold a read lock
        // while performing i/o
        let key = command.remote_address.unwrap_or_default();

        // hold the lock for as short a time as possible.
        let remote_client = match remote_players.read().await.get(&key) {
            Some(client) => client,
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(Response::error("no players have connected yet".to_string())),
                )
            }
        };

        // send the command over a websocket to be received by a browser, which should
        // execute the command.
        match remote_client.send(command.message).await {
            Ok(result) => (result, Json(Response::success("success".to_string()))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e))),
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::domain::traits::{MockRemotePlayer, RemotePlayer};

    #[test]
    fn test_container() {
        let key1 = String::from("key1");
        let key2 = String::from("key2");

        let mut rp = RemotePlayerService::new();

        let player1 = MockRemotePlayer::new();

        let player1: Arc<dyn RemotePlayer> = Arc::new(player1);

        assert!(rp.remote_player.is_none());

        rp.add(key1.clone(), player1);

        assert!(rp.remote_player.is_some());

        let player2 = MockRemotePlayer::new();

        let player2: Arc<dyn RemotePlayer> = Arc::new(player2);

        rp.add(key2.clone(), player2);

        let players = rp.list();

        assert!(players.contains(&key1));

        rp.remove(&key1);

        rp.remove(&key2);

        assert!(rp.remote_player.is_none());
    }
}
