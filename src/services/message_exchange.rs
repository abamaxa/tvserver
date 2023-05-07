use crate::domain::messages::{ReceivedRemoteMessage, RemoteMessage, Response};
use crate::domain::traits::RemotePlayer;
use crate::services::message_exchange::ClientRole::Player;
use axum::{http::StatusCode, Json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::RwLock;
use tokio::task::JoinSet;

#[derive(Debug, Clone, PartialEq)]
enum ClientRole {
    Player = 0,
    RemoteControl = 1,
}

#[derive(Clone)]
pub struct Client {
    client: Arc<dyn RemotePlayer>,
    role: ClientRole,
}

type MessengerMap = HashMap<SocketAddr, Client>;
type ClientMap = Arc<RwLock<MessengerMap>>;
type ObserverMap = Arc<RwLock<HashMap<SocketAddr, Vec<SocketAddr>>>>;

#[derive(Clone)]
pub struct MessageExchange {
    /*
    Tracks clients that are available to play media, e.g. Samsung TVs.
     */
    client_map: ClientMap,
    observers: ObserverMap,
    default_player: Arc<RwLock<Option<Arc<dyn RemotePlayer>>>>,
    sender: Sender<ReceivedRemoteMessage>,
}

impl MessageExchange {
    pub fn new() -> Self {
        let observers = Arc::new(RwLock::new(HashMap::<SocketAddr, Vec<SocketAddr>>::new()));
        let client_map = Arc::new(RwLock::new(MessengerMap::new()));

        let (sender, mut out_rx) = channel::<ReceivedRemoteMessage>(100);

        tokio::spawn((|observers: ObserverMap, client_map: ClientMap| async move {
            while let Some(msg) = out_rx.recv().await {
                MessageExchange::on_player_message(
                    observers.clone(),
                    client_map.clone(),
                    msg.from_address,
                    msg.message,
                )
                .await;
            }
        })(observers.clone(), client_map.clone()));

        let exchanger = Self {
            client_map: client_map.clone(),
            observers: observers.clone(),
            default_player: Arc::new(RwLock::new(None)),
            sender: sender,
        };

        exchanger
    }

    pub async fn add_player(&self, key: SocketAddr, client: Arc<dyn RemotePlayer>) {
        self.client_map.write().await.insert(
            key,
            Client {
                client: client.clone(),
                role: Player,
            },
        );
        *self.default_player.write().await = Some(client);
    }

    pub async fn add_control(&self, key: SocketAddr, client: Arc<dyn RemotePlayer>) {
        self.client_map.write().await.insert(
            key,
            Client {
                client: client.clone(),
                role: ClientRole::RemoteControl,
            },
        );
    }

    pub async fn get(&self, key: SocketAddr) -> Option<Arc<dyn RemotePlayer>> {
        {
            let map = self.client_map.read().await;
            if let Some(entry) = map.get(&key) {
                return Some(entry.client.clone());
            }
        }

        self.default_player.read().await.clone()
    }

    pub async fn remove(&self, key: SocketAddr) {
        let mut clear_default = false;

        {
            let mut map = self.client_map.write().await;
            if let Some(client) = map.remove(&key) {
                if client.role == Player {
                    // TODO: should check if no more players in Map, or even
                    clear_default = map.is_empty();
                }
            }
        }

        if clear_default {
            *self.default_player.write().await = None;
        }
    }

    pub async fn list_players(&self) -> Vec<String> {
        self.client_map
            .read()
            .await
            .iter()
            .filter_map(|(key, client)| match client.role {
                Player => Some(key.to_string()),
                _ => None,
            })
            .collect()
    }

    pub async fn observe_player(&self, player_key: SocketAddr, client_key: SocketAddr) {
        let mut map = self.observers.write().await;
        if let Some(observers) = map.get_mut(&player_key) {
            if !observers.contains(&client_key) {
                observers.push(client_key);
            }
        } else {
            map.insert(player_key, vec![client_key]);
        }
    }

    pub async fn on_player_message(
        _observers: ObserverMap,
        client_map: ClientMap,
        player_key: SocketAddr,
        message: RemoteMessage,
    ) {
        let mut clients = vec![];
        /*
        For now we will push every message received to every
        other client

        let mut client_keys = None;

        {
            let map = observers.read().await;
            if let Some(keys) = map.get(&player_key) {
                client_keys = Some(keys.clone());
            }
        }

        if let Some(keys) = client_keys {
            {
                let map = client_map.read().await;
                for key in keys.iter() {
                    if let Some(item) = map.get(key) {
                        clients.push(item.client.clone());
                    }
                }
            }
        }*/
        {
            let map = client_map.read().await;
            for (key, item) in map.iter() {
                if *key != player_key {
                    clients.push(item.client.clone());
                }
            }
        }

        if !clients.is_empty() {
            let mut result_set = JoinSet::new();
            for client in clients.into_iter() {
                let message = message.clone();
                result_set.spawn(async move {
                    match client.send(message).await {
                        Ok(result) => (result, Json(Response::success("success".to_string()))),
                        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e))),
                    }
                });
            }

            // just want to join all here
            while let Some(_) = result_set.join_next().await {}
        }
    }

    pub async fn execute(
        &self,
        key: SocketAddr,
        command: RemoteMessage,
    ) -> (StatusCode, Json<Response>) {
        // hold the lock for as short a time as possible.
        let remote_client = match self.get(key).await {
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
        match remote_client.send(command).await {
            Ok(result) => (result, Json(Response::success("success".to_string()))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e))),
        }
    }

    pub fn get_sender(&self) -> Sender<ReceivedRemoteMessage> {
        self.sender.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::messages::RemoteMessage;
    use crate::domain::traits::RemotePlayer;
    use async_trait::async_trait;
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::sync::mpsc::{channel, Sender};

    #[tokio::test]
    async fn test_add_player() {
        let message_exchange = MessageExchange::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let remote_player = MockRemotePlayer::new(addr);

        message_exchange
            .add_player(addr, remote_player.clone())
            .await;

        let players = message_exchange.list_players().await;
        assert_eq!(players.len(), 1);
        assert_eq!(players[0], addr.to_string());
    }

    #[tokio::test]
    async fn test_remove_player() {
        let message_exchange = MessageExchange::new();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let remote_player = MockRemotePlayer::new(addr);

        message_exchange
            .add_player(addr, remote_player.clone())
            .await;
        message_exchange.remove(addr).await;

        let players = message_exchange.list_players().await;
        assert_eq!(players.len(), 0);
    }

    struct MockRemotePlayer {
        _address: SocketAddr,
        sender: Sender<RemoteMessage>,
    }

    impl MockRemotePlayer {
        fn new(address: SocketAddr) -> Arc<Self> {
            let (sender, _) = channel::<RemoteMessage>(100);
            Arc::new(Self {
                _address: address,
                sender,
            })
        }
    }

    #[async_trait]
    impl RemotePlayer for MockRemotePlayer {
        async fn send(&self, message: RemoteMessage) -> Result<StatusCode, String> {
            self.sender.send(message).await.map_err(|e| e.to_string())?;
            Ok(StatusCode::OK)
        }
    }
}
