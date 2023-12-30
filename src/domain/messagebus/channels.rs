use crate::domain::traits::{ChannelBroadcaster, ChannelReceiver, Receiver};
use async_trait::async_trait;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct MessageSender<T> {
    sender: broadcast::Sender<T>,
}

impl<T> MessageSender<T> {
    /*fn new(value: broadcast::Sender<T>) -> Self {
        Self { sender: value }
    }*/
}

pub struct MessageReceiver<T> {
    receiver: broadcast::Receiver<T>,
}

impl<T> MessageReceiver<T> {
    fn new(value: broadcast::Receiver<T>) -> Self {
        Self { receiver: value }
    }
}

impl<T: Clone + Send + Sync + Debug + 'static> ChannelBroadcaster<T> for MessageSender<T> {
    fn subscribe(&self) -> Receiver<T> {
        let receiver = self.sender.subscribe();
        Arc::new(MessageReceiver::new(receiver))
    }

    fn send(&self, message: T) -> anyhow::Result<()> {
        self.sender.send(message)?;
        Ok(())
    }
}

#[async_trait]
impl<T: Send + Clone> ChannelReceiver<T> for MessageReceiver<T> {
    async fn recv(&mut self) -> anyhow::Result<T> {
        match self.receiver.recv().await {
            Ok(receiver) => Ok(receiver),
            Err(e) => Err(anyhow::anyhow!(e)),
        }
    }
}
