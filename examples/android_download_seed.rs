//! Disposable local peer for tests/android_background_download_smoke.cjs --transfers.
//! Build with: cargo build --no-default-features --features webserver --example android_download_seed
use anyhow::Context;
use base64::Engine;
use librqbit::{
    create_torrent, limits::LimitsConfig, AddTorrent, AddTorrentOptions, CreateTorrentOptions,
    Session, SessionOptions,
};
use std::num::NonZeroU32;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let source = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("expected disposable seed file")?,
    );
    let torrent = create_torrent(
        &source,
        CreateTorrentOptions {
            piece_length: Some(65536),
            ..Default::default()
        },
    )
    .await?;
    let bytes = torrent.as_bytes()?;
    let session = Session::new_with_opts(
        source.parent().context("seed file needs a parent")?.into(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            enable_upnp_port_forwarding: false,
            listen_port_range: Some(33000..35000),
            ratelimits: LimitsConfig {
                upload_bps: NonZeroU32::new(256 * 1024),
                download_bps: None,
            },
            ..Default::default()
        },
    )
    .await?;
    let handle = session
        .add_torrent(
            AddTorrent::from_bytes(bytes.clone()),
            Some(AddTorrentOptions {
                overwrite: true,
                disable_trackers: true,
                ..Default::default()
            }),
        )
        .await?
        .into_handle()
        .context("seed did not register")?;
    handle.wait_until_initialized().await?;
    println!(
        "{}",
        serde_json::json!({"port": session.tcp_listen_port(),
        "torrent": base64::engine::general_purpose::STANDARD.encode(&bytes), "seeded": handle.stats().finished})
    );
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        println!(
            "{}",
            serde_json::json!({"uploaded": handle.stats().uploaded_bytes})
        );
    }
}
