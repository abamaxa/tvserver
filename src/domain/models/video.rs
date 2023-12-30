use chrono::{NaiveDateTime, Local, Duration};
use mockall::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};


#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CollectionDetails {
    pub collection: String,
    pub parent_collection: String,
    pub child_collections: Vec<String>,
    pub videos: Vec<MediaItem>,
    pub errors: Vec<String>,
}

impl CollectionDetails {
    pub fn from(
        collection: &str,
        child_collections: Vec<String>,
        videos: Vec<MediaItem>,
    ) -> CollectionDetails {
        let mut parent_collection = String::new();

        if collection.find('/').is_some() {
            let v: Vec<&str> = collection.rsplitn(2, '/').collect();
            parent_collection = v[1].to_string();
        }

        CollectionDetails {
            collection: collection.to_string(),
            parent_collection,
            child_collections,
            videos,
            ..Default::default()
        }
    }

    pub fn error(error: String) -> CollectionDetails {
        CollectionDetails {
            errors: vec![error],
            ..Default::default()
        }
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub audio_tracks: u32,
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
    pub thumbnail: PathBuf,
    pub metadata: VideoMetadata,
    pub checksum: i64,
    pub search_phrase: Option<String>,
    pub state: VideoState,
    pub created_on: NaiveDateTime,
    pub updated_on: NaiveDateTime,
}


impl VideoDetails {
    pub fn new(video: String, collection: String, path: &PathBuf) -> Self {
        let now = Local::now().naive_local();
        Self {
            video,
            collection,
            description: "".to_string(),
            series: SeriesDetails::from(path.as_path()),
            thumbnail: PathBuf::new(),
            metadata: VideoMetadata{..VideoMetadata::default()},
            checksum: 0,
            search_phrase: None,
            state: VideoState::NewFile,
            created_on: now,
            updated_on: now
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

    pub fn full_title(&self) -> String {
        let mut title = self.series_title.clone();

        if !self.season.is_empty() {
            title = format!("{}, Season {}", title, self.season);
        }

        if !self.episode.is_empty() {
            title = format!("{}, Episode {}", title, self.episode);
        }

        if !self.episode_title.is_empty() {
            title = format!("{}, {}", title, self.episode_title);
        }

        title
    }

    fn parse_file_name(file_name: &str) -> Option<Self> {
        lazy_static! {
            static ref PARSER_EX: [Regex; 7] = [
                Regex::new(r"(?P<series_title>[^\\/\n]+)/Series (?P<season>\d+)/S[\d]+E(?P<episode>\d+) - (?P<episode_title>[^\\/\n]+)").unwrap(),
                Regex::new(r"(?P<series_title>[^\\/\n]+)/Series (?P<season>\d+)/.*\d+-(?P<episode>\d+) (?P<episode_title>[^\\/\n]+)").unwrap(),
                Regex::new(r"(?P<series_title>[^\\/\n]+)/(?P<season>[^\\/\n]+)/S[\d]+E(?P<episode>\d+) - (?P<episode_title>[^\\/\n]+)").unwrap(),
                Regex::new(r"^(?P<series_title>[^\\/\n]+) (?P<season>\d+)-(?P<episode>\d+) (?P<episode_title>[^\\/\n]+)$").unwrap(),
                Regex::new(r"(?P<series_title>[^\\/\n]+)/.*S(?P<season>\d+)E(?P<episode>\d+)").unwrap(),
                Regex::new(r"^(?P<series_title>[^\\/\n]+) S(?P<season>\d+)E(?P<episode>\d+)").unwrap(),
                Regex::new(r"^S(?P<season>\d+)E(?P<episode>\d+) - (?P<series_title>[^\\/\n]+)").unwrap(),
            ];
        }

        let file_name = &file_name[..file_name.find('.').unwrap_or(file_name.len())];

        PARSER_EX
            .iter()
            .find_map(|regex| Self::parse_tv_series_info(file_name, regex))
    }

    fn parse_tv_series_info(file_name: &str, regex: &Regex) -> Option<Self> {
        let captures = regex.captures(&file_name)?;

        let series_title = captures.name("series_title")?.as_str().to_string();
        let season_str = captures.name("season")?.as_str();
        let episode_str = captures.name("episode")?.as_str();

        let season = match season_str.parse::<u32>() {
            Ok(s) => s.to_string(),
            _ => season_str.to_string(),
        };

        let episode = match episode_str.parse::<u32>() {
            Ok(s) => s.to_string(),
            _ => episode_str.to_string(),
        };

        // allow failure to map episode name
        let episode_title = captures
            .name("episode_title")
            .map_or(String::new(), |m| m.as_str().to_string());

        Some(Self {
            series_title,
            season,
            episode,
            episode_title,
        })
    }
}

impl TryFrom<&str> for SeriesDetails {
    type Error = VideoParseError;

    fn try_from(value: &str) -> Result<Self, VideoParseError> {
        match Self::parse_file_name(value) {
            Some(details) => Ok(details),
            _ => Err(VideoParseError {
                message: format!("could not parse name: {}", value),
            }),
        }
    }
}

impl From<&Path> for SeriesDetails {
    fn from(value: &Path) -> Self {
        let binding = value.with_extension("");
        let file_name = binding
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default();

        // Create an iterator over the path components
        let mut components = binding.iter();

        // Loop until there are no more components left
        while let Some(_) = components.next() {
            // Create a new PathBuf from the remaining components
            let new_path = components.clone().collect::<PathBuf>();

            if new_path.parent().is_none() {
                break;
            }

            if let Some(details) = Self::parse_file_name(file_name) {
                return details;
            }
        }

        Self {
            series_title: file_name.to_string(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use std::iter::zip;

    #[test]
    fn test_parse_file_name() {
        let tests = [
            "Line Of Duty S02E03",
            "Line Of Duty S02E03.mp4",
            "Line of Duty/Line Of Duty S02E03.mp4",
            "S00E07 - The Frog's Legacy.mkv",
            "Only Fools and Horses/Specials/S00E07 - The Frog's Legacy.mkv",
            "The Sweeney 4-01 Messenger Of The Gods.mkv",
            "The Sweeney/Series 4/The Sweeney 4-01 Messenger Of The Gods.mkv",
        ];

        let expected_results = [
            SeriesDetails::new("Line Of Duty", "2", "3", None),
            SeriesDetails::new("Line Of Duty", "2", "3", None),
            SeriesDetails::new("Line of Duty", "2", "3", None),
            SeriesDetails::new("The Frog's Legacy", "0", "7", None),
            SeriesDetails::new(
                "Only Fools and Horses",
                "Specials",
                "7",
                Some("The Frog's Legacy"),
            ),
            SeriesDetails::new("The Sweeney", "4", "1", Some("Messenger Of The Gods")),
            SeriesDetails::new("The Sweeney", "4", "1", Some("Messenger Of The Gods")),
        ];

        assert_eq!(tests.len(), expected_results.len());

        for (test, expected) in zip(tests, expected_results) {
            let result = SeriesDetails::try_from(test);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), expected);
        }
    }
}
