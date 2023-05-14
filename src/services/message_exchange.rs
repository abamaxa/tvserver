use crate::domain::messages::{PlayerListItem, ReceivedRemoteMessage, RemoteMessage, Response};
use crate::domain::traits::RemotePlayer;
use crate::services::message_exchange::ClientRole::{Player, RemoteControl};
use axum::{http::StatusCode, Json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Sub;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio::time::sleep;

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
            role: Player,
            timestamp: SystemTime::now(),
            last_message: None,
        }
    }

    pub fn new_remote_control(client: &Arc<dyn RemotePlayer>) -> Self {
        Self {
            client: client.clone(),
            role: RemoteControl,
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
            if client.role == Player {
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
                Player => Some(PlayerListItem {
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
                if item.role != Player {
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
}

type ClientMap = Arc<RwLock<MessengerMap>>;
type ObserverMap = Arc<RwLock<HashMap<SocketAddr, Vec<SocketAddr>>>>;

#[derive(Clone)]
pub struct MessageExchange {
    /*
    Tracks clients that are available to play media, e.g. Samsung TVs.
     */
    client_map: ClientMap,
    observers: ObserverMap,
    sender: Sender<ReceivedRemoteMessage>,
    //messenger_handle: JoinHandle<()>,
    //checker_handle: JoinHandle<()>,
}

impl MessageExchange {
    pub fn new() -> Self {
        let observers = Arc::new(RwLock::new(HashMap::<SocketAddr, Vec<SocketAddr>>::new()));
        let client_map = Arc::new(RwLock::new(MessengerMap::new()));

        let (sender, mut out_rx) = channel::<ReceivedRemoteMessage>(100);

        let _ = tokio::spawn((|observers: ObserverMap, client_map: ClientMap| async move {
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

        let _ = tokio::spawn((|client_map: ClientMap| async move {
            loop {
                MessageExchange::check_clients(client_map.clone()).await
            }
        })(client_map.clone()));

        let exchanger = Self {
            client_map: client_map.clone(),
            observers: observers.clone(),
            sender,
        };

        exchanger
    }

    pub async fn add_player(&self, key: SocketAddr, client: Arc<dyn RemotePlayer>) {
        self.client_map.write().await.add_player(key, client)
    }

    pub async fn add_control(&self, key: SocketAddr, client: Arc<dyn RemotePlayer>) {
        self.client_map.write().await.add_control(key, client);
    }

    pub async fn get(&self, key: SocketAddr) -> Option<Arc<dyn RemotePlayer>> {
        self.client_map.read().await.get(key)
    }

    pub async fn remove(&self, key: SocketAddr) {
        self.client_map.write().await.remove(key).await
    }

    pub async fn list_players(&self) -> Vec<PlayerListItem> {
        self.client_map.read().await.list_players()
    }

    pub async fn check_clients(client_map: ClientMap) {
        client_map.read().await.ping_all().await;

        sleep(Duration::from_secs(15)).await;

        client_map
            .write()
            .await
            .remove_old_entries(SystemTime::now().sub(Duration::from_secs(90)))
            .await;
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
        let _ = match message {
            RemoteMessage::Pong(who) => client_map.write().await.update_timestamp(&who),
            RemoteMessage::Close(who) => client_map.write().await.remove(who).await,
            RemoteMessage::SendLastState => {
                client_map.read().await.send_last_message(player_key).await
            }
            _ => Self::dispatch_message(_observers, client_map, player_key, message).await,
        };
    }

    async fn dispatch_message(
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
            let mut map = observers.write().await;
            if let Some(keys) = map.get(&player_key) {
                client_keys = Some(keys.clone());
            }
            map.update_last_message(message);
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
            let mut map = client_map.write().await;
            for (key, item) in map.inner.iter() {
                if *key != player_key {
                    clients.push(item.client.clone());
                }
            }
            map.update_last_message(&player_key, message.clone());
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
        assert_eq!(players[0].name, addr.to_string());
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
