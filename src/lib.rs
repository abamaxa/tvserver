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
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

use crate::{domain::traits::Downloader, services::{setup_logging, TVSERVER_LOG}};
use crate::domain::config::{get_client_path, get_movie_dir, get_thumbnail_dir};
use crate::entrypoints::create_context;
use crate::entrypoints::register;
use crate::services::{
    MetaDataManager, Monitor, TransmissionDaemon,
};

pub async fn run() -> anyhow::Result<()> {
    let context = create_context().await?;

    let downloader: Downloader = Arc::new(TransmissionDaemon::new());

    let monitor_handle = Monitor::start(
        context.get_store(),
        downloader,
        context.get_task_manager(),
    );

    setup_logging(TVSERVER_LOG);

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
