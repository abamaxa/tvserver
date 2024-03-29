use super::super::messages::{PlayerListItem, ReceivedRemoteMessage, RemoteMessage, Response};
use super::super::traits::RemotePlayer;
use super::client_manager::{ClientMap, MessengerMap};
use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender};
use axum::{http::StatusCode, Json};
use std::net::SocketAddr;
use std::ops::Sub;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::task::JoinSet;
use tokio::time::sleep;

#[derive(Clone)]
pub struct MessageExchange {
    /*
    Tracks clients that are available to play media, e.g. Samsung TVs.

    Queues:

    File available/changed/deleted
    Remote Message Received
    Task Started/State/Complete
     */
    client_map: ClientMap,
    sender: mpsc::Sender<ReceivedRemoteMessage>,
    receiver: broadcast::Sender<ReceivedRemoteMessage>,
    local_sender: LocalMessageSender,
}

impl MessageExchange {
    pub fn new() -> Self {
        let client_map = Arc::new(RwLock::new(MessengerMap::new()));

        let (sender, mut out_rx) = mpsc::channel::<ReceivedRemoteMessage>(100);

        let (in_tx, _receiver) = broadcast::channel::<ReceivedRemoteMessage>(1);

        let _ = tokio::spawn(
            (|client_map: ClientMap, broadcast: broadcast::Sender<ReceivedRemoteMessage>| async move {
                let _hold = Arc::new(_receiver);
                while let Some(msg) = out_rx.recv().await {
                    if let Err(e) = broadcast.send(msg.clone()) {
                        tracing::error!("could not send remote message: {}, {:?}", e, &msg);
                    }

                    MessageExchange::on_player_message(
                        client_map.clone(),
                        msg.from_address,
                        msg.message,
                    )
                    .await;
                }
            })(client_map.clone(), in_tx.clone()),
        );

        let (local_sender, _) = broadcast::channel::<LocalMessage>(100);

        let _ = tokio::spawn((|client_map: ClientMap| async move {
            loop {
                MessageExchange::check_clients(client_map.clone()).await
            }
        })(client_map.clone()));

        let exchanger = Self {
            client_map: client_map.clone(),
            receiver: in_tx,
            sender,
            local_sender,
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

    pub async fn on_player_message(
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
            _ => Self::dispatch_message(client_map, player_key, message).await,
        };
    }

    async fn dispatch_message(
        client_map: ClientMap,
        player_key: SocketAddr,
        message: RemoteMessage,
    ) {
        let mut clients = vec![];
        {
            let mut map = client_map.write().await;

            clients.extend(map.get_clients(player_key));

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

    pub fn get_sender(&self) -> mpsc::Sender<ReceivedRemoteMessage> {
        self.sender.clone()
    }

    pub fn get_receiver(&self) -> broadcast::Receiver<ReceivedRemoteMessage> {
        self.receiver.subscribe()
    }

    pub fn get_local_sender(&self) -> LocalMessageSender {
        self.local_sender.clone()
    }

    pub fn get_local_receiver(&self) -> LocalMessageReceiver {
        self.local_sender.subscribe()
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
    use tokio::sync::watch;
    use tokio::sync::watch::Receiver;

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

    #[tokio::test]
    async fn test_watch_channels() {
        let (tx, rx) = watch::channel("hello");

        tokio::spawn((|mut rx: Receiver<&'static str>| async move {
            while rx.changed().await.is_ok() {
                println!("listener 1 received = {:?}", *rx.borrow());
            }
        })(rx.clone()));

        tokio::spawn((|mut rx: Receiver<&'static str>| async move {
            while rx.changed().await.is_ok() {
                println!("listener 2 received = {:?}", *rx.borrow());
            }
        })(rx.clone()));

        tx.send("world").unwrap();

        sleep(Duration::from_secs(3)).await;
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
