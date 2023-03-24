use crate::domain::traits::Storer;
use crate::domain::SearchEngineType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteMessage {
    Command { command: String },
    Play { url: String },
    Seek { interval: i32 },
    Stop,
    TogglePause(String),
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayRequest {
    pub collection: String,
    pub video: String,
    pub remote_address: Option<String>,
}

impl PlayRequest {
    pub fn make_remote_command(&self) -> Command {
        let url: String = if self.collection.is_empty() {
            format!("/stream/{}  ", self.video)
        } else {
            format!("/stream/{}/{}", self.collection, self.video)
        };

        Command {
            remote_address: self.remote_address.clone(),
            message: RemoteMessage::Play { url },
        }
    }

    pub fn make_local_command(&self, store: &Storer) -> String {
        format!("add file://{}", store.as_path(&self.collection, &self.video))
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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayerList {
    pub players: Vec<String>,
}

impl PlayerList {
    pub fn new(players: Vec<String>) -> Self {
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
}
