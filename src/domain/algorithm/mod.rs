mod naming;
mod video_utils;    
mod series;
mod html;

pub use naming::{
    generate_display_name, 
    get_collection_and_video_from_path, 
    get_next_version_name, 
    get_collection_from_path,
    title_case,
    replace_extension
};

pub use video_utils::{
    get_videos_for_series_or_id, skip_file
};

pub use series::{
    parse_file_path, parse_only_file_name
};

pub use html::generate_video_html;
