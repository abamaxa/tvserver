extern crate core;

mod adaptors;
mod domain;
mod services;

use axum::routing::get;
use tower_http::{trace::{DefaultMakeSpan, TraceLayer}, cors::CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, filter::LevelFilter, filter};
use std::{net::SocketAddr, sync::Arc};
use crate::domain::config::get_movie_dir;
use crate::domain::traits::{Player, VideoStore};
use crate::services::download_monitor::monitor_downloads;


pub async fn run() -> anyhow::Result<()> {

    let pool = adaptors::repository::get_database().await?;

    adaptors::repository::do_migrations(&pool).await.unwrap();

    let store: Arc<dyn VideoStore> = Arc::new(
        adaptors::filestore::FileStore::from(&get_movie_dir())
    );

    let _ = monitor_downloads(store.clone());

    let player: Option<Arc<dyn Player>> = None;

    // player = Some(Arc::new(adaptors::vlc_player::VLCPlayer::new()));
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

    let app = services::web::register(player, store)
        .nest_service("/", get(services::client_serving::app_handler))
        .nest_service("/player", get(services::client_serving::player_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default().include_headers(true)));

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
