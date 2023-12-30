mod conversion;
mod search_results;
mod torrent;
mod video;
mod youtube;

pub use conversion::{Conversion, AVAILABLE_CONVERSIONS};
pub use search_results::{DownloadableItem, SearchResults};
pub use torrent::{FileDetails, TaskListResults, TorrentTask};
pub use video::{CollectionDetails, SeriesDetails, VideoDetails, VideoState, VideoMetadata};
pub use youtube::{Id, Item, Snippet, YoutubeResponse};

#[cfg(test)]
pub mod test {
    pub use super::torrent::test::torrents_from_fixture;
}
