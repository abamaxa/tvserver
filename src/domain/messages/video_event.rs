use crate::domain::models::VideoDetails;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum VideoEventType {
    VideoEventAdded = 0,
    VideoEventChanged = 1,
    VideoEventDeleted = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEvent {
    #[serde(rename = "type")]
    pub event_type: VideoEventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoDetails>,
    pub checksum: String,
}

impl VideoEvent {
    pub fn new_video_added_event(video: VideoDetails) -> Self {
        VideoEvent {
            event_type: VideoEventType::VideoEventAdded,
            video: Some(video.clone()),
            checksum: video.checksum.to_string(),
        }
    }

    pub fn new_video_changed_event(video: VideoDetails) -> Self {
        VideoEvent {
            event_type: VideoEventType::VideoEventChanged,
            video: Some(video.clone()),
            checksum: video.checksum.to_string(),
        }
    }

    pub fn new_video_deleted_event(checksum: i64) -> Self {
        VideoEvent {
            event_type: VideoEventType::VideoEventDeleted,
            video: None,
            checksum: checksum.to_string(),
        }
    }
}
