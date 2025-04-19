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
use crate::services::{setup_logging, TVSERVER_LOG};
use crate::domain::config::{get_client_path, get_movie_dir, get_thumbnail_dir};
use crate::entrypoints::TVServer;
use crate::entrypoints::register;

pub async fn run() -> anyhow::Result<()> {
    setup_logging(TVSERVER_LOG);

    let tvserver = TVServer::new().await?;

    run_http_server(&tvserver).await?;

    tvserver.shutdown();

    Ok(())
}

async fn run_http_server(tvserver: &TVServer) -> anyhow::Result<()> {
    let context = tvserver.get_context().clone();

    let app = register(Arc::new(context))
        .fallback_service(ServeDir::new(get_client_path("newapp")))
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
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();

    Ok(())
}
