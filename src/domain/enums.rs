use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Eq, Hash, Serialize, Deserialize, PartialEq)]
pub enum SearchEngineType {
    YouTube = 0,
    #[default]
    Torrent = 1,
}
