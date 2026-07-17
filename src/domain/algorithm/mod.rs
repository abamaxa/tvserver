mod naming;
mod media_kind;
mod video_utils;
mod series;
mod html;
pub mod file_integrity;

pub use naming::{
    generate_display_name, 
    get_collection_and_video_from_path, 
    get_collection_and_book_from_path,
    get_collection_and_file_from_rooted_path,
    get_next_version_name, 
    get_collection_from_path,
    get_book_collection_from_path,
    get_collection_from_rooted_path,
    collection_id_to_path,
    path_to_collection_id,
    title_case,
    replace_extension,
    get_video_url,
    get_thumbnails_url,
    get_book_url,
    get_book_thumbnail_url,
    get_book_download_path,
    get_book_thumbnail_file_name
};

pub use media_kind::{
    classify_media_kind,
    MediaKind,
};

pub use video_utils::{
    get_videos_for_series_or_id, is_video_scan_candidate, skip_file
};

pub use series::{
    parse_file_path, parse_only_file_name
};

pub use html::generate_video_html;
