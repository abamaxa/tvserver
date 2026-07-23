use crate::domain::algorithm::get_video_url;
use crate::domain::models::{VideoDetails, VideoMetadata};
use crate::domain::TaskType;
use mockall::lazy_static;
use serde::{Deserialize, Deserializer, Serialize};
use std::net::{IpAddr, SocketAddr};

use super::{BookEvent, VideoEvent};

lazy_static! {
    static ref DEFAULT_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 80);
}

fn default_playback_rate() -> f64 {
    1.0
}

fn deserialize_f64_or_zero<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<f64>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_playback_rate<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<f64>::deserialize(deserializer)?.unwrap_or_else(default_playback_rate))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePlayerState {
    #[serde(default, deserialize_with = "deserialize_f64_or_zero")]
    pub current_time: f64,
    #[serde(default, deserialize_with = "deserialize_f64_or_zero")]
    pub duration: f64,
    pub collection: String,
    pub video: String,
    #[serde(rename = "videoId")]
    pub video_id: String,
    pub paused: bool,
    #[serde(
        default = "default_playback_rate",
        rename = "playbackRate",
        deserialize_with = "deserialize_playback_rate"
    )]
    pub playback_rate: f64,
    #[serde(rename = "currentSubtitleTrack")]
    pub current_subtitle_track: Option<i32>,
}

impl Default for RemotePlayerState {
    fn default() -> Self {
        Self {
            current_time: 0.0,
            duration: 0.0,
            collection: String::new(),
            video: String::new(),
            video_id: String::new(),
            paused: false,
            playback_rate: default_playback_rate(),
            current_subtitle_track: None,
        }
    }
}

impl RemotePlayerState {
    pub fn merge_playback_rate(prev: Option<RemotePlayerState>, rate: f64) -> RemotePlayerState {
        match prev {
            Some(mut s) => {
                s.playback_rate = rate;
                s
            }
            None => RemotePlayerState {
                playback_rate: rate,
                ..Default::default()
            },
        }
    }

    pub fn merge_subtitle_track(
        prev: Option<RemotePlayerState>,
        track_id: Option<i32>,
    ) -> RemotePlayerState {
        match prev {
            Some(mut s) => {
                s.current_subtitle_track = track_id;
                s
            }
            None => RemotePlayerState {
                current_subtitle_track: track_id,
                ..Default::default()
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RemoteMessage {
    Command {
        command: String,
    },
    Play {
        #[serde(rename = "videoId")]
        video_id: String,
        url: String,
        collection: String,
        video: String,
        width: i32,
        height: i32,
        #[serde(rename = "aspectWidth")]
        aspect_width: i32,
        #[serde(rename = "aspectHeight")]
        aspect_height: i32,
        metadata: Option<VideoMetadata>,
    },
    Seek {
        interval: i32,
    },
    SetAudioTrack {
        #[serde(rename = "trackId")]
        track_id: i32,
    },
    SetPlaybackRate {
        rate: f64,
    },
    SetSubtitleTrack {
        #[serde(rename = "trackId")]
        track_id: Option<i32>,
    },
    Stop,
    TogglePause(String),

    State(RemotePlayerState),
    SendLastState,
    Error(String),

    Ping(u64),
    Pong(SocketAddr),

    Close(String),

    LastState(VideoDetails),
    CurrentTasks(Vec<TaskState>),

    Book(Vec<BookEvent>),
    Video(Vec<VideoEvent>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceivedRemoteMessage {
    pub from_address: String,
    pub message: RemoteMessage,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    pub remote_address: Option<String>,
    pub message: RemoteMessage,
}

impl Command {
    pub fn address(&self) -> String {
        self.remote_address.clone().unwrap_or_default()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayRequest {
    pub collection: String,
    pub video: String,
    pub remote_address: Option<String>,
    pub width: i32,
    pub height: i32,
    pub aspect_width: i32,
    pub aspect_height: i32,
    #[serde(rename = "videoId")]
    pub video_id: String,
    pub metadata: Option<VideoMetadata>,
}

impl PlayRequest {
    pub fn make_remote_command(&self) -> RemoteMessage {
        let url: String = get_video_url(&self.collection, &self.video);

        RemoteMessage::Play {
            url,
            collection: self.collection.clone(),
            video: self.video.clone(),
            width: self.width,
            height: self.height,
            aspect_width: self.aspect_width,
            aspect_height: self.aspect_height,
            video_id: self.video_id.clone(),
            metadata: self.metadata.clone(),
        }
    }

    pub fn address(&self) -> String {
        self.remote_address.clone().unwrap_or_default()
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CopyFromServerRequest {
    pub host_url: String,
    pub videos: Vec<VideoDetails>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn deserializes_state_when_duration_is_null() -> Result<()> {
        let message: RemoteMessage = serde_json::from_str(
            r#"{"State":{"collection":"c","video":"v","videoId":"id","currentTime":12.5,"duration":null,"paused":false,"playbackRate":1.25,"currentSubtitleTrack":null}}"#,
        )?;

        match message {
            RemoteMessage::State(state) => {
                assert_eq!(state.collection, "c");
                assert_eq!(state.video, "v");
                assert_eq!(state.video_id, "id");
                assert_eq!(state.current_time, 12.5);
                assert_eq!(state.duration, 0.0);
                assert_eq!(state.playback_rate, 1.25);
                assert_eq!(state.current_subtitle_track, None);
            }
            other => panic!("expected State message, got {:?}", other),
        }

        Ok(())
    }

    #[test]
    fn default_state_uses_normal_playback_rate() {
        assert_eq!(RemotePlayerState::default().playback_rate, 1.0);
    }
}
