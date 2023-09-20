use crate::domain::models::{CollectionDetails, VideoDetails};
use crate::domain::traits::Storer;
use crate::domain::{SearchEngineType, TaskType};
use chrono::{NaiveDate, Utc};
use mockall::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::sync::broadcast;

lazy_static! {
    static ref DEFAULT_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 80);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlayerState {
    pub current_time: f64,
    pub duration: f64,
    pub current_src: String,
    pub collection: String,
    pub video: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RemoteMessage {
    Command {
        command: String,
    },
    Play {
        url: String,
        collection: String,
        video: String,
    },
    Seek {
        interval: i32,
    },
    Stop,
    TogglePause(String),

    State(RemotePlayerState),
    SendLastState,
    Error(String),

    Ping(u64),
    Pong(SocketAddr),

    Close(SocketAddr),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAdded {
    pub full_path: PathBuf,
    pub search: Option<String>,
    pub date: Option<NaiveDate>,
}

impl MediaAdded {
    pub fn new(path: &Path, search: Option<String>) -> Self {
        Self {
            full_path: PathBuf::from(path),
            search: search,
            date: Some(NaiveDate::from(Utc::now().naive_utc())),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMoved {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
}

/*
This event is generated when a file is downloaded, renamed, deleted and is used to trigger
copying the file into the MediaStore, metadata generation and notifications to remote clients.
 */
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaEvent {
    MediaAvailable(MediaAdded),
    MediaMoved(MediaMoved),
    MediaDeleted(PathBuf),
}

impl MediaEvent {
    pub fn new_media(path: &Path, search: Option<String>) -> Self {
        Self::MediaAvailable(MediaAdded::new(path, search))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MediaItem {
    Collection(CollectionDetails),
    Video(VideoDetails),
    Error(String),
}

impl MediaItem {
    pub fn error(message: &str) -> Self {
        Self::Error(message.to_string())
    }
}

impl From<std::io::Error> for MediaItem {
    fn from(value: std::io::Error) -> Self {
        Self::Error(value.to_string())
    }
}

impl From<anyhow::Error> for MediaItem {
    fn from(value: anyhow::Error) -> Self {
        Self::Error(value.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceivedRemoteMessage {
    pub from_address: SocketAddr,
    pub message: RemoteMessage,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LocalCommand {
    pub command: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
    pub remote_address: Option<String>,
    pub message: RemoteMessage,
}

impl Command {
    pub fn address(&self) -> SocketAddr {
        as_sockaddr(&self.remote_address)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayRequest {
    pub collection: String,
    pub video: String,
    pub remote_address: Option<String>,
}

impl PlayRequest {
    pub fn make_remote_command(&self) -> RemoteMessage {
        let url: String = if self.collection.is_empty() {
            format!("/api/stream/{}  ", self.video)
        } else {
            format!("/api/stream/{}/{}", self.collection, self.video)
        };

        RemoteMessage::Play {
            url,
            collection: self.collection.clone(),
            video: self.video.clone(),
        }
    }

    pub fn make_local_command(&self, store: &Storer) -> String {
        format!(
            "add file://{}",
            store.as_local_path(&self.collection, &self.video)
        )
    }

    pub fn address(&self) -> SocketAddr {
        as_sockaddr(&self.remote_address)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Response {
    pub message: String,
    pub errors: Vec<String>,
}

impl Response {
    pub fn success(message: String) -> Response {
        Response {
            message,
            ..Default::default()
        }
    }
    pub fn error(error: String) -> Response {
        Response {
            errors: vec![error],
            ..Default::default()
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClientLogMessage {
    pub level: String,
    pub messages: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub name: String,
    pub link: String,
    pub engine: SearchEngineType,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayerListItem {
    pub name: String,
    pub last_message: Option<RemoteMessage>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayerList {
    pub players: Vec<PlayerListItem>,
}

impl PlayerList {
    pub fn new(players: Vec<PlayerListItem>) -> Self {
        Self { players }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionRequest {
    pub name: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRequest {
    pub new_name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskState {
    pub key: String,
    pub name: String,
    pub display_name: String,
    pub finished: bool,
    pub eta: i64,
    pub percent_done: f32,
    pub size_details: String,
    pub rate_details: String,
    pub process_details: String,
    pub error_string: String,
    pub task_type: TaskType,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalMessage {
    Media(MediaEvent),
    Task(Vec<TaskState>),
}

fn as_sockaddr(remote_address: &Option<String>) -> SocketAddr {
    match remote_address {
        Some(addr) => match SocketAddr::from_str(&addr) {
            Ok(addr) => addr,
            _ => *DEFAULT_ADDRESS,
        },
        _ => *DEFAULT_ADDRESS,
    }
}

#[serde_with::skip_serializing_none]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTRequest {
    pub model: String,
    pub messages: Vec<ChatGPTMessage>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub n: Option<i32>,
    pub stream: Option<bool>,
    pub stop: Option<Vec<String>>,
    pub max_tokens: Option<i32>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
    pub logit_bias: Option<HashMap<String, f64>>,
    pub user: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub usage: ChatGPTUsage,
    pub choices: Vec<ChatGPTChoice>,
}

impl<'a> ChatGPTResponse {
    pub fn get_all_content(&self) -> String {
        self.choices
            .iter()
            .map(|c| c.message.content.to_string())
            .collect::<Vec<String>>()
            .join(" ")
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTUsage {
    #[serde(rename = "prompt_tokens")]
    pub prompt_tokens: i64,
    #[serde(rename = "completion_tokens")]
    pub completion_tokens: i64,
    #[serde(rename = "total_tokens")]
    pub total_tokens: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTChoice {
    pub message: ChatGPTMessage,
    #[serde(rename = "finish_reason")]
    pub finish_reason: String,
    pub index: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGPTMessage {
    pub role: String,
    pub content: String,
}

impl ChatGPTMessage {
    pub fn system(message: &str) -> Self {
        Self {
            role: "system".to_string(),
            content: message.to_string(),
        }
    }

    pub fn user(message: &str) -> Self {
        Self {
            role: "user".to_string(),
            content: message.to_string(),
        }
    }

    pub fn assistant(message: &str) -> Self {
        Self {
            role: "assistant".to_string(),
            content: message.to_string(),
        }
    }
}

pub type LocalMessageReceiver = broadcast::Receiver<LocalMessage>;
pub type LocalMessageSender = broadcast::Sender<LocalMessage>;
