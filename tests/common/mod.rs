#![allow(dead_code)]

mod context;
mod json_fetcher;
mod media_checker;
mod media_store;
mod player;
mod repository;
mod search;
#[cfg(feature = "webserver")]
mod server;
mod spawner;
mod task_manager;
mod text_fetcher;
mod torrents;

#[allow(unused_imports)]
pub use context::{
    get_book_services, get_book_services_at, get_context, get_context_with_book_services,
};
pub use json_fetcher::get_json_fetcher;
pub use media_checker::get_checker;
#[allow(unused_imports)]
pub use media_store::get_media_store;
#[allow(unused_imports)]
pub use player::get_remote_player;
pub use repository::get_repository;
#[allow(unused_imports)]
pub use search::{get_pirate_search, get_search_service, get_youtube_search};
#[allow(unused_imports)]
#[cfg(feature = "webserver")]
pub use server::{create_server, create_server_with_book_roots};
#[allow(unused_imports)]
pub use spawner::{get_no_spawner, get_spawner};
pub use task_manager::get_task_manager;
pub use text_fetcher::get_text_fetcher;
pub use torrents::get_torrent_downloader;
