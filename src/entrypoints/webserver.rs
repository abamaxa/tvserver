//! # TVServer
//!
//! `TVServer` is the daemon server that provides a REST API for the remote control and more....
//!
//! Currently its very lightly documented as it is very much a work in progress.

extern crate core;

use std::{net::SocketAddr, sync::Arc};

use crate::adaptors::restrict_access;
use crate::domain::config::{get_client_path, get_movie_dir, get_thumbnail_dir};
use crate::entrypoints::capability_file_service::{CapabilityBackend, StaticFilePolicy};
use crate::entrypoints::register;
use crate::entrypoints::TVServer;
use crate::services::{setup_logging, TVSERVER_LOG};
use axum::{
    body::Body,
    extract::Request,
    http::{
        header::{IF_RANGE, RANGE},
        HeaderName, HeaderValue, Method, StatusCode,
    },
    middleware,
    middleware::Next,
    response::Response,
    routing::get,
    Router,
};
use http_range_header::{parse_range_header, EndPosition, StartPosition};
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};

const MAX_VIDEO_RANGE_BYTES: u64 = 8 * 1024 * 1024;

const READER_CONTENT_SECURITY_POLICY: &str = concat!(
    "default-src 'self'; ",
    "base-uri 'none'; object-src 'none'; frame-ancestors 'none'; ",
    "script-src 'self'; style-src 'self' 'unsafe-inline' blob:; ",
    "img-src 'self' data: blob:; font-src 'self' data: blob:; ",
    "media-src 'self' blob:; frame-src blob:; worker-src 'self' blob:; ",
    "connect-src 'self' ws: wss:; form-action 'self'; ",
    "navigate-to 'self' http: https: mailto: tel:"
);

fn with_security_headers(mut response: Response) -> Response {
    response.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(READER_CONTENT_SECURITY_POLICY),
    );
    response
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    with_security_headers(next.run(request).await)
}

async fn empty_unsatisfiable_range_body(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        response.headers_mut().insert(
            HeaderName::from_static("content-length"),
            HeaderValue::from_static("0"),
        );
        *response.body_mut() = Body::empty();
    }
    response
}

fn normalize_static_file_request(request: &mut Request<Body>) -> bool {
    if request.method() != Method::GET || request.headers().contains_key(IF_RANGE) {
        request.headers_mut().remove(RANGE);
        return false;
    }

    true
}

fn capped_video_range(range: &HeaderValue) -> Option<HeaderValue> {
    let parsed = parse_range_header(range.to_str().ok()?).ok()?;
    let [range] = parsed.ranges.as_slice() else {
        return None;
    };

    let value = match (range.start, range.end) {
        (StartPosition::Index(start), EndPosition::LastByte) => {
            format!(
                "bytes={start}-{}",
                start.saturating_add(MAX_VIDEO_RANGE_BYTES - 1)
            )
        }
        (StartPosition::Index(start), EndPosition::Index(end))
            if end > start.saturating_add(MAX_VIDEO_RANGE_BYTES - 1) =>
        {
            format!(
                "bytes={start}-{}",
                start.saturating_add(MAX_VIDEO_RANGE_BYTES - 1)
            )
        }
        (StartPosition::FromLast(suffix), EndPosition::LastByte)
            if suffix > MAX_VIDEO_RANGE_BYTES =>
        {
            format!("bytes=-{MAX_VIDEO_RANGE_BYTES}")
        }
        _ => return None,
    };

    HeaderValue::from_str(&value).ok()
}

async fn conservatively_normalize_static_file_request(
    mut request: Request<Body>,
    next: Next,
) -> Response {
    normalize_static_file_request(&mut request);
    next.run(request).await
}

async fn normalize_video_stream_request(mut request: Request<Body>, next: Next) -> Response {
    if normalize_static_file_request(&mut request) {
        if let Some(range) = request.headers().get(RANGE).and_then(capped_video_range) {
            request.headers_mut().insert(RANGE, range);
        }
    }
    next.run(request).await
}

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
    let video_stream_service = Router::new()
        .fallback_service(ServeDir::new(&movie_dir))
        .layer(middleware::from_fn(normalize_video_stream_request));

    let mut unprotected_routes = Router::new()
        .route(
            "/api/stream-audio/{audio_index}/{*path}",
            get(crate::entrypoints::api::stream_audio),
        )
        .nest_service("/api/stream", video_stream_service)
        .nest_service("/api/thumbnails", ServeDir::new(get_thumbnail_dir(&movie_dir)));

    let (book_download_routes, book_thumbnail_routes) = match book_static_roots {
        Some(roots) => (
            Router::new()
                .route_service(
                    "/{*path}",
                    ServeDir::with_backend(
                        "",
                        CapabilityBackend::new(roots.downloads, StaticFilePolicy::BookDownload),
                    ),
                )
                .layer(middleware::from_fn(empty_unsatisfiable_range_body))
                .layer(middleware::from_fn(conservatively_normalize_static_file_request)),
            Router::new()
                .route_service(
                    "/{file}",
                    ServeDir::with_backend(
                        "",
                        CapabilityBackend::new(roots.thumbnails, StaticFilePolicy::BookThumbnail),
                    ),
                )
                .layer(middleware::from_fn(empty_unsatisfiable_range_body)),
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
        )
        .layer(middleware::from_fn(security_headers)))
}

async fn serve_unavailable_book_static() -> StatusCode {
    StatusCode::SERVICE_UNAVAILABLE
}

#[cfg(test)]
mod security_tests {
    use super::*;

    fn script_sources_are_self_only(policy: &str) -> bool {
        let mut script_src_count = 0;

        for directive in policy.split(';') {
            let mut tokens = directive.split_ascii_whitespace();
            let Some(name) = tokens.next() else {
                continue;
            };
            if !name.eq_ignore_ascii_case("script-src") {
                continue;
            }

            script_src_count += 1;
            if tokens.next() != Some("'self'") || tokens.next().is_some() {
                return false;
            }
        }

        script_src_count == 1
    }

    #[test]
    fn reader_csp_blocks_scripts_and_objects_without_blocking_reader_assets() {
        let response = with_security_headers(Response::new(Body::empty()));
        let policy = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        for directive in [
            "default-src 'self'",
            "script-src 'self'",
            "worker-src 'self' blob:",
            "frame-src blob:",
            "object-src 'none'",
            "base-uri 'none'",
            "frame-ancestors 'none'",
        ] {
            assert!(policy.contains(directive), "missing {directive}");
        }
        assert!(script_sources_are_self_only(policy));
    }

    #[test]
    fn reader_csp_blocks_scripts_with_duplicate_mixed_case_directives() {
        assert!(!script_sources_are_self_only(
            "SCRIPT-SRC https://evil; script-src 'self'"
        ));
    }
}
