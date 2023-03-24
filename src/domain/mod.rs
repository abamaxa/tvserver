use std::sync::Arc;

mod algorithm;
pub mod config;
mod enums;
pub mod messages;
pub mod models;
pub mod traits;
pub use enums::*;
#[cfg(test)]
mod test_util;

pub type Searcher = Arc<dyn traits::MediaSearcher<models::DownloadableItem>>;
pub type SearchDownloader = Arc<dyn traits::MediaDownloader>;

#[cfg(test)]
pub use test_util::NoSpawner;
