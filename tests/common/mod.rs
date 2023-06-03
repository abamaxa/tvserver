#![allow(dead_code)]

mod json_fetcher;
mod media_store;
mod player;
mod repository;
mod search;
mod server;
mod spawner;
mod task_manager;
mod text_fetcher;
mod torrents;

pub use json_fetcher::get_json_fetcher;
pub use media_store::get_media_store;
pub use player::{get_player, get_remote_player};
pub use repository::get_repository;
pub use search::{get_pirate_search, get_search_service, get_youtube_search};
pub use server::create_server;
pub use spawner::{get_no_spawner, get_spawner};
pub use task_manager::get_task_manager;
pub use text_fetcher::get_text_fetcher;
pub use torrents::get_torrent_downloader;
