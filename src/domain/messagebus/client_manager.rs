use crate::domain::messages::{PlayerListItem, RemoteMessage};
use crate::domain::traits::{RemotePlayer, SendError};
use std::collections::HashMap;
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
    inner: HashMap<String, Client>,
}

impl MessengerMap {
    // Create a new ClientMap
    pub fn new() -> Self {
        Self {
            inner: HashMap::<String, Client>::new(),
            ..Default::default()
        }
    }

    // Update the timestamp on a given client
    pub fn update_timestamp(&mut self, key: &String) {
        let new_time = SystemTime::now();
        self.inner
            .get_mut(key)
            .map(|client| client.timestamp = new_time);
    }

    pub fn update_last_message(&mut self, key: &String, message: RemoteMessage) {
        let new_time = SystemTime::now();
        self.inner.get_mut(key).map(|client| {
            client.timestamp = new_time;
            client.last_message = Some(message);
        });
    }

    pub fn get_last_message(&self, key: &String) -> Option<RemoteMessage> {
        self.inner.get(key).and_then(|c| c.last_message.clone())
    }

    // Remove Client entries that have a timestamp older than the specified time
    pub async fn remove_old_entries(&mut self, older_than: SystemTime) {
        self.inner.retain(|_, client| client.timestamp > older_than);
    }

    pub fn add_player(&mut self, key: String, client: Arc<dyn RemotePlayer>) {
        self.inner.insert(key.clone(), Client::new_player(&client));
    }

    pub fn add_control(&mut self, key: String, client: Arc<dyn RemotePlayer>) {
        self.inner.insert(key, Client::new_remote_control(&client));
    }

    pub fn get(&self, key: &String) -> Option<Arc<dyn RemotePlayer>> {
        if let Some(entry) = self.inner.get(key) {
            return Some(entry.client.clone());
        }

        None
    }

    pub async fn remove(&mut self, key: &String) {
        if let Some(client) = self.inner.remove(key) {
            if let Err(e) = client.client.send(RemoteMessage::Close(key.clone())).await {
                match e {
                    SendError::Disconnected(msg) => {
                        tracing::info!("client already disconnected {}: {}", key, msg);
                    },
                    SendError::Other(msg) => {
                        tracing::error!("error sending close to {}: {}", key, msg);
                    }
                }
            }
        }
    }

    pub fn list_players(&self) -> Vec<PlayerListItem> {
        self.inner
            .iter()
            .filter_map(|(key, client)| match client.role {
                ClientRole::Player => Some(PlayerListItem {
                    name: key.clone(),
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
        for (key, item) in self.inner.iter() {
            let client_key = key.clone();
            js.spawn(
                (|client: Arc<dyn RemotePlayer>, message: RemoteMessage, key: String| async move {
                    if let Err(err) = client.send(message).await {
                        match err {
                            SendError::Disconnected(msg) => {
                                tracing::debug!("ping to disconnected client {}: {}", key, msg);
                            },
                            SendError::Other(msg) => {
                                tracing::error!("error sending ping to {}: {}", key, msg);
                            }
                        }
                    }
                })(item.client.clone(), message.clone(), client_key),
            );
        }

        // just want to join all here
        while let Some(_) = js.join_next().await {}
    }

    pub async fn send_last_message(&self, to_key: &String) {
        if let Some(destination) = self.inner.get(to_key) {
            for item in self.inner.values() {
                if item.role != ClientRole::Player {
                    continue;
                }
                if let Some(message) = &item.last_message {
                    if let Err(err) = destination.client.send(message.clone()).await {
                        match err {
                            SendError::Disconnected(msg) => {
                                tracing::warn!("could not send last message to disconnected client {}: {}", to_key, msg);
                            },
                            SendError::Other(msg) => {
                                tracing::error!("could not send last message {}: {}", to_key, msg);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn get_all_clients(&self) -> HashMap<String, Arc<dyn RemotePlayer>> {
        self.inner
            .iter()
            .map(|(key, client)| (key.clone(), client.client.clone()))
            .collect()
    }

    pub fn get_clients(&self, exclude: &String) -> Vec<Arc<dyn RemotePlayer>> {
        self.inner
            .iter()
            .filter_map(|(key, item)| {
                if key != exclude {
                    Some(item.client.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

pub type ClientMap = Arc<RwLock<MessengerMap>>;
