use crate::domain::messages::{LocalMessageReceiver, LocalMessageSender};

use super::super::messages::{LocalMessage, PlayerListItem, ReceivedRemoteMessage, RemoteMessage, Response, RemotePlayerState};
use super::super::traits::{RemotePlayer, SendError};
use super::client_manager::{ClientMap, MessengerMap};
use axum::{http::StatusCode, Json};
use std::ops::Sub;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinSet;
use tokio::time::sleep;

#[derive(Debug, thiserror::Error)]
pub enum MessageExchangeError {
    #[error("No players connected")]
    NoPlayers,
    #[error("Internal server error")]
    ExecuteError,
}

#[derive(Clone)]
pub struct MessageExchange {
    /*
    Tracks clients that are available to play media, e.g. Samsung TVs.
     */
    client_map: ClientMap,
    incoming_sender: mpsc::Sender<ReceivedRemoteMessage>,
}

impl MessageExchange {
    pub fn new(local_sender:LocalMessageSender, local_receiver:LocalMessageReceiver) -> Self {
        let client_map = Arc::new(RwLock::new(MessengerMap::new()));
        let (incoming_sender, mut incoming_receiver) = mpsc::channel::<ReceivedRemoteMessage>(100);

        let _ = tokio::spawn(
            (|client_map: ClientMap| async move {
                while let Some(msg) = incoming_receiver.recv().await {
                    MessageExchange::on_player_message(
                        client_map.clone(),
                        msg.from_address,
                        msg.message,
                        local_sender.clone(),
                    )
                    .await;
                }
            })(client_map.clone()),
        );

        #[cfg(feature = "webserver")]
        let _ = tokio::spawn((|client_map: ClientMap| async move {
            loop {
                MessageExchange::check_clients(client_map.clone()).await
            }
        })(client_map.clone()));

        let exchanger = Self {
            client_map: client_map.clone(),
            incoming_sender,
        };

        // Start processing local messages
        tokio::spawn(async move {
            MessageExchange::process_local_messages(client_map.clone(), local_receiver).await;
        });

        exchanger
    }

    pub async fn add_player(&self, key: String, client: Arc<dyn RemotePlayer>) {
        self.client_map.write().await.add_player(key, client)
    }

    pub async fn add_control(&self, key: String, client: Arc<dyn RemotePlayer>) {
        self.client_map.write().await.add_control(key, client);
    }

    pub async fn get(&self, key: &String) -> Option<Arc<dyn RemotePlayer>> {
        self.client_map.read().await.get(key)
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
        player_key: String,
        message: RemoteMessage,
        _local_sender: LocalMessageSender,
    ) {
        let _ = match message {
            RemoteMessage::Ping(who) => _ = who,
            RemoteMessage::Pong(who) => client_map.write().await.update_timestamp(&who.to_string()),
            RemoteMessage::Close(who) => client_map.write().await.remove(&who).await,
            RemoteMessage::SendLastState => {
                client_map.read().await.send_last_message(&player_key).await
            }
            RemoteMessage::State(state) => {
                if let Err(e) = _local_sender.send(LocalMessage::PlayerState(state.clone())).await {
                    tracing::error!("Error sending player state: {}", e);
                }
                Self::dispatch_message(client_map, player_key, RemoteMessage::State(state)).await
            }
            RemoteMessage::SetPlaybackRate { rate } => {
                // Merge into last known state and broadcast as State
                let prev = match client_map.read().await.get_last_message(&player_key) {
                    Some(RemoteMessage::State(last)) => Some(last.clone()),
                    _ => None,
                };
                let merged = RemotePlayerState::merge_playback_rate(prev, rate);
                if let Err(e) = _local_sender.send(LocalMessage::PlayerState(merged.clone())).await {
                    tracing::error!("Error sending player state: {}", e);
                }
                Self::dispatch_message(client_map, player_key, RemoteMessage::State(merged)).await
            }
            RemoteMessage::SetSubtitleTrack { track_id } => {
                let prev = match client_map.read().await.get_last_message(&player_key) {
                    Some(RemoteMessage::State(last)) => Some(last.clone()),
                    _ => None,
                };
                let merged = RemotePlayerState::merge_subtitle_track(prev, track_id);
                if let Err(e) = _local_sender.send(LocalMessage::PlayerState(merged.clone())).await {
                    tracing::error!("Error sending player state: {}", e);
                }
                Self::dispatch_message(client_map, player_key, RemoteMessage::State(merged)).await
            }
            _ => Self::dispatch_message(client_map, player_key, message).await,
        };
    }

    async fn dispatch_message(
        client_map: ClientMap,
        player_key: String,
        message: RemoteMessage,
    ) {
        let mut clients = vec![];
        {
            let mut map = client_map.write().await;

            clients.extend(map.get_clients(&player_key));

            map.update_last_message(&player_key, message.clone());
        }

        if !clients.is_empty() {
            let mut result_set = JoinSet::new();
            for client in clients.into_iter() {
                let message = message.clone();
                result_set.spawn(async move {
                    match client.send(message).await {
                        Ok(result) => (result, Json(Response::success("success".to_string()))),
                        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e.to_string()))),
                    }
                });
            }

            // just want to join all here
            while let Some(_) = result_set.join_next().await {}
        }
    }

    pub async fn execute(&self, key: String, command: RemoteMessage) -> Result<(), MessageExchangeError> {
        // hold the lock for as short a time as possible.
        let remote_client = match self.get(&key).await {
            Some(client) => client,
            _ => {
                MessageExchange::broadcast_to_all(&self.client_map, command).await;
                return Ok(());
            }
        };

        // send the command over a websocket to be received by a browser, which should
        // execute the command.
        tracing::info!("Sending command to {}: {:?}", &key, command);
        match remote_client.send(command).await {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!("Error sending message to {}: {}", &key, e);
                Err(MessageExchangeError::ExecuteError)
            }
        }
    }

    pub fn get_sender(&self) -> mpsc::Sender<ReceivedRemoteMessage> {
        self.incoming_sender.clone()
    }

    pub async fn process_local_messages(client_map: ClientMap, mut local_rx: LocalMessageReceiver) {
        loop {
            match local_rx.recv().await {
                Ok(message) => {
                    MessageExchange::on_local_message(&client_map, message).await;
                }
                Err(e) => {
                    tracing::error!("Error receiving local message: {}", e);
                    break;
                }
            }
        }
    }

    async fn on_local_message(client_map: &ClientMap, message: LocalMessage) {
        match message {
            LocalMessage::Media(media_event) => {
                let remote_message = RemoteMessage::Command { 
                    command: format!("media_event:{:?}", media_event) 
                };
                
                // Broadcast to all clients
                MessageExchange::broadcast_to_all(client_map, remote_message).await;
            }
            LocalMessage::Task(tasks) => {
                let remote_message = RemoteMessage::CurrentTasks(tasks.clone());
                
                // Broadcast to all clients
                MessageExchange::broadcast_to_all(client_map, remote_message).await;
            },
            LocalMessage::Video(video_event) => {
                let remote_message = RemoteMessage::Video(vec![video_event.clone()]);
                
                // Broadcast to all clients
                MessageExchange::broadcast_to_all(client_map, remote_message).await;
            },
            LocalMessage::PlayerState(player_state) => {
                let remote_message = RemoteMessage::State(player_state.clone());
                
                // Broadcast to all clients
                MessageExchange::broadcast_to_all(client_map, remote_message).await;
            },
            _ => tracing::warn!("Received unknown local message type: {:?}", message),
        }
    }

    async fn broadcast_to_all(client_map: &ClientMap, message: RemoteMessage) {
        let clients = client_map.read().await.get_all_clients();
        
        for (key, player) in clients {
            if let Err(e) = player.send(message.clone()).await {
                match e {
                    SendError::Disconnected(msg) => {
                        tracing::info!("client already disconnected {}: {}", key, msg);
                        client_map.write().await.remove(&key).await;
                    },
                    SendError::Other(msg) => {
                        tracing::error!("error sending message to {}: {}", key, msg);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /*use super::*;
    use crate::domain::messages::RemoteMessage;
    use crate::domain::traits::RemotePlayer;
    use async_trait::async_trait;
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::sync::mpsc::{channel, Sender};
    use tokio::sync::watch;
    use tokio::sync::watch::Receiver;

    #[tokio::test]
    async fn test_add_player() {
        let (local_sender, _) = broadcast::channel::<LocalMessage>(10);
        let local_receiver = local_sender.subscribe();
        let message_exchange = MessageExchange::new(local_sender, local_receiver);
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
        let (local_sender, _) = broadcast::channel::<LocalMessage>(10);
        let local_receiver = local_sender.subscribe();
        let message_exchange = MessageExchange::new(local_sender, local_receiver);
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
    }*/
}
