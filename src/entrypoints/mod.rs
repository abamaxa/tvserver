pub mod api;
mod context;
mod tvserver;
#[cfg(feature = "webserver")]
mod webserver;
#[cfg(not(feature = "webserver"))]
mod app;
#[cfg(not(feature = "webserver"))]
mod tauri_api;

pub use api::{register, SharedState};
pub use context::{Context, create_context};
pub use tvserver::TVServer;

#[cfg(feature = "webserver")]
pub use webserver::run_webserver;

#[cfg(not(feature = "webserver"))]
pub use app::run_tauri;
