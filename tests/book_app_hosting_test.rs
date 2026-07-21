#![cfg(feature = "webserver")]

mod common;

use std::{
    env,
    ffi::OsString,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use app_lib::{
    adaptors::SqlRepository,
    domain::{config::MOVIE_DIR, traits::Repository},
    entrypoints::webserver::build_http_router,
};
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{
        header::{
            AUTHORIZATION, CONTENT_RANGE, CONTENT_TYPE, LOCATION, RANGE, WWW_AUTHENTICATE,
        },
        Request, StatusCode,
    },
    response::Response,
    Router,
};
use reqwest::Url;
use tower::ServiceExt;

use crate::common::{
    get_book_services_at, get_checker, get_context_with_book_services, get_media_store,
    get_pirate_search, get_task_manager,
};

const CLIENT_DIR: &str = "CLIENT_DIR";
const AUTH_CREDENTIALS: &str = "AUTH_CREDENTIALS";
const AUTHORIZATION_VALUE: &str = "Basic dGVzdDpzZWNyZXQ=";
const PUBLIC_IP: &str = "8.8.8.8";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = env::temp_dir().join(format!(
            "tvserver-book-app-hosting-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root)?;
        Ok(Self(root))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    fn set(values: &[(&'static str, &std::path::Path)], strings: &[(&'static str, &str)]) -> Self {
        let originals = values
            .iter()
            .map(|(name, _)| (*name, env::var_os(name)))
            .chain(strings.iter().map(|(name, _)| (*name, env::var_os(name))))
            .collect();
        for (name, value) in values {
            env::set_var(name, value);
        }
        for (name, value) in strings {
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

async fn request(app: &Router, path: &str, authenticated: bool, range: Option<&str>) -> Response {
    let mut builder = Request::builder().uri(path).header("X-Real-IP", PUBLIC_IP);
    if authenticated {
        builder = builder.header(AUTHORIZATION, AUTHORIZATION_VALUE);
    }
    if let Some(range) = range {
        builder = builder.header(RANGE, range);
    }
    let mut request = builder.body(Body::empty()).unwrap();
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        4081,
    )));
    app.clone().oneshot(request).await.unwrap()
}

async fn body_text(response: Response) -> Result<String> {
    Ok(String::from_utf8(
        to_bytes(response.into_body(), usize::MAX).await?.to_vec(),
    )?)
}

#[tokio::test]
async fn hosts_books_app_behind_auth_without_shadowing_video_or_book_downloads() -> Result<()> {
    let temp = TempRoot::new()?;
    let client_root = temp.0.join("client");
    let movie_root = temp.0.join("movies");
    let book_root = temp.0.join("library");
    let thumbnail_root = book_root.join(".thumbnails");
    let book_bytes = b"capability-backed-book";

    std::fs::create_dir_all(client_root.join("newapp/books/assets"))?;
    std::fs::create_dir_all(client_root.join("books/assets"))?;
    std::fs::create_dir_all(book_root.join("Shelf"))?;
    std::fs::create_dir_all(&movie_root)?;
    std::fs::write(client_root.join("newapp/index.html"), "video-index-sentinel")?;
    std::fs::write(
        client_root.join("newapp/books/index.html"),
        "video-fallback-books-sentinel",
    )?;
    std::fs::write(
        client_root.join("newapp/books/assets/books.js"),
        "video-fallback-asset-sentinel",
    )?;
    std::fs::write(client_root.join("books/index.html"), "books-index-sentinel")?;
    std::fs::write(client_root.join("books/assets/books.js"), "books-asset-sentinel")?;
    std::fs::write(book_root.join("Shelf/book.epub"), book_bytes)?;

    let _env = EnvGuard::set(
        &[(CLIENT_DIR, &client_root), (MOVIE_DIR, &movie_root)],
        &[(AUTH_CREDENTIALS, "test:secret")],
    );
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

    let root_challenge = request(&app, "/", false, None).await;
    let books_challenge = request(&app, "/books", false, None).await;
    assert_eq!(root_challenge.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(books_challenge.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        root_challenge.headers().get(WWW_AUTHENTICATE),
        books_challenge.headers().get(WWW_AUTHENTICATE)
    );
    assert_eq!(
        books_challenge.headers()[WWW_AUTHENTICATE],
        "Basic realm=\"tvserver\""
    );
    assert!(books_challenge
        .headers()
        .contains_key("content-security-policy"));

    let books_redirect = request(&app, "/books", true, None).await;
    assert_eq!(books_redirect.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(books_redirect.headers()[LOCATION], "/books/");
    assert!(books_redirect
        .headers()
        .contains_key("content-security-policy"));

    let final_url = Url::parse("http://localhost/books")?
        .join(books_redirect.headers()[LOCATION].to_str()?)?;
    assert_eq!(final_url.path(), "/books/");
    let relative_asset_url = final_url.join("./assets/books.js")?;
    assert_eq!(relative_asset_url.path(), "/books/assets/books.js");

    let books = request(&app, final_url.path(), true, None).await;
    assert_eq!(books.status(), StatusCode::OK);
    assert_eq!(books.headers()[CONTENT_TYPE], "text/html");
    assert!(books.headers().contains_key("content-security-policy"));
    assert_eq!(body_text(books).await?, "books-index-sentinel");

    let asset = request(&app, relative_asset_url.path(), true, None).await;
    assert_eq!(asset.status(), StatusCode::OK);
    assert!(asset.headers()[CONTENT_TYPE]
        .to_str()?
        .contains("javascript"));
    assert_eq!(body_text(asset).await?, "books-asset-sentinel");

    let missing_deep_link = request(&app, "/books/library/missing", true, None).await;
    assert_eq!(missing_deep_link.status(), StatusCode::NOT_FOUND);
    assert_ne!(body_text(missing_deep_link).await?, "video-index-sentinel");

    let video = request(&app, "/", true, None).await;
    assert_eq!(video.status(), StatusCode::OK);
    assert_eq!(body_text(video).await?, "video-index-sentinel");

    let download = request(
        &app,
        "/api/books/download/Shelf/book.epub",
        true,
        Some("bytes=1-4"),
    )
    .await;
    assert_eq!(download.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(download.headers()[CONTENT_TYPE], "application/epub+zip");
    assert_eq!(download.headers()[CONTENT_RANGE], "bytes 1-4/22");
    assert_eq!(
        to_bytes(download.into_body(), usize::MAX).await?.as_ref(),
        &book_bytes[1..=4]
    );

    Ok(())
}
