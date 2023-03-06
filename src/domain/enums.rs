use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum SearchEngineType {
    YouTube = 0,
    #[default]
    Torrent = 1,
}
