use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::sync::mpsc;

use super::book_event::BookEvent;
use super::messages::{TaskState, RemotePlayerState, RemoteMessage};
use super::media::MediaEvent;
use super::video_event::VideoEvent;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalMessage {
    Media(MediaEvent),
    Task(Vec<TaskState>),
    Book(BookEvent),
    Video(VideoEvent),
    PlayerState(RemotePlayerState),
    SendToRemote(SocketAddr, RemoteMessage),
    LastStateRequest(SocketAddr),
}

pub type LocalMessageReceiver = broadcast::Receiver<LocalMessage>;
pub type LocalMessageSender = mpsc::Sender<LocalMessage>;
