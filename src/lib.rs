//! # TVServer
//!
//! `TVServer` is the daemon server that provides a REST API for the remote control and more....
//!
//! Currently its very lightly documented as it is very much a work in progress.

extern crate core;

pub mod adaptors;
pub mod domain;
pub mod entrypoints;
pub mod services;

use std::{net::SocketAddr, sync::Arc};

use crate::adaptors::TokioProcessSpawner;
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::domain::traits::Downloader;
use crate::domain::{
    config::{enable_vlc_player, get_client_path, get_movie_dir},
    traits::Player,
};
use crate::entrypoints::{register, Context};
use crate::services::{
    MediaStore, Monitor, RemotePlayerService, SearchService, TaskManager, TransmissionDaemon,
    VLCPlayer,
};

pub async fn run() -> anyhow::Result<()> {
    let pool = adaptors::get_database().await?;

    adaptors::do_migrations(&pool).await?;

    let context = get_dependencies();

    let downloader: Downloader = Arc::new(TransmissionDaemon::new());

    let monitor_handle =
        Monitor::start(context.get_store(), downloader, context.get_task_manager());

    setup_logging();

    let app = register(Arc::new(context))
        .nest_service("/", ServeDir::new(get_client_path("app")))
        .nest_service("/player", ServeDir::new(get_client_path("player")))
        .nest_service("/stream", ServeDir::new(get_movie_dir()))
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();

    monitor_handle.abort();

    Ok(())
}

fn get_dependencies() -> Context {
    let player: Option<Arc<dyn Player>> = if enable_vlc_player() {
        Some(Arc::new(VLCPlayer::start()))
    } else {
        None
    };

    let task_manager = Arc::new(TaskManager::new(Arc::new(TokioProcessSpawner::new())));

    Context::from(
        Arc::new(MediaStore::from(&get_movie_dir())),
        SearchService::new(task_manager.clone()),
        RemotePlayerService::new(),
        player,
        task_manager,
    )
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
