use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteMessage {
    Command {command: String},
    Play {url: String},
    Seek {interval: i32},
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
    pub message: RemoteMessage
}


#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayRequest {
    pub collection: String,
    pub video: String,
    pub remote_address: Option<String>
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Response {
    pub message: String,
    pub errors: Vec<String>,
}

impl Response {
    pub fn success(message: String) -> Response {
        Response{message: message, ..Default::default()}
    }

    pub fn error(error: String) -> Response {
        Response{errors: vec![error], ..Default::default()}
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClientLogMessage {
    pub level: String,
    pub messages: Vec<String>,
}