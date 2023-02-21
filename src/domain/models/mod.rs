pub mod downloadable;
pub mod search_results;
pub mod video_entry;

pub use downloadable::{DownloadProgress, FileDetails, TorrentListResults};
pub use search_results::{DownloadableItem, SearchResults};
pub use video_entry::{VideoEntry};
