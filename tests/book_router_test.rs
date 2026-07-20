#![cfg(feature = "webserver")]

mod common;

use std::{env, path::PathBuf, sync::Arc};

use anyhow::Result;
use app_lib::{
    adaptors::SqlRepository,
    domain::{
        config::{BOOK_DIR, BOOK_THUMBNAIL_DIR, MOVIE_DIR},
        models::{default_book_thumbnail_bytes, DEFAULT_BOOK_THUMBNAIL},
        traits::{MockMediaStorer, Repository, Storer},
    },
    entrypoints::{
        webserver::build_http_router, BookRuntime, Context, BOOK_LIBRARY_UNAVAILABLE,
    },
};
use reqwest::{
    header::{CONTENT_LENGTH, CONTENT_TYPE},
    Response, StatusCode,
};
use tokio::sync::Mutex;

use crate::common::{
    get_book_services_at, get_checker, get_context_with_book_services, get_media_store,
    get_pirate_search, get_task_manager,
};

const MOVIE_ROOT: &str = "tests/fixtures/media_dir";
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Result<Self> {
        let path =
            env::temp_dir().join(format!("tvserver-book-router-{}-57207", std::process::id()));
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn make_context(book_root: &PathBuf, thumbnail_root: &PathBuf) -> Result<Context> {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    let book_runtime =
        get_book_services_at(repository.clone(), book_root, thumbnail_root).await;
    get_context_with_book_services(
        get_media_store(),
        get_pirate_search("torrents_get.json", "pb_search.html").await,
        get_task_manager(),
        repository,
        get_checker(),
        book_runtime,
    )
    .await
}

async fn make_unavailable_context() -> Result<Context> {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await?);
    let mut media_store = MockMediaStorer::new();
    media_store
        .expect_list()
        .withf(|collection| collection == "root")
        .returning(|_| Err(anyhow::anyhow!("missing test video collection")));
    let media_store: Storer = Arc::new(media_store);
    get_context_with_book_services(
        media_store,
        get_pirate_search("torrents_get.json", "pb_search.html").await,
        get_task_manager(),
        repository,
        get_checker(),
        BookRuntime::Unavailable {
            message: Arc::from(BOOK_LIBRARY_UNAVAILABLE),
        },
    )
    .await
}

#[tokio::test]
async fn unavailable_book_runtime_returns_503_for_book_progress_without_disabling_video_routes(
) -> Result<()> {
    let _env_lock = ENV_LOCK.lock().await;
    env::set_var(MOVIE_DIR, MOVIE_ROOT);

    let server = common::create_server(make_unavailable_context().await?, 57213).await;
    let client = reqwest::Client::builder().no_proxy().build()?;

    for (method, path) in [
        (reqwest::Method::GET, "/api/books"),
        // Axum rejects this path while extracting `Path<String>`. The unavailable
        // runtime response must take precedence over that extractor rejection.
        (reqwest::Method::GET, "/api/books/%FF"),
        (reqwest::Method::GET, "/api/book/1"),
        (reqwest::Method::DELETE, "/api/book/1"),
        (
            reqwest::Method::GET,
            "/api/books/download/Shelf/book.epub",
        ),
        (
            reqwest::Method::GET,
            "/api/book-thumbnails/default-book.jpg",
        ),
    ] {
        let response = client
            .request(method, format!("http://localhost:57213{path}"))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
        let has_json_error = matches!(path, "/api/books" | "/api/books/%FF")
            || path.starts_with("/api/book/");
        if has_json_error {
            assert_eq!(response.headers()[CONTENT_TYPE], "application/json", "{path}");
            let body: app_lib::domain::messages::Response = response.json().await?;
            assert!(body.message.is_empty(), "{path}");
            assert_eq!(body.errors, [BOOK_LIBRARY_UNAVAILABLE], "{path}");
        }
    }

    let save_progress = client
        .put("http://localhost:57213/api/book/1/progress")
        .json(&serde_json::json!({
            "locator": { "type": "pdf-page", "value": "1" }
        }))
        .send()
        .await?;
    assert_eq!(save_progress.status(), StatusCode::SERVICE_UNAVAILABLE);

    for (method, path, expected) in [
        (
            reqwest::Method::GET,
            "/api/book-progress",
            StatusCode::NOT_FOUND,
        ),
        (
            reqwest::Method::GET,
            "/api/book/1/progress",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            reqwest::Method::DELETE,
            "/api/book/1/progress",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ] {
        let response = client
            .request(method, format!("http://localhost:57213{path}"))
            .send()
            .await?;
        assert_eq!(response.status(), expected, "{path}");
    }

    let video = client
        .get("http://localhost:57213/api/media/root")
        .send()
        .await?;
    assert_ne!(video.status(), StatusCode::SERVICE_UNAVAILABLE);

    Ok(server.abort())
}

#[tokio::test]
async fn book_progress_put_uses_ordinary_json_rejection_precedence() -> Result<()> {
    let _env_lock = ENV_LOCK.lock().await;
    env::set_var(MOVIE_DIR, MOVIE_ROOT);

    let server = common::create_server(make_unavailable_context().await?, 57218).await;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let malformed = client
        .put("http://localhost:57218/api/book/1/progress")
        .header(CONTENT_TYPE, "application/json")
        .body(r#"{"locator":{"type":"pdf-page","value":"1"}"#)
        .send()
        .await?;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    let valid = client
        .put("http://localhost:57218/api/book/1/progress")
        .json(&serde_json::json!({
            "locator": { "type": "pdf-page", "value": "1" }
        }))
        .send()
        .await?;
    assert_eq!(valid.status(), StatusCode::SERVICE_UNAVAILABLE);

    Ok(server.abort())
}

#[tokio::test]
async fn runtime_materializes_and_retains_the_default_book_thumbnail_capability() -> Result<()> {
    let _env_lock = ENV_LOCK.lock().await;
    let temp_root = TempRoot::new()?;
    let book_root = temp_root.0.join("books");
    let thumbnail_root = book_root.join(".thumbnails");
    env::set_var(MOVIE_DIR, MOVIE_ROOT);
    env::set_var(BOOK_DIR, &book_root);
    env::remove_var(BOOK_THUMBNAIL_DIR);

    let server =
        common::create_server(make_context(&book_root, &thumbnail_root).await?, 57207).await;
    let default_thumbnail = thumbnail_root.join(DEFAULT_BOOK_THUMBNAIL);
    assert_eq!(
        std::fs::read(&default_thumbnail)?,
        default_book_thumbnail_bytes()
    );

    server.abort();

    let invalid_thumbnail_root = temp_root.0.join("not-a-directory");
    std::fs::write(&invalid_thumbnail_root, b"not a directory")?;
    env::set_var(BOOK_THUMBNAIL_DIR, &invalid_thumbnail_root);
    let server_after_config_change =
        common::create_server(make_context(&book_root, &thumbnail_root).await?, 57208).await;
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get("http://localhost:57208/api/book-thumbnails/default-book.jpg")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await?.as_ref(), default_book_thumbnail_bytes());
    server_after_config_change.abort();

    Ok(())
}

#[tokio::test]
async fn builder_defaults_book_dir_beside_movie_dir() -> Result<()> {
    let _env_lock = ENV_LOCK.lock().await;
    let temp_root = TempRoot::new()?;
    let movie_root = temp_root.0.join("movies");
    let book_root = temp_root.0.join("books");
    let thumbnail_root = book_root.join(".thumbnails");
    std::fs::create_dir_all(&movie_root)?;
    env::set_var(MOVIE_DIR, &movie_root);
    env::remove_var(BOOK_DIR);
    env::remove_var(BOOK_THUMBNAIL_DIR);

    let _router = build_http_router(make_context(&book_root, &thumbnail_root).await?)?;
    assert_eq!(
        std::fs::read(thumbnail_root.join(DEFAULT_BOOK_THUMBNAIL))?,
        default_book_thumbnail_bytes()
    );

    Ok(())
}

async fn assert_rejected_without_leaking(response: Response, secret: &[u8]) -> Result<()> {
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.bytes().await?;
    assert!(!body.windows(secret.len()).any(|window| window == secret));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn book_static_routes_enforce_capability_and_file_type_boundaries() -> Result<()> {
    use std::os::unix::fs::symlink;

    let _env_lock = ENV_LOCK.lock().await;
    let temp_root = TempRoot::new()?;
    let book_root = temp_root.0.join("books");
    let thumbnail_root = book_root.join(".thumbnails");
    let outside_root = temp_root.0.join("outside");
    std::fs::create_dir_all(book_root.join("Valid"))?;
    std::fs::create_dir_all(thumbnail_root.join("nested"))?;
    std::fs::create_dir_all(&outside_root)?;

    let epub_bytes = b"valid nested epub";
    let pdf_bytes = b"valid uppercase pdf";
    let generated_thumbnail = default_book_thumbnail_bytes();
    std::fs::write(book_root.join("Valid/Nested.EPUB"), epub_bytes)?;
    std::fs::write(book_root.join("Valid/Manual.PDF"), pdf_bytes)?;
    std::fs::write(book_root.join("notes.txt"), b"unsupported download")?;
    std::fs::write(thumbnail_root.join("generated-cover.jpg"), generated_thumbnail)?;
    std::fs::write(thumbnail_root.join("nested/cover.jpg"), b"nested thumbnail")?;

    let outside_book_secret = b"outside book secret";
    let outside_thumbnail_secret = b"outside thumbnail secret";
    std::fs::write(outside_root.join("secret.epub"), outside_book_secret)?;
    std::fs::write(outside_root.join("secret.jpg"), outside_thumbnail_secret)?;
    symlink(&outside_root, book_root.join("Escape"))?;
    symlink(
        outside_root.join("secret.jpg"),
        thumbnail_root.join("leak.jpg"),
    )?;

    env::set_var(MOVIE_DIR, MOVIE_ROOT);
    let server = common::create_server(make_context(&book_root, &thumbnail_root).await?, 57210)
        .await;
    let client = reqwest::Client::new();

    let epub_response = client
        .get("http://localhost:57210/api/books/download/Valid/Nested.EPUB")
        .send()
        .await?;
    assert_eq!(epub_response.status(), StatusCode::OK);
    assert_eq!(
        epub_response.headers()[CONTENT_TYPE],
        "application/epub+zip"
    );
    assert_eq!(
        epub_response.headers()[CONTENT_LENGTH],
        epub_bytes.len().to_string()
    );
    assert_eq!(epub_response.bytes().await?.as_ref(), epub_bytes);

    let pdf_response = client
        .get("http://localhost:57210/api/books/download/Valid/Manual.PDF")
        .send()
        .await?;
    assert_eq!(pdf_response.status(), StatusCode::OK);
    assert_eq!(pdf_response.headers()[CONTENT_TYPE], "application/pdf");
    assert_eq!(pdf_response.bytes().await?.as_ref(), pdf_bytes);

    let generated_response = client
        .get("http://localhost:57210/api/book-thumbnails/generated-cover.jpg")
        .send()
        .await?;
    assert_eq!(generated_response.status(), StatusCode::OK);
    assert_eq!(generated_response.headers()[CONTENT_TYPE], "image/jpeg");
    assert_eq!(
        generated_response.bytes().await?.as_ref(),
        generated_thumbnail
    );

    let public_generated_response = client
        .get("http://localhost:57210/api/book-thumbnails/generated-cover.jpg")
        .header("X-Real-IP", "8.8.8.8")
        .send()
        .await?;
    assert_eq!(public_generated_response.status(), StatusCode::OK);

    let default_response = client
        .get("http://localhost:57210/api/book-thumbnails/default-book.jpg")
        .send()
        .await?;
    assert_eq!(default_response.status(), StatusCode::OK);
    assert_eq!(default_response.headers()[CONTENT_TYPE], "image/jpeg");
    assert_eq!(
        default_response.bytes().await?.as_ref(),
        default_book_thumbnail_bytes()
    );

    assert_rejected_without_leaking(
        client
            .get("http://localhost:57210/api/books/download/Escape/secret.epub")
            .send()
            .await?,
        outside_book_secret,
    )
    .await?;
    assert_rejected_without_leaking(
        client
            .get("http://localhost:57210/api/book-thumbnails/leak.jpg")
            .send()
            .await?,
        outside_thumbnail_secret,
    )
    .await?;
    assert_rejected_without_leaking(
        client
            .get("http://localhost:57210/api/books/download/notes.txt")
            .send()
            .await?,
        b"unsupported download",
    )
    .await?;
    assert_rejected_without_leaking(
        client
            .get("http://localhost:57210/api/book-thumbnails/nested/cover.jpg")
            .send()
            .await?,
        b"nested thumbnail",
    )
    .await?;

    Ok(server.abort())
}
