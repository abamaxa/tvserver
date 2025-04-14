use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing;

const CHANNEL_SIZE: usize = 100;

#[derive(Debug, thiserror::Error)]
pub enum LocalMessageExchangeError {
    #[error("Listener with this key already exists")]
    ListenerExists,
    #[error("Listener with this key does not exist")]
    ListenerDoesNotExist,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MessageFilter {
    All,
    Media,
    Task,
}


pub struct LocalMessageSenderReceiver {
    sender: broadcast::Sender<LocalMessage>,
    _receiver: broadcast::Receiver<LocalMessage>,
}

impl LocalMessageSenderReceiver {
    pub fn new() -> Self {
        let (sender, _receiver) = broadcast::channel::<LocalMessage>(CHANNEL_SIZE);
        Self { sender, _receiver }
    }

    pub fn subscribe(&self) -> LocalMessageReceiver {
        self.sender.subscribe()
    }
}

#[derive(Clone)]
pub struct LocalMessageExchange {
    multiplexer: LocalMessageSender,
    broadcasters: Arc<RwLock<HashMap<MessageFilter, LocalMessageSenderReceiver>>>,
}

impl LocalMessageExchange {
    pub fn new() -> Self {
        let initial_broadcasters = vec![
                (MessageFilter::All, LocalMessageSenderReceiver::new()),
                (MessageFilter::Media, LocalMessageSenderReceiver::new()),
                (MessageFilter::Task, LocalMessageSenderReceiver::new()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>();
        let broadcasters = Arc::new(RwLock::new(initial_broadcasters));   
        let (multiplexer, mut multiplexer_rx) = mpsc::channel::<LocalMessage>(CHANNEL_SIZE);
        let copy_broadcasters = broadcasters.clone();

        // Spawn a task to process incoming messages
        tokio::spawn(async move {
            while let Some(message) = multiplexer_rx.recv().await {
                Self::broadcast(broadcasters.clone(), message).await;
            }

            tracing::debug!("Multiplexer channel closed");
        });

        Self {
            multiplexer,
            broadcasters: copy_broadcasters,
        }
    }

    pub fn new_sender(&self) -> LocalMessageSender {
        // this seems to be for testing only
        /*let multiplexer = self.multiplexer.clone();
        
        let (sender, mut sender_rx) = mpsc::channel::<LocalMessage>(CHANNEL_SIZE);
        
        tokio::spawn(async move {
            while let Some(message) = sender_rx.recv().await {
                if let Err(e) = multiplexer.send(message).await {
                    tracing::error!("Failed to forward message to multiplexer: {}", e);
                    break;
                }
            }
            tracing::debug!("Sender channel closed");
        });
        
        sender*/
        self.multiplexer.clone()
    }

    pub async fn listen_for_messages(
        &self,
        _key: &str,
        filter: MessageFilter,
    ) -> Result<LocalMessageReceiver, LocalMessageExchangeError> {
        if let Some(receiver) = self.broadcasters.read().await.get(&filter) {
            return Ok(receiver.subscribe());
        }

        Err(LocalMessageExchangeError::ListenerExists)
    }

    async fn broadcast(
        broadcasters: Arc<RwLock<HashMap<MessageFilter, LocalMessageSenderReceiver>>>,
        message: LocalMessage,
    ) {
        let broadcasters = broadcasters.read().await;

        match message.clone() {
            LocalMessage::Media(_event) => {
                let broadcaster = broadcasters.get(&MessageFilter::Media).unwrap();
                if let Err(e) = broadcaster.sender.send(message.clone()) {
                    tracing::warn!("Failed to send Media message: {}", e);
                }
            }
            LocalMessage::Task(_task) => {
                let broadcaster = broadcasters.get(&MessageFilter::Task).unwrap();
                if let Err(e) = broadcaster.sender.send(message.clone()) {
                    tracing::warn!("Failed to send Task message: {}", e);
                }
            }
        }

        let broadcaster = broadcasters.get(&MessageFilter::All).unwrap();
        if let Err(e) = broadcaster.sender.send(message) {
            tracing::warn!("Failed to send message to All channel: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::messages::{LocalMessage, MediaEvent};
    use std::path::PathBuf;
    use tokio::time::timeout;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_new_sender() {
        let exchange = LocalMessageExchange::new();
        let sender = exchange.new_sender();
        
        assert!(sender.send(LocalMessage::Task(vec![])).await.is_ok());
    }

    #[tokio::test]
    async fn test_listen_for_messages() {
        let exchange = LocalMessageExchange::new();
        
        let result = exchange.listen_for_messages("test", MessageFilter::Media).await;
        assert!(result.is_ok());
        
        // Try to listen with the same key again, should fail
        let result = exchange.listen_for_messages("test", MessageFilter::Media).await;
        assert!(matches!(result, Err(LocalMessageExchangeError::ListenerExists)));
    }

    #[tokio::test]
    async fn test_sending_messages() {
        let exchange = LocalMessageExchange::new();
        
        // Create a sender
        let sender = exchange.new_sender();
        
        // Register listeners for different message types
        let mut media_receiver = exchange.listen_for_messages("media", MessageFilter::Media).await.unwrap();
        let mut task_receiver = exchange.listen_for_messages("tasks", MessageFilter::Task).await.unwrap();
        let mut all_receiver = exchange.listen_for_messages("all", MessageFilter::All).await.unwrap();
        
        // Send a media message
        let media_event = MediaEvent::new_media(&PathBuf::from("test.mp4"), None);
        let media_message = LocalMessage::Media(media_event.clone());
        sender.send(media_message.clone()).await.unwrap();
        
        // Media and All receivers should get the message
        assert!(timeout(Duration::from_millis(100), media_receiver.recv()).await.is_ok());
        assert!(timeout(Duration::from_millis(100), all_receiver.recv()).await.is_ok());
        
        // Task receiver should not get the message
        assert!(timeout(Duration::from_millis(100), task_receiver.recv()).await.is_err());
        
        // Send a task message
        let task_message = LocalMessage::Task(vec![]);
        sender.send(task_message.clone()).await.unwrap();
        
        // Task and All receivers should get the message
        assert!(timeout(Duration::from_millis(100), task_receiver.recv()).await.is_ok());
        assert!(timeout(Duration::from_millis(100), all_receiver.recv()).await.is_ok());
        
        // Media receiver should not get the message
        assert!(timeout(Duration::from_millis(100), media_receiver.recv()).await.is_err());
    }

    #[tokio::test]
    async fn test_sending_lots_of_messages() {
        let exchange = LocalMessageExchange::new();
        
        // Create sender
        let sender1 = exchange.new_sender();
        
        // Register multiple media listeners
        let mut media_receiver1 = exchange.listen_for_messages("media1", MessageFilter::Media).await.unwrap();
        let mut media_receiver2 = exchange.listen_for_messages("media2", MessageFilter::Media).await.unwrap();
        
        // Register a task listener
        let mut task_receiver = exchange.listen_for_messages("tasks", MessageFilter::Task).await.unwrap();
        
        // Send two media messages
        let media_event1 = MediaEvent::new_media(&PathBuf::from("test1.mp4"), None);
        let media_message1 = LocalMessage::Media(media_event1.clone());
        sender1.send(media_message1.clone()).await.unwrap();
        
        let media_event2 = MediaEvent::new_media(&PathBuf::from("test2.mp4"), None);
        let media_message2 = LocalMessage::Media(media_event2.clone());
        sender1.send(media_message2.clone()).await.unwrap();
        
        // Each media receiver should get both messages
        for _ in 0..2 {
            assert!(timeout(Duration::from_millis(100), media_receiver1.recv()).await.is_ok());
            assert!(timeout(Duration::from_millis(100), media_receiver2.recv()).await.is_ok());
        }
        
        // Task receiver should not get any messages
        assert!(timeout(Duration::from_millis(100), task_receiver.recv()).await.is_err());
    }
}
