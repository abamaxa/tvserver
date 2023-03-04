extern crate core;

mod adaptors;
mod domain;
mod services;

use std::{env, net::SocketAddr, sync::Arc};

use axum::routing::get;
use tower_http::{trace::{DefaultMakeSpan, TraceLayer}, cors::CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, filter::LevelFilter, filter};

use crate::domain::{config::get_movie_dir, ENABLE_VLC,traits::{Player, MediaStorer}};
use crate::services::{
    api,
    client_apps,
    media_store::MediaStore,
    monitor::monitor_downloads,
    vlc_player::VLCPlayer,
};


pub async fn run() -> anyhow::Result<()> {

    let pool = adaptors::repository::get_database().await?;

    adaptors::repository::do_migrations(&pool).await?;

    let store: Arc<dyn MediaStorer> = Arc::new(MediaStore::from(&get_movie_dir()));

    let monitor_handle = monitor_downloads(store.clone());

    let enable_vlc = env::var(ENABLE_VLC).unwrap_or_default();

    let player: Option<Arc<dyn Player>> = match enable_vlc.as_str() {
        "1" | "true" => Some(Arc::new(VLCPlayer::new())),
        _ => None,
    };

    let filter = filter::Targets::new()
        .with_target("tower_http::trace::on_response", LevelFilter::DEBUG)
        .with_target("tower_http::trace::on_request", LevelFilter::INFO)
        .with_target("tower_http::trace::make_span", LevelFilter::DEBUG)
        .with_default(LevelFilter::INFO);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "websockets=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .with(filter)
        .init();

    let app = api::register(player, store)
        .nest_service("/", get(client_apps::app_handler))
        .nest_service("/player", get(client_apps::player_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default().include_headers(false)));

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();

    monitor_handle.abort();

    Ok(())
}
