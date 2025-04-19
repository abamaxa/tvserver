use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{self, error};

use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender, RemoteMessage, RemotePlayerState};
use crate::domain::traits::Databaser;

pub struct HistoryService {
    repo: Arc<dyn Databaser>,
    sender: LocalMessageSender,
}

impl HistoryService {
    pub fn new(repo: Arc<dyn Databaser>, sender: LocalMessageSender) -> Self {
        Self {repo, sender}
    }

    pub fn observe(self, mut receiver: LocalMessageReceiver) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut counter = 0;
            loop {
                tokio::select! {
                    result = receiver.recv() => {
                        match result {
                            Ok(message) => {
                                match message {
                                    LocalMessage::PlayerState(player_state) => {
                                        // Process player state message if available
                                        counter += 1;
                                        
                                        if counter % 10 == 0 {
                                            self.update_history(&player_state).await;
                                        }
                                    }
                                    LocalMessage::LastStateRequest(remote_address) => {
                                        // This would need to be adapted based on the actual task structure
                                        // to extract remote addresses for last state requests
                                        self.send_last_state(&remote_address).await;
                                    }
                                    _ => {}
                                }
                            }
                            Err(e) => {
                                error!("error receiving message: {}", e);
                                return;
                            }
                        }
                    }
                    else => break,
                }
            }
        })
    }

    async fn update_history(&self, message: &RemotePlayerState) {
        let current_time = message.current_time;

        let checksum = match string_to_int64(&message.video_id) {
            Ok(c) => c,
            Err(e) => {
                error!("could not convert videoId: {} error: {}", message.video_id, e);
                return;
            }
        };

        /*
        This is very simple that's good enough for a couple of clients but won't scale,
        need to batch updates and write to the database periodically in bulk.
        */
        if let Err(e) = self.repo.update_watched_video(checksum, current_time).await {
            error!("failed to update watched video: {}", e);
        }
    }

    async fn send_last_state(&self, to: &SocketAddr) {
        let history = match self.repo.get_history(0, 1).await {
            Ok(h) => h,
            Err(e) => {
                error!("failed to get last history: {}", e);
                return;
            }
        };

        if !history.is_empty() {
            let last_state = RemoteMessage::LastState(history[0].clone());
            
            // Send message to the specified address
            let message = LocalMessage::SendToRemote(to.clone(), last_state);
            if let Err(e) = self.sender.send(message).await {
                error!("failed to send last state message: {}", e);
            }
        }
    }
}

fn string_to_int64(s: &str) -> Result<i64, String> {
    if s.is_empty() {
        return Err("empty string".to_string());
    }

    s.parse::<i64>().map_err(|e| e.to_string())
}
