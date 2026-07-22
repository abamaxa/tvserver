#![cfg(feature = "webserver")]

mod common;

use std::{
    env,
    ffi::OsString,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{ensure, Result};
use app_lib::{
    adaptors::SqlRepository,
    domain::{config::MOVIE_DIR, traits::Repository},
    entrypoints::webserver::build_http_router,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use crate::common::{
    get_book_services_at, get_checker, get_context_with_book_services, get_media_store,
    get_pirate_search, get_task_manager,
};

const AUTH_CREDENTIALS: &str = "AUTH_CREDENTIALS";
const BOOKS_PROTOCOL: &str = "books-v1";
const VALID_AUTH_PROTOCOL: &str = "basic.dGVzdDpzZWNyZXQ";
const PUBLIC_IP: &str = "8.8.8.8";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            env::temp_dir().join(format!("tvserver-websocket-auth-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    fn set(values: &[(&'static str, OsString)]) -> Self {
        let originals = values
            .iter()
            .map(|(name, _)| (*name, env::var_os(name)))
            .collect();
        for (name, value) in values {
            env::set_var(name, value);
        }
        Self(originals)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            match value {
                Some(value) => env::set_var(name, value),
                None => env::remove_var(name),
            }
        }
    }
}

async fn websocket_handshake(
    address: SocketAddr,
    protocols: &str,
    forwarded_ip: Option<&str>,
) -> Result<String> {
    let mut stream = TcpStream::connect(address).await?;
    let forwarded_header = forwarded_ip
        .map(|ip| format!("X-Real-IP: {ip}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET /api/remote/ws HTTP/1.1\r\n\
         Host: {address}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Protocol: {protocols}\r\n\
         {forwarded_header}\r\n"
    );
    stream.write_all(request.as_bytes()).await?;

    let response = timeout(Duration::from_secs(2), async {
        let mut response = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !response.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).await?;
            ensure!(read > 0, "connection closed before the handshake response");
            response.extend_from_slice(&buffer[..read]);
        }
        Result::<Vec<u8>>::Ok(response)
    })
    .await??;
    Ok(String::from_utf8(response)?)
}

fn response_header<'a>(response: &'a str, expected_name: &str) -> Option<&'a str> {
    response.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(expected_name)
            .then_some(value.trim())
    })
}

#[tokio::test]
async fn router_authenticates_and_negotiates_the_books_websocket_upgrade() -> Result<()> {
    let temp = TempRoot::new()?;
    let movie_root = temp.0.join("movies");
    let book_root = temp.0.join("books");
    let thumbnail_root = book_root.join(".thumbnails");
    std::fs::create_dir(&movie_root)?;
    let _env = EnvGuard::set(&[
        (MOVIE_DIR, movie_root.into_os_string()),
        (AUTH_CREDENTIALS, OsString::from("test:secret")),
    ]);

    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    let book_runtime = get_book_services_at(repository.clone(), &book_root, &thumbnail_root).await;
    let context = get_context_with_book_services(
        get_media_store(),
        get_pirate_search("torrents_get.json", "pb_search.html").await,
        get_task_manager(),
        repository,
        get_checker(),
        book_runtime,
    )
    .await?;
    let app = build_http_router(context)?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
    });

    let local = websocket_handshake(address, BOOKS_PROTOCOL, None).await?;
    assert!(local.starts_with("HTTP/1.1 101"), "{local}");
    assert_eq!(
        response_header(&local, "Sec-WebSocket-Protocol"),
        Some(BOOKS_PROTOCOL)
    );

    for protocols in [BOOKS_PROTOCOL, "books-v1, basic.d3Jvbmc6d3Jvbmc"] {
        let rejected = websocket_handshake(address, protocols, Some(PUBLIC_IP)).await?;
        assert!(rejected.starts_with("HTTP/1.1 401"), "{rejected}");
    }

    let accepted = websocket_handshake(
        address,
        &format!("{BOOKS_PROTOCOL}, {VALID_AUTH_PROTOCOL}"),
        Some(PUBLIC_IP),
    )
    .await?;
    assert!(accepted.starts_with("HTTP/1.1 101"), "{accepted}");
    assert_eq!(
        response_header(&accepted, "Sec-WebSocket-Protocol"),
        Some(BOOKS_PROTOCOL)
    );
    assert!(!accepted.contains(VALID_AUTH_PROTOCOL));

    server.abort();
    Ok(())
}
