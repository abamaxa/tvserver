use crate::domain::messages::{PlayerListItem, RemoteMessage};
use crate::domain::traits::RemotePlayer;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;
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
    timestamp: SystemTime,
    last_message: Option<RemoteMessage>,
}

impl Client {
    pub fn new_player(client: &Arc<dyn RemotePlayer>) -> Self {
        Self {
            client: client.clone(),
            role: ClientRole::Player,
            timestamp: SystemTime::now(),
            last_message: None,
        }
    }

    pub fn new_remote_control(client: &Arc<dyn RemotePlayer>) -> Self {
        Self {
            client: client.clone(),
            role: ClientRole::RemoteControl,
            timestamp: SystemTime::now(),
            last_message: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct MessengerMap {
    inner: HashMap<SocketAddr, Client>,
    default_player: Option<Arc<dyn RemotePlayer>>,
    default_player_key: Option<SocketAddr>,
}

impl MessengerMap {
    // Create a new ClientMap
    pub fn new() -> Self {
        Self {
            inner: HashMap::<SocketAddr, Client>::new(),
            ..Default::default()
        }
    }

    // Update the timestamp on a given client
    pub fn update_timestamp(&mut self, addr: &SocketAddr) {
        let new_time = SystemTime::now();
        self.inner
            .get_mut(addr)
            .map(|client| client.timestamp = new_time);
    }

    pub fn update_last_message(&mut self, addr: &SocketAddr, message: RemoteMessage) {
        let new_time = SystemTime::now();
        self.inner.get_mut(addr).map(|client| {
            client.timestamp = new_time;
            client.last_message = Some(message);
        });
    }

    // Remove Client entries that have a timestamp older than the specified time
    pub async fn remove_old_entries(&mut self, older_than: SystemTime) {
        self.inner.retain(|_, client| client.timestamp > older_than);
    }

    pub fn add_player(&mut self, key: SocketAddr, client: Arc<dyn RemotePlayer>) {
        self.inner.insert(key, Client::new_player(&client));
        self.default_player = Some(client);
        self.default_player_key = Some(key);
    }

    pub fn add_control(&mut self, key: SocketAddr, client: Arc<dyn RemotePlayer>) {
        self.inner.insert(key, Client::new_remote_control(&client));
    }

    pub fn get(&self, key: SocketAddr) -> Option<Arc<dyn RemotePlayer>> {
        if let Some(entry) = self.inner.get(&key) {
            return Some(entry.client.clone());
        }

        self.default_player.clone()
    }

    pub async fn remove(&mut self, key: SocketAddr) {
        let mut clear_default = false;

        if let Some(client) = self.inner.remove(&key) {
            if client.role == ClientRole::Player {
                // TODO: should check if no more players in Map, or even
                clear_default = self.inner.is_empty();
            }

            if let Err(e) = client.client.send(RemoteMessage::Close(key)).await {
                tracing::info!("error sending close to {}: {}", key, e);
            }
        }

        if clear_default || self.default_player_key == Some(key) {
            self.default_player = None;
            self.default_player_key = None;
        }
    }

    pub fn list_players(&self) -> Vec<PlayerListItem> {
        self.inner
            .iter()
            .filter_map(|(key, client)| match client.role {
                ClientRole::Player => Some(PlayerListItem {
                    name: key.to_string(),
                    last_message: client.last_message.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    pub async fn ping_all(&self) {
        let ping_msg = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(n) => n.as_secs(),
            Err(_) => 1,
        };

        let message = RemoteMessage::Ping(ping_msg);

        let mut js = JoinSet::new();
        for item in self.inner.values() {
            js.spawn(
                (|client: Arc<dyn RemotePlayer>, message: RemoteMessage| async move {
                    client.send(message).await
                })(item.client.clone(), message.clone()),
            );
        }

        // just want to join all here
        while let Some(_) = js.join_next().await {}
    }

    pub async fn send_last_message(&self, to_host: SocketAddr) {
        if let Some(destination) = self.inner.get(&to_host) {
            for item in self.inner.values() {
                if item.role != ClientRole::Player {
                    continue;
                }
                if let Some(message) = &item.last_message {
                    if let Err(err) = destination.client.send(message.clone()).await {
                        tracing::error!("could not send last message {}, {}", to_host, err);
                    }
                }
            }
        }
    }

    pub fn get_clients(&self, exclude: SocketAddr) -> Vec<Arc<dyn RemotePlayer>> {
        self.inner
            .iter()
            .filter_map(|(key, item)| {
                if *key != exclude {
                    Some(item.client.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

pub type ClientMap = Arc<RwLock<MessengerMap>>;
