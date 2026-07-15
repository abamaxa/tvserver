use std::{env, net::SocketAddr};

use anyhow::Result;
use tokio::{task::JoinHandle, time};

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
