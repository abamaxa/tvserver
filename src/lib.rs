//! # TVServer
//!
//! `TVServer` is the daemon server that provides a REST API for the remote control and more....
//!
//! Currently its very lightly documented as it is very much a work in progress.

extern crate core;

pub mod adaptors;
pub mod domain;
pub mod entrypoints;
pub mod services;

use anyhow::anyhow;
use axum::body::BoxBody;
use axum::http::{HeaderValue, StatusCode};
use axum::{
    body::{Body, HttpBody},
    http, Router,
};
use futures::task::noop_waker_ref;
use std::pin::Pin;
use std::str::FromStr;
use std::task::Poll;
use std::{net::SocketAddr, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::adaptors::TokioProcessSpawner;
use tower::Service;
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};
use urlencoding::decode;

use crate::domain::traits::Downloader;
use crate::domain::{
    config::{enable_vlc_player, get_client_path, get_movie_dir},
    traits::Player,
};
use crate::entrypoints::{register, Context};
use crate::services::{
    get_range, MediaStore, Monitor, RemotePlayerService, SearchService, TaskManager,
    TransmissionDaemon, VLCPlayer,
};

pub async fn run() -> anyhow::Result<()> {
    let pool = adaptors::get_database().await?;

    adaptors::do_migrations(&pool).await?;

    let context = get_dependencies();

    let downloader: Downloader = Arc::new(TransmissionDaemon::new());

    let monitor_handle =
        Monitor::start(context.get_store(), downloader, context.get_task_manager());

    setup_logging();

    let app = register(Arc::new(context))
        .nest_service("/", ServeDir::new(get_client_path("app")))
        .nest_service("/player", ServeDir::new(get_client_path("player")))
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(false)),
        );

    /*let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();*/

    let addr = SocketAddr::from(([0, 0, 0, 0], 80));
    let listener = TcpListener::bind(addr).await.unwrap();

    println!("Server listening on {}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let app = app.clone();
                tokio::spawn(process_request(stream, app));
            }
            Err(e) => eprintln!("Failed to accept connection: {}", e),
        }
    }

    monitor_handle.abort();

    Ok(())
}

fn get_dependencies() -> Context {
    let player: Option<Arc<dyn Player>> = if enable_vlc_player() {
        Some(Arc::new(VLCPlayer::start()))
    } else {
        None
    };

    let task_manager = Arc::new(TaskManager::new(Arc::new(TokioProcessSpawner::new())));

    Context::from(
        Arc::new(MediaStore::from(&get_movie_dir())),
        SearchService::new(task_manager.clone()),
        RemotePlayerService::new(),
        player,
        task_manager,
    )
}

fn setup_logging() {
    let format = fmt::format()
        .with_ansi(false)
        .without_time()
        .with_level(true)
        .with_target(false)
        .compact();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tvserver=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().event_format(format))
        .init();
}

async fn process_request(mut stream: tokio::net::TcpStream, mut app: Router) {
    let mut buf = vec![0u8; 4096];
    let end_buf = match stream.read(&mut buf).await {
        Ok(end_buf) => end_buf,
        Err(err) => {
            tracing::error!("reading stream: {}", err);
            return;
        }
    };

    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);

    let body_offset = match req.parse(&buf[..end_buf]) {
        Ok(httparse::Status::Complete(body_offset)) => body_offset,
        Ok(httparse::Status::Partial) => {
            tracing::error!("partial read stream: {:?}", String::from_utf8(buf));
            return;
        }
        Err(e) => {
            tracing::error!("error reading stream: {}", e);
            return;
        }
    };

    if req.path.is_none() {
        tracing::error!("request does not contain a path");
        return;
    }

    let parsed_path = match decode(req.path.unwrap()) {
        Ok(path) => path,
        _ => {
            tracing::error!("could not parse path: {:?}", req.path);
            return;
        }
    };

    // req.path = Some(&parsed_path);

    if let Some(video_name) = parsed_path.strip_prefix("/stream/") {
        let (start, end) = get_range(&req);
        services::stream_video(&mut stream, video_name, start, end).await;
    } else {
        if let Some(request) = make_request(&req, &buf[body_offset..end_buf]) {
            if let Ok(response) = app.call(request).await {
                if let Err(e) = write_response_to_stream(response, stream).await {
                    tracing::error!("could not write result {}", e.to_string());
                }
            }
        }
    }
}

fn make_request(req: &httparse::Request, body: &[u8]) -> Option<http::Request<Body>> {
    let request = http::Request::new(Body::from(body.to_owned()));

    let (mut parts, body) = request.into_parts();

    // println!("method: {:?}, uri: {:?}", req.method, req.path);

    parts.method = http::Method::from_str(&req.method?).ok()?;
    parts.uri = req.path?.parse::<http::Uri>().unwrap();

    let mut headers = http::HeaderMap::with_capacity(req.headers.len());

    for h in req.headers.iter() {
        if let Ok(name) = http::HeaderName::from_bytes(h.name.as_bytes()) {
            let value =
                http::HeaderValue::from_bytes(h.value).unwrap_or(HeaderValue::from_static("????"));
            headers.insert(name, value);
        }
    }
    parts.headers = headers;
    /* version: Version::default(),
    extensions: Extensions::default()*/
    Some(http::Request::from_parts(parts, body))
}

async fn write_response_to_stream(
    mut response: http::Response<BoxBody>,
    mut stream: tokio::net::TcpStream,
) -> anyhow::Result<()> {
    let status_code = response.status();
    let version = response.version();
    let headers = response.headers();

    let http_version = match version {
        http::Version::HTTP_09 => "HTTP/0.9",
        http::Version::HTTP_10 => "HTTP/1.0",
        http::Version::HTTP_11 => "HTTP/1.1",
        http::Version::HTTP_2 => "HTTP/2.0",
        http::Version::HTTP_3 => "HTTP/3.0",
        _ => "HTTP/1.1",
    };

    let status_line = format!(
        "{} {} {}\r\n",
        http_version,
        status_code.as_str(),
        status_code.canonical_reason().unwrap_or("UNKNOWN")
    );
    stream.write_all(status_line.as_bytes()).await?;

    for (key, value) in headers.iter() {
        let header_line = format!("{}: {}\r\n", key.as_str(), value.to_str()?);
        stream.write_all(header_line.as_bytes()).await?;
    }

    stream.write_all(b"\r\n").await?;

    /*let body: &mut BoxBody = response.body_mut();

    if let Some(Ok(data)) = body.data().await {
        stream.write_all(&data).await?;
    }*/

    let body = response.body_mut();
    let mut data: Vec<u8> = Vec::new();

    loop {
        // totally fucked up, just want to write the body to the socket but we have
        // no direct access to the data, instead we have to use this shit async
        // interface cos some genius thought the same Body object could serve for
        // both Requests and Responses. So badly designed.
        let mut ctx = std::task::Context::from_waker(noop_waker_ref());
        let poll_result = Pin::new(&mut *body).poll_data(&mut ctx);
        match poll_result {
            Poll::Ready(Some(Ok(chunk))) => {
                // Can't call await here cos something (context? Data?) isn't Send....
                //stream.write_all(chunk.as_ref()).await?;
                data.extend(chunk.as_ref().iter());
            }
            Poll::Ready(Some(Err(_e))) => {
                return Err(anyhow!("fffff"));
            }
            Poll::Ready(None) => {
                break;
            }
            Poll::Pending => {
                // Its never going to be pending, we're just reading a fucking
                // byte array here through some shit design interface.
                //tokio::task::yield_now().await;
            }
        }
    }

    stream.write_all(&data).await?;

    if status_code == StatusCode::UPGRADE_REQUIRED {
        println!("upgrade response: {:?}", String::from_utf8(data));
    }

    stream.flush().await?;

    Ok(())
}
