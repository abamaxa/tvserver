use chrono::{NaiveDateTime, Local, Duration};
use serde::{Deserialize, Serialize, Serializer};
use std::{collections::HashMap, path::{Path, PathBuf}};
use crate::domain::algorithm::{title_case, parse_file_path, get_video_url, get_thumbnails_url};
use serde::ser::SerializeStruct;
use serde_json;
use thiserror::Error;

use crate::domain::messages::MediaItem;

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CollectionItem {
    pub collection: String,
    pub thumbnail: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CollectionDetails {
    pub collection: String,
    pub parent_collection: String,
    pub child_collections: Vec<CollectionItem>,
    pub series: HashMap<String, Vec<VideoDetails>>,
    pub videos: Vec<MediaItem>,
    pub errors: Vec<String>,
}

impl CollectionDetails {
    pub fn new(
        collection: String,
        child_collections: Vec<CollectionItem>,
        items: Vec<VideoDetails>,
    ) -> Self {
        // Convert `VideoDetails` items into `MediaItem`s.
        let videos: Vec<MediaItem> = items.iter().cloned().map(MediaItem::Video).collect();
        // Group VideoDetails by series.
        let series = Self::group_by_series(&items);

        CollectionDetails {
            collection: collection.clone(),
            parent_collection: Self::parent_collection(&collection),
            child_collections,
            videos,
            series,
            errors: Vec::new(),
        }
    }

    pub fn error(error: String) -> CollectionDetails {
        CollectionDetails {
            errors: vec![error],
            ..Default::default()
        }
    }

    /// Helper function to extract the parent collection from a collection string.
    /// If the string contains a '/', it returns the part before the slash; otherwise, an empty string.
    fn parent_collection(collection: &str) -> String {
        if let Some(pos) = collection.find('/') {
            collection[..pos].to_string()
        } else {
            "".to_string()
        }
    }

    fn group_by_series(items: &Vec<VideoDetails>) -> HashMap<String, Vec<VideoDetails>> {
        // For demonstration purposes, we return an empty HashMap.
        let mut series = HashMap::new();
        for item in items {
            series.entry(item.series.season.clone()).or_insert(Vec::new()).push(item.clone());
        }
        series
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    pub id: i64,
    pub language: String,
    pub title: Option<String>,
    pub codec: Option<String>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub id: i64,
    pub language: String,
    pub title: Option<String>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub aspect_width: u32,
    pub aspect_height: u32,
    pub audio_tracks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_track_list: Option<Vec<AudioTrack>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_tracks: Option<Vec<SubtitleTrack>>,
    #[serde(skip)]
    pub probe_data: Option<String>,
}


#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SeriesDetails {
    pub series_title: String,
    pub season: String,
    pub episode: String,
    pub episode_title: String,
}

#[derive(Deserialize, Debug)]
struct FFProbeStream {
    index: i64,
    codec_type: String,
    codec_name: Option<String>,
    tags: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Debug)]
struct FFProbeData {
    streams: Vec<FFProbeStream>,
}

#[derive(Error, Debug)]
#[error("{message:}")]
pub struct VideoParseError {
    message: String,
}

#[derive(Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum VideoState {
    #[default]
    Ready = 0,
    NewFile = 1,
    ZeroFileSize = 2,
    NoVideoSize = 3,
    NeedThumbnail = 4,
    NeedVideoMetaData = 5,
    NeedDescription = 6,
    Exception = 10,
}

fn video_state_from_int<T: Into<i64>>(value: T) -> VideoState {
    match value.into() {
        0 => VideoState::Ready,
        1 => VideoState::NewFile,
        10 => VideoState::Exception,
        2 => VideoState::ZeroFileSize,
        3 => VideoState::NoVideoSize,
        4 => VideoState::NeedThumbnail,
        5 => VideoState::NeedVideoMetaData,
        6 => VideoState::NeedDescription,
        _ => VideoState::default(),
    }
}

impl From<i8> for VideoState {
    fn from(value: i8) -> Self {
        video_state_from_int(value)
    }
}

impl From<i16> for VideoState {
    fn from(value: i16) -> Self {
        video_state_from_int(value)
    }
}

impl From<i32> for VideoState {
    fn from(value: i32) -> Self {
        video_state_from_int(value)
    }
}

impl From<i64> for VideoState {
    fn from(value: i64) -> Self {
        video_state_from_int(value)
    }
}


#[serde_with::skip_serializing_none]
#[derive(Default, Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoDetails {
    pub video: String,
    pub collection: String,
    pub description: String,
    pub series: SeriesDetails,
    pub thumbnail: Vec<String>,
    pub metadata: VideoMetadata,
    #[serde(serialize_with = "serialize_i64_to_string", deserialize_with = "deserialize_string_to_i64")]
    pub checksum: i64,
    pub search_phrase: Option<String>,
    pub state: VideoState,
    pub created_on: NaiveDateTime,
    pub updated_on: NaiveDateTime,
    pub play_from: Option<f32>,
    pub last_viewed: Option<NaiveDateTime>,
    #[serde(skip)]
    pub dir_path: Option<PathBuf>,
}

fn deserialize_string_to_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let s = String::deserialize(deserializer)?;
    s.parse::<i64>().map_err(D::Error::custom)
}

impl VideoDetails {
    pub fn new(video: String, collection: String, path: &PathBuf, suggested_series: Option<String>) -> Self {
        let now = Local::now().naive_local();
        let series = SeriesDetails::parse_collection_video(&collection, &video, suggested_series);
        let dir_path = path.parent().map(|dir| dir.to_path_buf());

        Self {
            video,
            collection,
            description: "".to_string(),
            series,
            thumbnail: Vec::new(),
            metadata: VideoMetadata{..VideoMetadata::default()},
            checksum: 0,
            search_phrase: None,
            state: VideoState::NewFile,
            created_on: now,
            updated_on: now,
            play_from: None,
            last_viewed: None,
            dir_path,
        }
    }

    pub fn should_retry_metadata(&self) -> bool {
        if self.metadata.duration == 0. || self.metadata.height == 0 {
            return !Self::is_older_than_x_hours(self.updated_on, 6);
        }
        false
    }

    pub fn should_delete(&self) -> bool {
        if self.metadata.duration == 0. || self.metadata.height == 0 {
            return Self::is_older_than_x_hours(self.updated_on, 24);
        }
        false
    }

    fn is_older_than_x_hours(given_datetime: NaiveDateTime, num_hours: i64) -> bool {
        let current_datetime = Local::now().naive_utc();
        let duration_since_given = current_datetime.signed_duration_since(given_datetime);
    
        duration_since_given >= Duration::hours(num_hours)
    }
    
    /// Returns the full path to the video file.
    /// 
    /// If dir_path is set, joins dir_path and video.
    /// Otherwise, uses the movie directory from config and joins with collection and video.
    pub fn get_full_path(&self) -> PathBuf {
        if let Some(dir_path) = &self.dir_path {
            dir_path.join(&self.video)
        } else if self.collection.is_empty() {
            Path::new(&crate::domain::config::get_movie_dir()).join(&self.video)
        } else {
            Path::new(&crate::domain::config::get_movie_dir())
                .join(&self.collection)
                .join(&self.video)
        }
    }
    
    /// Returns the relative path to the video file for download/sharing purposes.
    /// 
    /// This is the path relative to the movie directory, used for URLs.
    pub fn get_download_path(&self) -> String {
        if self.collection.is_empty() {
            self.video.clone()
        } else {
            format!("{}/{}", self.collection, self.video)
        }
    }
}

impl Serialize for VideoDetails {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Calculate the number of fields dynamically based on Option values and the added url
        let mut field_count = 11; // Base fields + url
        if self.search_phrase.is_none() { field_count -= 1; }
        if self.play_from.is_none() { field_count -= 1; }
        if self.last_viewed.is_none() { field_count -= 1; }

        let mut state = serializer.serialize_struct("VideoDetails", field_count)?;
        let thumbnails = get_thumbnails_url(&self.thumbnail);

        state.serialize_field("video", &self.video)?;
        state.serialize_field("collection", &self.collection)?;
        state.serialize_field("description", &self.description)?;
        state.serialize_field("series", &self.series)?;
        state.serialize_field("thumbnail", &thumbnails)?;
        state.serialize_field("metadata", &self.metadata)?;
        // Apply custom i64 to string serialization for checksum explicitly
        state.serialize_field("checksum", &self.checksum.to_string())?;

        // Handle Option fields honoring skip_serializing_none implicitly by checking is_some()
        if let Some(ref sp) = self.search_phrase {
            state.serialize_field("searchPhrase", sp)?;
        }
        state.serialize_field("state", &self.state)?;
        state.serialize_field("createdOn", &self.created_on)?;
        state.serialize_field("updatedOn", &self.updated_on)?;
        if let Some(ref pf) = self.play_from {
            state.serialize_field("playFrom", pf)?;
        }
        if let Some(ref lv) = self.last_viewed {
             state.serialize_field("lastViewed", lv)?;
        }

        // Add the computed url field
        state.serialize_field("url", &get_video_url(&self.collection, &self.video))?;

        state.end()
    }
}

impl SeriesDetails {
    pub fn new(
        series_title: &str,
        season: &str,
        episode: &str,
        episode_title: Option<&str>,
    ) -> Self {
        Self {
            series_title: series_title.to_string(),
            season: season.to_string(),
            episode: episode.to_string(),
            episode_title: episode_title.unwrap_or_default().to_string(),
        }
    }

    pub fn parse_collection_video(collection: &str, video: &str, suggested_series: Option<String>) -> Self {

        // Clean and check if collection path is absolute
        let clean_collection = Path::new(collection).to_string_lossy().into_owned();
        if clean_collection.len() != 1 && Path::new(&clean_collection).is_absolute() {
            return Self::parse_file_name_with_series(video, suggested_series);
        }

        let path = if !collection.is_empty() {
            format!("{}/{}", collection, video)
        } else {
            video.to_string()
        };

        Self::parse_file_name_with_series(&path, suggested_series)
    }

    pub fn parse_file_name_with_series(file_name: &str, suggested_series: Option<String>) -> Self {
        let result = parse_file_path(file_name);
        
        let series_title = if let Some(series) = suggested_series {
            title_case(&series)
        } else {
            result.series_details.series_title
        };

        Self {
            series_title,
            season: result.series_details.season,
            episode: result.series_details.episode,
            episode_title: result.series_details.episode_title,
        }
    }

    pub fn full_title(&self) -> String {
        use std::fmt::Write;
        let mut title = String::new();
        
        write!(&mut title, "{}", self.series_title).unwrap();
        
        if !self.season.is_empty() {
            write!(&mut title, ", Season {}", self.season).unwrap();
        }
        
        if !self.episode.is_empty() {
            write!(&mut title, ", Episode {}", self.episode).unwrap();
        }
        
        if !self.episode_title.is_empty() {
            write!(&mut title, ", {}", self.episode_title).unwrap();
        }
        
        title
    }
}

impl VideoMetadata {
    pub fn from_probe_data(mut self, probe_data_str: &Option<String>) -> Self {
        if let Some(data) = probe_data_str {
            if let Ok(probe_output) = serde_json::from_str::<FFProbeData>(data) {
                let audio_tracks: Vec<AudioTrack> = probe_output
                    .streams
                    .iter()
                    .filter(|s| s.codec_type == "audio")
                    .map(|s| {
                        let tags = s.tags.as_ref();
                        AudioTrack {
                            id: s.index,
                            language: tags
                                .and_then(|t| t.get("language").cloned())
                                .unwrap_or_else(|| "und".to_string()),
                            title: tags.and_then(|t| t.get("title").cloned()),
                            codec: s.codec_name.clone(),
                        }
                    })
                    .collect();

                if !audio_tracks.is_empty() {
                    tracing::debug!("Found {} audio tracks", audio_tracks.len());
                    self.audio_track_list = Some(audio_tracks);
                } else {
                    tracing::debug!("No audio tracks found in probe data");
                }

                let subtitle_tracks: Vec<SubtitleTrack> = probe_output
                    .streams
                    .iter()
                    .filter(|s| s.codec_type == "subtitle")
                    .map(|s| {
                        let tags = s.tags.as_ref();
                        SubtitleTrack {
                            id: s.index,
                            language: tags
                                .and_then(|t| t.get("language").cloned())
                                .unwrap_or_else(|| "und".to_string()),
                            title: tags.and_then(|t| t.get("title").cloned()),
                        }
                    })
                    .collect();

                if !subtitle_tracks.is_empty() {
                    tracing::debug!("Found {} subtitle tracks", subtitle_tracks.len());
                    self.subtitle_tracks = Some(subtitle_tracks);
                }
            } else {
                tracing::warn!("Failed to parse probe data as JSON");
            }
        } else {
            tracing::debug!("No probe data available");
        }
        self
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use std::iter::zip;
    use std::path::PathBuf;
    #[test]
    fn test_parse_file_name() {
        let tests = [
            "S00E07 - The Frog's Legacy.mkv",
            "Line Of Duty S02E03",
            "Line Of Duty S04E05.mp4",
            &PathBuf::from("Line of Duty").join("Line Of Duty S06E07.mp4").to_string_lossy().to_string(),
            &PathBuf::from("Only Fools and Horses").join("Specials").join("S00E07 - The Frog's Legacy.mkv").to_string_lossy().to_string(),
            "The Sweeney 5-02 Messenger Of The Gods.mkv",
            &PathBuf::from("The Sweeney").join("Series 4").join("The Sweeney 4-01 Messenger Of The Gods.mkv").to_string_lossy().to_string(),
        ];

        let expected_results = [
            SeriesDetails::new("The Frog's Legacy", "", "07", None),
            SeriesDetails::new("Line Of Duty", "2", "03", None),
            SeriesDetails::new("Line Of Duty", "4", "05", None),
            SeriesDetails::new("Line of Duty", "6", "07", None),
            SeriesDetails::new(
                "Only Fools and Horses",
                "0",
                "07",
                Some("The Frog's Legacy"),
            ),
            SeriesDetails::new("The Sweeney", "5", "02", Some("Messenger Of The Gods")),
            SeriesDetails::new("The Sweeney", "4", "01", Some("Messenger Of The Gods")),
        ];

        assert_eq!(tests.len(), expected_results.len());

        for (test, expected) in zip(tests, expected_results) {
            let result = SeriesDetails::parse_file_name_with_series(test, None);
            assert_eq!(result, expected);
        }
    }
}
