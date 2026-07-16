use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::Result;
use tokio::{task::JoinHandle, time};

use app_lib::{
    domain::config::BOOK_DIR,
    entrypoints::{
        webserver::{build_http_router, build_http_router_with_roots},
        Context,
    },
};

pub async fn create_server(context: Context, port: u16) -> JoinHandle<Result<()>> {
    if env::var_os(BOOK_DIR).is_none() {
        env::set_var(
            BOOK_DIR,
            env::temp_dir().join(format!("tvserver-http-tests-{}", std::process::id())),
        );
    }

    create_server_task(context, port, None).await
}

pub async fn create_server_with_book_roots(
    context: Context,
    port: u16,
    book_root: PathBuf,
    thumbnail_root: PathBuf,
) -> JoinHandle<Result<()>> {
    create_server_task(context, port, Some((book_root, thumbnail_root))).await
}

async fn create_server_task(
    context: Context,
    port: u16,
    book_roots: Option<(PathBuf, PathBuf)>,
) -> JoinHandle<Result<()>> {
    let task = tokio::spawn(async move {
        let app = match book_roots {
            Some((book_root, thumbnail_root)) => {
                build_http_router_with_roots(context, book_root, thumbnail_root)?
            }
            None => build_http_router(context)?,
        };

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
