pub mod repository;
pub mod vlc_player;
pub mod filestore;
pub mod browser_player;
pub mod youtube;
pub mod pirate_bay;
pub mod torrents;
pub mod subprocess;

pub use browser_player::{RemotePlayer, RemoteBrowserPlayer};
pub use filestore::FileStore;
pub use pirate_bay::{PirateClient};
pub use torrents::TransmissionDaemon;
#[cfg(feature = "vlc")]
pub use vlc_player::{Player, VLCPlayer};
