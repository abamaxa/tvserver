mod adaptors;
mod domain;
mod services;

use axum::routing::get;
use tower_http::{trace::TraceLayer, cors::CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, filter::LevelFilter, filter};
use std::{env, net::SocketAddr, sync::Arc};


pub async fn run() -> anyhow::Result<()> {
    let pool = adaptors::repository::get_database().await?;

    adaptors::repository::do_migrations(&pool).await.unwrap();

    let player = Arc::new(adaptors::player::VLCPlayer::new());

    let movie_dir = env::var("MOVIE_DIR").expect("MOVIE_DIR environment variable is not set");

    let store = Arc::new(adaptors::filestore::FileStore::from(&movie_dir));

    let filter = filter::Targets::new()
        .with_target("tower_http::trace::on_response", LevelFilter::DEBUG)
        .with_target("tower_http::trace::on_request", LevelFilter::DEBUG)
        .with_target("tower_http::trace::make_span", LevelFilter::DEBUG)
        .with_default(LevelFilter::INFO);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let app = services::web::register(player, store)
        .nest_service("/", get(services::client_serving::file_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 4000));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
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
