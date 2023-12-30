use std::sync::Arc;

pub mod algorithm;
pub mod config;
mod enums;
pub mod messages;
pub mod messagebus;
pub mod models;
pub mod services;
pub mod traits;
pub use enums::*;

#[cfg(test)]
mod test_util;

pub type Searcher = Arc<dyn traits::MediaSearcher<models::DownloadableItem>>;
pub type SearchDownloader = Arc<dyn traits::MediaDownloader>;

#[cfg(test)]
pub use test_util::NoSpawner;
