use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result};
use cap_std::fs::Dir;

use crate::{
    adaptors::FileSystemStore,
    domain::{
        messages::LocalMessageSender,
        models::ensure_default_book_thumbnail,
        services::{BookCheck, BookPathLeaseCoordinator},
        traits::{BookChecker, BookCheckerHandle, FileStorer, Repository},
    },
    services::BookStore,
};

pub const BOOK_LIBRARY_UNAVAILABLE: &str = "book library unavailable";

#[derive(Clone)]
#[cfg_attr(not(feature = "webserver"), allow(dead_code))]
pub struct BookStaticRoots {
    pub(crate) downloads: Arc<Dir>,
    pub(crate) thumbnails: Arc<Dir>,
}

#[derive(Clone)]
pub struct BookIngestionRuntime {
    pub storer: FileStorer,
    pub leases: BookPathLeaseCoordinator,
}

pub struct AvailableBookRuntime {
    pub store: Arc<BookStore>,
    pub checker: BookCheckerHandle,
    pub ingestion: BookIngestionRuntime,
    pub static_roots: BookStaticRoots,
}

#[derive(Clone)]
pub enum BookRuntime {
    Available(Arc<AvailableBookRuntime>),
    Unavailable { message: Arc<str> },
}

impl BookRuntime {
    pub async fn initialize(
        repository: Repository,
        sender: LocalMessageSender,
        book_root: impl AsRef<Path>,
        thumbnail_root: impl AsRef<Path>,
    ) -> Self {
        match Self::try_initialize(
            repository,
            sender,
            book_root.as_ref().to_path_buf(),
            thumbnail_root.as_ref().to_path_buf(),
        )
        .await
        {
            Ok(runtime) => Self::Available(Arc::new(runtime)),
            Err(error) => {
                tracing::error!(error = ?error, "book runtime initialization failed");
                Self::Unavailable {
                    message: Arc::from(BOOK_LIBRARY_UNAVAILABLE),
                }
            }
        }
    }

    async fn try_initialize(
        repository: Repository,
        sender: LocalMessageSender,
        book_root: PathBuf,
        thumbnail_root: PathBuf,
    ) -> Result<AvailableBookRuntime> {
        tokio::fs::create_dir_all(&book_root)
            .await
            .with_context(|| format!("failed to create book directory {}", book_root.display()))?;
        tokio::fs::create_dir_all(&thumbnail_root)
            .await
            .with_context(|| {
                format!(
                    "failed to create book thumbnail directory {}",
                    thumbnail_root.display()
                )
            })?;

        let thumbnail_root_for_materialization = thumbnail_root.clone();
        tokio::task::spawn_blocking(move || {
            ensure_default_book_thumbnail(&thumbnail_root_for_materialization)
        })
        .await
        .context("default book thumbnail task failed")?
        .with_context(|| {
            format!(
                "failed to materialize default book thumbnail in {}",
                thumbnail_root.display()
            )
        })?;

        let book_root_string = book_root.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "configured book directory is not valid UTF-8: {}",
                book_root.display()
            )
        })?;
        let thumbnail_root_string = thumbnail_root.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "configured book thumbnail directory is not valid UTF-8: {}",
                thumbnail_root.display()
            )
        })?;
        let book_filesystem = FileSystemStore::try_new(book_root_string)
            .with_context(|| format!("failed to open book directory {}", book_root.display()))?;
        let thumbnail_filesystem =
            FileSystemStore::try_new(thumbnail_root_string).with_context(|| {
                format!(
                    "failed to open book thumbnail directory {}",
                    thumbnail_root.display()
                )
            })?;
        let static_roots = BookStaticRoots {
            downloads: book_filesystem.retained_root()?,
            thumbnails: thumbnail_filesystem.retained_root()?,
        };
        let book_storer: FileStorer = Arc::new(book_filesystem);
        let thumbnail_storer: FileStorer = Arc::new(thumbnail_filesystem);
        let leases = BookPathLeaseCoordinator::new();
        let checker: BookCheckerHandle = Arc::new(BookCheck::new_with_root_and_leases(
            book_storer.clone(),
            repository.clone(),
            sender,
            &book_root,
            leases.clone(),
        ));
        let store = Arc::new(BookStore::new_with_roots_and_leases(
            book_storer.clone(),
            thumbnail_storer,
            repository,
            &book_root,
            &thumbnail_root,
            leases.clone(),
        ));

        Ok(AvailableBookRuntime {
            store,
            checker,
            ingestion: BookIngestionRuntime {
                storer: book_storer,
                leases,
            },
            static_roots,
        })
    }

    pub fn available(&self) -> Option<Arc<AvailableBookRuntime>> {
        match self {
            Self::Available(runtime) => Some(runtime.clone()),
            Self::Unavailable { .. } => None,
        }
    }

    pub fn ingestion(&self) -> Option<BookIngestionRuntime> {
        self.available().map(|runtime| runtime.ingestion.clone())
    }

    pub fn static_roots(&self) -> Option<BookStaticRoots> {
        self.available().map(|runtime| runtime.static_roots.clone())
    }

    pub fn unavailable_message(&self) -> Option<&str> {
        match self {
            Self::Available(_) => None,
            Self::Unavailable { message } => Some(message),
        }
    }
}

pub(crate) struct UnavailableBookChecker;

#[async_trait::async_trait]
impl BookChecker for UnavailableBookChecker {
    async fn check_book_information(&self) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn unavailable_book_checker() -> BookCheckerHandle {
    static CHECKER: OnceLock<BookCheckerHandle> = OnceLock::new();
    CHECKER
        .get_or_init(|| Arc::new(UnavailableBookChecker))
        .clone()
}
