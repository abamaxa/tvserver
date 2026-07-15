mod common;

use std::{env, path::PathBuf, sync::Arc};

use anyhow::Result;
use app_lib::{
    adaptors::SqlRepository,
    domain::{
        config::{BOOK_DIR, BOOK_THUMBNAIL_DIR, MOVIE_DIR},
        models::{default_book_thumbnail_bytes, DEFAULT_BOOK_THUMBNAIL},
        traits::Repository,
    },
    entrypoints::{webserver::build_http_router, Context},
};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

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
    let (book_store, book_file_storer) =
        get_book_services_at(repository.clone(), book_root, thumbnail_root);
    get_context_with_book_services(
        get_media_store(),
        get_pirate_search("torrents_get.json", "pb_search.html").await,
        get_task_manager(),
        repository,
        get_checker(),
        book_store,
        book_file_storer,
    )
    .await
}

#[tokio::test]
async fn server_startup_materializes_default_book_thumbnail_in_fallback_directory() -> Result<()> {
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
    let failed_server =
        common::create_server(make_context(&book_root, &thumbnail_root).await?, 57208).await;
    let startup_result = timeout(Duration::from_secs(1), failed_server)
        .await
        .expect("router construction should fail before the server starts")?;
    assert!(startup_result.is_err());

    Ok(())
}

#[tokio::test]
async fn builder_requires_book_dir() -> Result<()> {
    let _env_lock = ENV_LOCK.lock().await;
    let temp_root = TempRoot::new()?;
    let book_root = temp_root.0.join("books");
    let thumbnail_root = book_root.join(".thumbnails");
    env::set_var(MOVIE_DIR, MOVIE_ROOT);
    env::remove_var(BOOK_DIR);
    env::remove_var(BOOK_THUMBNAIL_DIR);

    let result = build_http_router(make_context(&book_root, &thumbnail_root).await?);
    assert!(result.is_err());
    assert!(format!("{:#}", result.unwrap_err()).contains(BOOK_DIR));

    Ok(())
}
