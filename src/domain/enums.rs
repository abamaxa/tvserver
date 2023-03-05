use serde::{Serialize, Deserialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub enum SearchEngineType {
    YouTube = 0,
    #[default]
    Torrent = 1
}
