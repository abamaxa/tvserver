use std::net::SocketAddr;

use anyhow::Result;
use tokio::{task::JoinHandle, time};

use app_lib::entrypoints::{webserver::build_http_router, Context};

pub async fn create_server(context: Context, port: u16) -> JoinHandle<Result<()>> {
    let task = tokio::spawn(async move {
        let app = build_http_router(context)?;

        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        axum::serve(
            tokio::net::TcpListener::bind(&addr).await?,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;

        Ok(())
    });

    // wait for the server to come up
    time::sleep(time::Duration::from_millis(100)).await;

    task
}
