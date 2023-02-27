use serde::{Serialize, Deserialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub enum SearchEngineType {
    YOUTUBE = 0,
    #[default]
    TORRENT = 1
}
