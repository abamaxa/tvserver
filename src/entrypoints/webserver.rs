//! # TVServer
//!
//! `TVServer` is the daemon server that provides a REST API for the remote control and more....
//!
//! Currently its very lightly documented as it is very much a work in progress.

extern crate core;

use std::{env, net::SocketAddr, sync::Arc};

use crate::adaptors::restrict_access;
use crate::domain::config::{
    get_book_thumbnail_dir, get_client_path, get_movie_dir, get_thumbnail_dir, BOOK_DIR,
};
use crate::domain::models::ensure_default_book_thumbnail;
use crate::entrypoints::register;
use crate::entrypoints::TVServer;
use crate::services::{setup_logging, TVSERVER_LOG};
use anyhow::Context as _;
use axum::{middleware, routing::get, Router};
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

pub async fn run_webserver(port: Option<u16>) -> anyhow::Result<()> {
    setup_logging(TVSERVER_LOG);

    let tvserver = TVServer::new().await?;

    run_http_server(&tvserver, port).await?;

    tvserver.shutdown();

    Ok(())
}

async fn run_http_server(tvserver: &TVServer, port: Option<u16>) -> anyhow::Result<()> {
    let context = tvserver.get_context().clone();
    let app = build_http_router(context)?;

    let port = port.unwrap_or(80);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}

pub fn build_http_router(context: crate::entrypoints::Context) -> anyhow::Result<Router> {
    let movie_dir = get_movie_dir();
    let book_dir = env::var(BOOK_DIR)
        .with_context(|| format!("{BOOK_DIR} environment variable is required"))?;
    let book_thumbnail_dir = get_book_thumbnail_dir(&book_dir);
    ensure_default_book_thumbnail(&book_thumbnail_dir).with_context(|| {
        format!(
            "failed to materialize default book thumbnail in {}",
            book_thumbnail_dir.display()
        )
    })?;

    // Protected routes: API endpoints, player, and fallback (app)
    let mut protected_routes = register(Arc::new(context))
        .nest_service("/player", ServeDir::new(get_client_path("player")))
        .fallback_service(ServeDir::new(get_client_path("newapp")));

    // Unprotected routes: streaming and thumbnails (need external access for casting)
    let mut unprotected_routes = Router::new()
        .route(
            "/api/stream-audio/{audio_index}/{*path}",
            get(crate::entrypoints::api::stream_audio),
        )
        .nest_service("/api/stream", ServeDir::new(&movie_dir))
        .nest_service("/api/thumbnails", ServeDir::new(get_thumbnail_dir(&movie_dir)));

    protected_routes =
        protected_routes.nest_service("/api/books/download", ServeDir::new(book_dir));
    unprotected_routes = unprotected_routes
        .nest_service("/api/book-thumbnails", ServeDir::new(book_thumbnail_dir));

    let protected_routes = protected_routes.layer(middleware::from_fn(restrict_access));

    Ok(Router::new()
        .merge(unprotected_routes)
        .merge(protected_routes)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        ))
}
