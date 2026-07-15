use std::{net::SocketAddr, sync::Arc};

use tower_http::{
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

use anyhow::Result;
use tokio::{task::JoinHandle, time};

use app_lib::{
    domain::config::{get_book_thumbnail_dir, get_client_path, get_movie_dir, BOOK_DIR},
    entrypoints::{register, Context},
};

pub async fn create_server(context: Context, port: u16) -> JoinHandle<Result<()>> {
    let task = tokio::spawn(async move {
        let mut app = register(Arc::new(context))
            .nest_service("/player", ServeDir::new(get_client_path("player")))
            .nest_service("/api/stream", ServeDir::new(get_movie_dir()));

        if let Ok(book_dir) = std::env::var(BOOK_DIR) {
            let book_thumbnail_dir = get_book_thumbnail_dir(&book_dir);
            app = app
                .nest_service("/api/books/download", ServeDir::new(book_dir))
                .nest_service(
                    "/api/book-thumbnails",
                    ServeDir::new(book_thumbnail_dir),
                );
        }

        let app = app
            .fallback_service(ServeDir::new(get_client_path("app")))
            .layer(TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::default()));

        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        axum::serve(tokio::net::TcpListener::bind(&addr).await?, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

        Ok(())
    });

    // wait for the server to come up
    time::sleep(time::Duration::from_millis(100)).await;

    task
}
