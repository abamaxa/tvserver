mod video_metadata;
mod book_metadata;
mod media_check;
mod book_check;
mod book_path_lease;
mod download_monitor;
mod history;
mod encoding;
mod media_sharing;
pub mod copy_server;
mod book_progress;

pub use video_metadata::{MetaDataError, MetaDataErrorCode, generate_video_metadatas, calculate_checksum, get_video_metadata};
pub use book_metadata::{
    extract_epub_metadata, extract_pdf_metadata, generate_book_metadata,
    BookMetadataExtraction, BookMetadataExtractionError,
};
pub(crate) use book_metadata::generate_book_metadata_with_cancellation;
pub use media_check::MediaCheck;
pub use book_check::BookCheck;
pub use book_path_lease::BookPathLeaseCoordinator;
pub use download_monitor::DownloadMonitor;
pub use history::HistoryService;
pub use encoding::{convert_to_mp4, extract_subtitles, re_encode, should_re_encode, re_encode_video, CodecArgs, AlreadyEncodedError};
pub use media_sharing::MediaSharing;
pub use copy_server::CopyServer;
pub use book_progress::{BookProgressError, BookProgressService};
