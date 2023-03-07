extern crate core;

pub mod adaptors;
pub mod domain;
pub mod services;

use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::domain::traits::DownloadClient;
use crate::domain::{config::get_movie_dir, traits::Player, CLIENT_DIR, ENABLE_VLC};
use crate::services::torrents::TransmissionDaemon;
use crate::services::{api, media_store::MediaStore, monitor::Monitor, vlc_player::VLCPlayer};

pub async fn run() -> anyhow::Result<()> {
    let pool = adaptors::repository::get_database().await?;

    adaptors::repository::do_migrations(&pool).await?;

    let enable_vlc = env::var(ENABLE_VLC).unwrap_or_default();
    let player: Option<Arc<dyn Player>> = match enable_vlc.as_str() {
        "1" | "true" => Some(Arc::new(VLCPlayer::new())),
        _ => None,
    };

    let context = api::Context::from(
        Arc::new(MediaStore::from(&get_movie_dir())),
        Arc::new(adaptors::HTTPClient::new()),
        Arc::new(adaptors::HTTPClient::new()),
        player,
    );

    let downloader: Arc<dyn DownloadClient> = Arc::new(TransmissionDaemon::new());

    let monitor_handle = Monitor::start(context.store.clone(), downloader);

    setup_logging();

    let app = api::register(Arc::new(context))
        .nest_service("/", ServeDir::new(get_client_path("app")))
        .nest_service("/player", ServeDir::new(get_client_path("player")))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default()));

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();

    monitor_handle.abort();

    Ok(())
}

fn setup_logging() {
    let format = fmt::format()
        .with_ansi(false)
        .without_time()
        .with_level(true)
        .with_target(false)
        .compact();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tvserver=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().event_format(format))
        .init();
}

pub fn get_client_path(subdir: &str) -> PathBuf {
    let root_dir = env::var(CLIENT_DIR).unwrap_or(String::from("client"));
    PathBuf::from(root_dir.as_str()).join(subdir)
}
