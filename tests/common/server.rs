use std::{net::SocketAddr, sync::Arc};

use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

use anyhow::Result;
use tokio::{task::JoinHandle, time};

use app_lib::{
    domain::config::{get_client_path, get_movie_dir},
    entrypoints::{register, Context},
};

pub async fn create_server(context: Context, port: u16) -> JoinHandle<Result<()>> {
    let task = tokio::spawn(async move {
        let app = register(Arc::new(context))
            .nest_service("/", ServeDir::new(get_client_path("app")))
            .nest_service("/player", ServeDir::new(get_client_path("player")))
            .nest_service("/api/stream", ServeDir::new(get_movie_dir()))
            .layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default()));

        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        axum::serve(tokio::net::TcpListener::bind(&addr).await?, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

        Ok(())
    });

    // wait for the server to come up
    time::sleep(time::Duration::from_millis(100)).await;

    task
}
