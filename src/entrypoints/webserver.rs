//! # TVServer
//!
//! `TVServer` is the daemon server that provides a REST API for the remote control and more....
//!
//! Currently its very lightly documented as it is very much a work in progress.

extern crate core;

use std::{net::SocketAddr, sync::Arc};
use axum::{middleware, routing::get, Router};
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use crate::adaptors::restrict_access;
use crate::services::{setup_logging, TVSERVER_LOG};
use crate::domain::config::{get_client_path, get_movie_dir, get_thumbnail_dir};
use crate::entrypoints::TVServer;
use crate::entrypoints::register;

pub async fn run_webserver(port: Option<u16>) -> anyhow::Result<()> {
    setup_logging(TVSERVER_LOG);

    let tvserver = TVServer::new().await?;

    let server_result = run_http_server(&tvserver, port).await;

    tvserver.shutdown().await;

    server_result
}

async fn run_http_server(tvserver: &TVServer, port: Option<u16>) -> anyhow::Result<()> {
    let context = tvserver.get_context().clone();

    // Protected routes: API endpoints, player, and fallback (app)
    let protected_routes = register(Arc::new(context))
        .nest_service("/player", ServeDir::new(get_client_path("player")))
        .fallback_service(ServeDir::new(get_client_path("newapp")))
        .layer(middleware::from_fn(restrict_access));

    // Unprotected routes: streaming and thumbnails (need external access for casting)
    let unprotected_routes = Router::new()
        .route("/api/stream-audio/{audio_index}/{*path}", get(crate::entrypoints::api::stream_audio))
        .nest_service("/api/stream", ServeDir::new(get_movie_dir()))
        .nest_service(
            "/api/thumbnails",
            ServeDir::new(get_thumbnail_dir(&get_movie_dir())),
        );

    let app = Router::new()
        .merge(unprotected_routes)
        .merge(protected_routes)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        );

    let port = port.unwrap_or(80);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    Ok(())
}
