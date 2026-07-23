mod book;
mod conversion;
mod search_results;
mod video;
mod youtube;

pub use book::{
    default_book_thumbnail_bytes, ensure_default_book_thumbnail, is_default_book_thumbnail,
    BookCollectionDetails, BookCollectionItem, BookDetails, BookFormat, BookLocator,
    BookLocatorType, BookMetadata, BookReadingProgress, BookState, SaveBookProgressRequest,
    DEFAULT_BOOK_THUMBNAIL,
};
pub use conversion::{Conversion, AVAILABLE_CONVERSIONS};
pub use search_results::{DownloadableItem, SearchResults, TaskListResults};
pub use video::{CollectionItem, CollectionDetails, SeriesDetails, VideoDetails, VideoState, VideoMetadata};
pub use youtube::{Id, Item, Snippet, YoutubeResponse};

/*#[cfg(test)]
pub mod test {
    pub use super::torrent::test::torrents_from_fixture;
}*/
