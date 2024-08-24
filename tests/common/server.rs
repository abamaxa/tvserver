use std::{net::SocketAddr, sync::Arc};

use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

use anyhow::Result;
use tokio::{net::TcpListener, task::JoinHandle, time};

use tvserver::{
    domain::config::get_client_path,
    entrypoints::{register, Context},
};

pub async fn create_server(context: Context, port: u16) -> JoinHandle<Result<()>> {
    let task = tokio::spawn(async move {
        let app = register(Arc::new(context))
            .nest_service("/", ServeDir::new(get_client_path("app")))
            .nest_service("/player", ServeDir::new(get_client_path("player")))
            .layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default()));

        let listener = TcpListener::bind(("0, 0, 0, 0", port)).await?;
        axum::serve(listener, app).await?;

        Ok(())
    });

    // wait for the server to come up
    time::sleep(time::Duration::from_millis(100)).await;

    task
}
