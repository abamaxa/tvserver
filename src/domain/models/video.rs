use chrono::{NaiveDateTime, Local, Duration};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::{Path, PathBuf}};
use crate::domain::algorithm::{title_case, parse_file_path};


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
pub struct VideoMetadata {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub aspect_width: u32,
    pub aspect_height: u32,
    pub audio_tracks: u32,
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

use thiserror::Error;

use crate::domain::messages::MediaItem;
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
#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
    pub play_from: Option<NaiveDateTime>,
    pub last_viewed: Option<NaiveDateTime>,
    #[serde(skip)]
    pub dir_path: Option<PathBuf>,
}

fn serialize_i64_to_string<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
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
            dir_path: Some(path.to_path_buf()),
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
