//! # TVServer
//!
//! `TVServer` is the daemon server that provides a REST API for the remote control and more....
//!
//! Currently its very lightly documented as it is very much a work in progress.

extern crate core;

use std::{
    ffi::OsString,
    net::SocketAddr,
    path::{Component, Path},
    sync::Arc,
};

use crate::adaptors::restrict_access;
use crate::domain::config::{
    get_client_path, get_movie_dir, get_thumbnail_dir,
};
use crate::entrypoints::register;
use crate::entrypoints::TVServer;
use crate::services::{setup_logging, TVSERVER_LOG};
use anyhow::anyhow;
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::{
    fs::{Dir, OpenOptions},
};
use tokio_util::io::ReaderStream;
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

pub async fn run_webserver(port: Option<u16>) -> anyhow::Result<()> {
    setup_logging(TVSERVER_LOG);

    let tvserver = TVServer::new().await?;

    let server_result = run_http_server(&tvserver, port).await;

    tvserver.shutdown().await;

    server_result
}

async fn run_http_server(tvserver: &TVServer, port: Option<u16>) -> anyhow::Result<()> {
    let context = tvserver.get_context().clone();
    let app = build_http_router(context)?;

    let port = port.unwrap_or(80);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}

pub fn build_http_router(context: crate::entrypoints::Context) -> anyhow::Result<Router> {
    let movie_dir = get_movie_dir();
    let book_static_roots = context.get_book_runtime().static_roots();

    // Protected routes: API endpoints, player, and fallback (app)
    let mut protected_routes = register(Arc::new(context))
        .nest_service("/player", ServeDir::new(get_client_path("player")))
        .fallback_service(ServeDir::new(get_client_path("newapp")));

    // Unprotected routes: streaming and thumbnails (need external access for casting)
    let mut unprotected_routes = Router::new()
        .route(
            "/api/stream-audio/{audio_index}/{*path}",
            get(crate::entrypoints::api::stream_audio),
        )
        .nest_service("/api/stream", ServeDir::new(&movie_dir))
        .nest_service("/api/thumbnails", ServeDir::new(get_thumbnail_dir(&movie_dir)));

    let (book_download_routes, book_thumbnail_routes) = match book_static_roots {
        Some(roots) => (
            Router::new()
                .route("/{*path}", get(serve_book_download))
                .with_state(RetainedRoot::new(roots.downloads)),
            Router::new()
                .route("/{file}", get(serve_book_thumbnail))
                .with_state(RetainedRoot::new(roots.thumbnails)),
        ),
        None => (
            Router::new().route("/{*path}", get(serve_unavailable_book_static)),
            Router::new().route("/{file}", get(serve_unavailable_book_static)),
        ),
    };

    protected_routes =
        protected_routes.nest("/api/books/download", book_download_routes);
    unprotected_routes =
        unprotected_routes.nest("/api/book-thumbnails", book_thumbnail_routes);

    let protected_routes = protected_routes.layer(middleware::from_fn(restrict_access));

    Ok(Router::new()
        .merge(unprotected_routes)
        .merge(protected_routes)
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        ))
}

#[derive(Clone)]
struct RetainedRoot {
    dir: Arc<Dir>,
}

impl RetainedRoot {
    fn new(dir: Arc<Dir>) -> Self {
        Self { dir }
    }
}

struct OpenedStaticFile {
    file: cap_std::fs::File,
    len: u64,
}

fn normal_components(path: &str, allow_nested: bool) -> anyhow::Result<Vec<OsString>> {
    let mut components = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(component) => components.push(component.to_os_string()),
            Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_)
            | Component::RootDir => return Err(anyhow!("static file path must be relative")),
        }
    }
    if components.is_empty() || (!allow_nested && components.len() != 1) {
        return Err(anyhow!("static file path has an invalid component count"));
    }
    Ok(components)
}

fn open_regular_file(root: &Dir, components: &[OsString]) -> anyhow::Result<OpenedStaticFile> {
    let mut current = root.try_clone()?;
    for component in &components[..components.len() - 1] {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No);
        let child = current.open_with(Path::new(component), &options)?;
        if !child.metadata()?.is_dir() {
            return Err(anyhow!("static file parent is not a directory"));
        }
        current = Dir::from_std_file(child.into_std());
    }

    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = current.open_with(
        Path::new(components.last().expect("validated non-empty components")),
        &options,
    )?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(anyhow!("static file is not a regular file"));
    }
    Ok(OpenedStaticFile {
        file,
        len: metadata.len(),
    })
}

fn download_content_type(path: &str) -> anyhow::Result<&'static str> {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => Ok("application/pdf"),
        Some("epub") => Ok("application/epub+zip"),
        _ => Err(anyhow!("unsupported book download extension")),
    }
}

fn thumbnail_content_type(path: &str) -> anyhow::Result<&'static str> {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg") => Ok("image/jpeg"),
        _ => Err(anyhow!("unsupported book thumbnail extension")),
    }
}

async fn stream_static_file(
    root: RetainedRoot,
    path: String,
    allow_nested: bool,
    content_type: &'static str,
) -> Response {
    let open_path = path.clone();
    let opened = tokio::task::spawn_blocking(move || {
        let components = normal_components(&open_path, allow_nested)?;
        open_regular_file(&root.dir, &components)
    })
    .await;

    let opened = match opened {
        Ok(Ok(opened)) => opened,
        Ok(Err(error)) => {
            tracing::warn!("Rejected static book file {}: {}", path, error);
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => {
            tracing::error!("Static book file task failed for {}: {}", path, error);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let file = tokio::fs::File::from_std(opened.file.into_std());
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, opened.len)
        .body(Body::from_stream(ReaderStream::new(file)))
        .unwrap_or_else(|error| {
            tracing::error!("Failed to build static book response for {}: {}", path, error);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })
}

async fn serve_book_download(
    State(root): State<RetainedRoot>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    let content_type = match download_content_type(&path) {
        Ok(content_type) => content_type,
        Err(error) => {
            tracing::warn!("Rejected book download {}: {}", path, error);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    stream_static_file(root, path, true, content_type).await
}

async fn serve_book_thumbnail(
    State(root): State<RetainedRoot>,
    AxumPath(file): AxumPath<String>,
) -> Response {
    let content_type = match thumbnail_content_type(&file) {
        Ok(content_type) => content_type,
        Err(error) => {
            tracing::warn!("Rejected book thumbnail {}: {}", file, error);
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    stream_static_file(root, file, false, content_type).await
}

async fn serve_unavailable_book_static() -> StatusCode {
    StatusCode::SERVICE_UNAVAILABLE
}
