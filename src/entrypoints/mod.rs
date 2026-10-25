pub mod api;
mod book_runtime;
#[cfg(feature = "webserver")]
mod capability_file_service;
mod context;
mod tvserver;
#[cfg(feature = "webserver")]
mod tv;
#[cfg(feature = "webserver")]
pub mod webserver;
#[cfg(not(feature = "webserver"))]
mod app;
#[cfg(any(all(target_os = "android", not(feature = "webserver")), test))]
mod app_storage;
#[cfg(not(feature = "webserver"))]
mod tauri_api;
#[cfg(not(feature = "webserver"))]
mod android;
#[cfg(not(feature = "webserver"))]
mod playlists;

pub use api::{register, SharedState};
pub use book_runtime::{
    AvailableBookRuntime, BookIngestionRuntime, BookRuntime, BookStaticRoots,
    BOOK_LIBRARY_UNAVAILABLE,
};
pub use context::{Context, create_context};
pub use tvserver::TVServer;

#[cfg(feature = "webserver")]
pub use webserver::run_webserver;

#[cfg(not(feature = "webserver"))]
pub use app::run_tauri;
