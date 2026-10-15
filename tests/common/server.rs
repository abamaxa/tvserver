use std::{env, net::SocketAddr};

use anyhow::Result;
use tokio::task::JoinHandle;

use app_lib::{
    domain::config::BOOK_DIR,
    entrypoints::{webserver::build_http_router, Context},
};

pub async fn create_server(context: Context, port: u16) -> JoinHandle<Result<()>> {
    if env::var_os(BOOK_DIR).is_none() {
        env::set_var(
            BOOK_DIR,
            env::temp_dir().join(format!("tvserver-http-tests-{}", std::process::id())),
        );
    }

    create_server_task(context, port).await
}

async fn create_server_task(context: Context, port: u16) -> JoinHandle<Result<()>> {
    // Bind before spawning so the port is already listening when this returns;
    // a fixed sleep raced with the spawned task on slow CI runners.
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind test server on {addr}: {e}"));

    tokio::spawn(async move {
        let app = build_http_router(context)?;

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;

        Ok(())
    })
}
