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

use sqlx::Error;
use std::{net::SocketAddr, sync::Arc};

use crate::adaptors::{FileSystemStore, TokioProcessSpawner};
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::domain::config::{get_database_url, get_thumbnail_dir};
use crate::domain::services::MessageExchange;
use crate::domain::traits::{Downloader, FileStorer};
use crate::domain::{
    config::{enable_vlc_player, get_client_path, get_movie_dir},
    traits::Player,
};
use crate::entrypoints::{register, Context};
use crate::services::{
    MediaStore, MetaDataManager, Monitor, SearchService, TaskManager, TransmissionDaemon, VLCPlayer,
};

pub async fn run() -> anyhow::Result<()> {
    let context = get_dependencies().await?;

    let downloader: Downloader = Arc::new(TransmissionDaemon::new());

    let monitor_handle = Monitor::start(
        context.get_store(),
        downloader,
        context.get_task_manager(),
        context.get_repository(),
    );

    setup_logging();

    let metadata_manager = MetaDataManager::consume(
        context.get_repository(),
        context.get_local_receiver(),
        context.get_local_sender(),
    );

    let app = register(Arc::new(context))
        .nest_service("/", ServeDir::new(get_client_path("app")))
        .nest_service("/player", ServeDir::new(get_client_path("player")))
        .nest_service("/api/stream", ServeDir::new(get_movie_dir()))
        .nest_service(
            "/api/thumbnails",
            ServeDir::new(get_thumbnail_dir(&get_movie_dir())),
        )
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
    metadata_manager.abort();

    Ok(())
}

async fn get_dependencies() -> Result<Context, Error> {
    let messenger = MessageExchange::new();

    let repository = adaptors::SqlRepository::new(&get_database_url()).await?;

    let player: Option<Arc<dyn Player>> = if enable_vlc_player() {
        Some(Arc::new(VLCPlayer::start()))
    } else {
        None
    };

    let task_manager = Arc::new(TaskManager::new(Arc::new(TokioProcessSpawner::new())));

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(&get_movie_dir()));

    Ok(Context::new(
        Arc::new(MediaStore::new(file_storer, messenger.get_local_sender())),
        SearchService::new(task_manager.clone()),
        messenger,
        player,
        task_manager,
        Arc::new(repository),
    ))
}

fn setup_logging() {
    let format = fmt::format()
        .with_ansi(false)
        .without_time()
        .with_level(true)
        .with_target(false)
        .compact();

    //const FILTER: &str = "tvserver=debug,tower_http=debug";
    const FILTER: &str = "tvserver=info,tower_http=debug";

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| FILTER.into()),
        )
        .with(tracing_subscriber::fmt::layer().event_format(format))
        .init();
}
