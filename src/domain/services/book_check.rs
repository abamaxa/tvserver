use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use async_recursion::async_recursion;

use crate::domain::{
    algorithm::{classify_media_kind, path_to_collection_id, MediaKind},
    config::get_book_dir,
    messages::{LocalMessage, LocalMessageSender, MediaEvent},
    models::BookState,
    traits::{BookChecker, FileStorer, Repository},
};

#[derive(Clone)]
pub struct BookCheck {
    store: FileStorer,
    repo: Repository,
    sender: LocalMessageSender,
    book_root: PathBuf,
}

impl BookCheck {
    pub fn new(store: FileStorer, repo: Repository, sender: LocalMessageSender) -> Self {
        Self::new_with_root(store, repo, sender, get_book_dir())
    }

    pub fn new_with_root(
        store: FileStorer,
        repo: Repository,
        sender: LocalMessageSender,
        book_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            store,
            repo,
            sender,
            book_root: book_root.as_ref().to_path_buf(),
        }
    }

    async fn scan(&self) -> anyhow::Result<()> {
        let books = self.repo.list_all_books().await?;
        let known_books = books
            .iter()
            .map(|book| ((book.collection.clone(), book.file_name.clone()), book.state))
            .collect();
        let mut disk_books = HashSet::new();
        let mut pending_events = Vec::new();
        self.process_collection(
            PathBuf::new(),
            &known_books,
            &mut disk_books,
            &mut pending_events,
        )
        .await?;

        for book in books {
            if disk_books.contains(&(book.collection.clone(), book.file_name.clone())) {
                continue;
            }
            if let Err(error) = self
                .repo
                .delete_book_if_path_matches(
                    book.checksum,
                    &book.collection,
                    &book.file_name,
                )
                .await
            {
                tracing::error!(
                    "error deleting orphaned book {} ({}): {error}",
                    book.file_name,
                    book.checksum
                );
            }
        }

        for event in pending_events {
            if let Err(error) = self.sender.send(LocalMessage::Media(event)).await {
                tracing::error!("could not queue book Media event: {error}");
            }
        }

        Ok(())
    }

    #[async_recursion]
    async fn process_collection(
        &self,
        relative_collection: PathBuf,
        known_books: &HashMap<(String, String), BookState>,
        disk_books: &mut HashSet<(String, String)>,
        pending_events: &mut Vec<MediaEvent>,
    ) -> anyhow::Result<()> {
        let collection = path_to_collection_id(&relative_collection)
            .ok_or_else(|| anyhow::anyhow!("book collection is not a portable path"))?;
        let (directories, files) = self.store.list_folder(&collection).await?;

        for directory in directories {
            if relative_collection.as_os_str().is_empty() && directory == ".thumbnails" {
                continue;
            }
            self.process_collection(
                relative_collection.join(directory),
                known_books,
                disk_books,
                pending_events,
            )
            .await?;
        }

        for file_name in files {
            if classify_media_kind(&file_name) != MediaKind::Book {
                continue;
            }

            disk_books.insert((collection.clone(), file_name.clone()));

            let should_queue = match known_books.get(&(collection.clone(), file_name.clone())) {
                None => true,
                Some(BookState::NeedMetadata | BookState::MetadataError) => true,
                Some(_) => false,
            };
            if !should_queue {
                continue;
            }

            let path = self.book_root.join(&relative_collection).join(file_name);
            let event = MediaEvent::new_media(&path, None);
            pending_events.push(event);
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl BookChecker for BookCheck {
    async fn check_book_information(&self) -> anyhow::Result<()> {
        self.scan().await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Mutex,
        },
    };

    use crate::{
        adaptors::{FileSystemStore, SqlRepository},
        domain::{
            algorithm::file_integrity::FileSeal,
            messages::{LocalMessage, MediaEvent},
            models::{BookDetails, BookFormat, BookState},
            traits::{
                BookChecker, FileStore, FileStorer, PrivateSnapshot, Repository, StagedFile,
                StoreObject,
            },
        },
    };
    use tokio::{
        sync::mpsc,
        time::{timeout, Duration},
    };

    use super::BookCheck;

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    struct ControlledListStore {
        inner: FileStorer,
        failing_collection: Option<String>,
        relocation: Mutex<Option<(Repository, BookDetails)>>,
    }

    #[async_trait::async_trait]
    impl FileStore for ControlledListStore {
        async fn create_folder(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.create_folder(path).await
        }

        async fn list_folder(&self, path: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
            if self.failing_collection.as_deref() == Some(path) {
                anyhow::bail!("forced nested list failure for {path}");
            }
            let relocation = self.relocation.lock().unwrap().take();
            if let Some((repository, book)) = relocation {
                repository.save_book(&book).await?;
            }
            self.inner.list_folder(path).await
        }

        async fn ensure_path_exists(&self, path: &str) -> anyhow::Result<()> {
            self.inner.ensure_path_exists(path).await
        }

        async fn rename(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename(old_path, new_path).await
        }

        async fn rename_no_replace(&self, old_path: &str, new_path: &str) -> anyhow::Result<()> {
            self.inner.rename_no_replace(old_path, new_path).await
        }

        async fn stage_no_follow(&self, source: &str) -> anyhow::Result<StagedFile> {
            self.inner.stage_no_follow(source).await
        }

        async fn create_private_snapshot(
            &self,
            staged: &StagedFile,
        ) -> anyhow::Result<PrivateSnapshot> {
            self.inner.create_private_snapshot(staged).await
        }

        async fn seal_private_snapshot(
            &self,
            snapshot: &PrivateSnapshot,
        ) -> anyhow::Result<FileSeal> {
            self.inner.seal_private_snapshot(snapshot).await
        }

        async fn publish_private_snapshot_no_replace(
            &self,
            snapshot: &PrivateSnapshot,
            destination: &str,
            expected_seal: &FileSeal,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_private_snapshot_no_replace(snapshot, destination, expected_seal)
                .await
        }

        async fn remove_private_snapshot(
            &self,
            snapshot: &PrivateSnapshot,
        ) -> anyhow::Result<()> {
            self.inner.remove_private_snapshot(snapshot).await
        }

        async fn regular_file_exists_no_follow(&self, path: &Path) -> anyhow::Result<bool> {
            self.inner.regular_file_exists_no_follow(path).await
        }

        async fn remove_regular_no_follow(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_regular_no_follow(path).await
        }

        async fn discard_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.discard_staged(staged).await
        }

        async fn publish_staged_no_replace(
            &self,
            staged: &StagedFile,
            destination: &str,
        ) -> anyhow::Result<()> {
            self.inner
                .publish_staged_no_replace(staged, destination)
                .await
        }

        async fn restore_staged(&self, staged: &StagedFile) -> anyhow::Result<()> {
            self.inner.restore_staged(staged).await
        }

        async fn restore(&self, staged_path: &str, original_path: &str) -> anyhow::Result<()> {
            self.inner.restore(staged_path, original_path).await
        }

        async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
            self.inner.get(path).await
        }

        async fn delete(&self, path: &str) -> anyhow::Result<()> {
            self.inner.delete(path).await
        }

        async fn remove_empty_dir(&self, path: &Path) -> anyhow::Result<()> {
            self.inner.remove_empty_dir(path).await
        }
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "tvserver-book-check-{name}-{}-{}",
                std::process::id(),
                NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn queues_new_book_in_recursive_collection() {
        let root = TestDir::new("new-recursive");
        let book_path = root.path().join("Fiction").join("Sci-Fi").join("Dune.EPUB");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"epub").await.unwrap();

        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, mut receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository, sender, root.path());

        checker.check_book_information().await.unwrap();

        let message = timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("new book should be queued")
            .expect("scanner sender should stay open");
        let LocalMessage::Media(MediaEvent::MediaAvailable(event)) = message else {
            panic!("expected MediaAvailable event");
        };
        assert_eq!(event.full_path, book_path);
        assert_eq!(event.search, None);
    }

    #[tokio::test]
    async fn queues_book_from_non_thumbnail_hidden_collection() {
        let root = TestDir::new("hidden-collection");
        let book_path = root
            .path()
            .join(".archive")
            .join(".thumbnails")
            .join("History.pdf");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"pdf").await.unwrap();

        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, mut receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository, sender, root.path());

        checker.check_book_information().await.unwrap();

        let LocalMessage::Media(MediaEvent::MediaAvailable(event)) = receiver
            .try_recv()
            .expect("book in a non-thumbnail hidden collection should be queued")
        else {
            panic!("expected MediaAvailable event");
        };
        assert_eq!(event.full_path, book_path);
        assert_eq!(event.search, None);
    }

    #[tokio::test]
    async fn skips_unsupported_files_and_generated_thumbnail_directory() {
        let root = TestDir::new("unsupported");
        tokio::fs::create_dir_all(root.path().join(".thumbnails"))
            .await
            .unwrap();
        for relative_path in [
            "book.pdf",
            "book.epub",
            "notes.txt",
            "cover.jpg",
            "movie.mp4",
            ".thumbnails/generated.pdf",
        ] {
            tokio::fs::write(root.path().join(relative_path), b"data")
                .await
                .unwrap();
        }

        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, mut receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository, sender, root.path());

        checker.check_book_information().await.unwrap();

        let mut queued = Vec::new();
        while let Ok(LocalMessage::Media(MediaEvent::MediaAvailable(event))) = receiver.try_recv() {
            queued.push(event.full_path);
        }
        queued.sort();
        assert_eq!(
            queued,
            vec![root.path().join("book.epub"), root.path().join("book.pdf")]
        );
    }

    #[tokio::test]
    async fn queues_only_need_metadata_and_metadata_error_books_for_retry() {
        let root = TestDir::new("retries");
        let collection_dir = root.path().join("Fiction").join("Sci-Fi");
        tokio::fs::create_dir_all(&collection_dir).await.unwrap();

        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        for (checksum, file_name, state) in [
            (101, "need.pdf", BookState::NeedMetadata),
            (102, "error.epub", BookState::MetadataError),
            (103, "ready.pdf", BookState::Ready),
        ] {
            let path = collection_dir.join(file_name);
            tokio::fs::write(&path, b"book").await.unwrap();
            let format = if file_name.ends_with("epub") {
                BookFormat::Epub
            } else {
                BookFormat::Pdf
            };
            let mut book = BookDetails::new(
                file_name.to_string(),
                "Fiction/Sci-Fi".to_string(),
                &path,
                format,
            );
            book.checksum = checksum;
            book.state = state;
            repository.save_book(&book).await.unwrap();
        }

        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, mut receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository, sender, root.path());

        checker.check_book_information().await.unwrap();

        let mut queued = Vec::new();
        while let Ok(LocalMessage::Media(MediaEvent::MediaAvailable(event))) = receiver.try_recv() {
            queued.push(event.full_path);
        }
        queued.sort();
        assert_eq!(
            queued,
            vec![collection_dir.join("error.epub"), collection_dir.join("need.pdf")]
        );
    }

    #[tokio::test]
    async fn deletes_orphaned_book_rows_and_preserves_present_books() {
        let root = TestDir::new("orphans");
        let collection_dir = root.path().join("Reference");
        tokio::fs::create_dir_all(&collection_dir).await.unwrap();
        let present_path = collection_dir.join("present.pdf");
        tokio::fs::write(&present_path, b"book").await.unwrap();

        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        for (checksum, file_name, path) in [
            (201, "present.pdf", present_path.clone()),
            (202, "missing.epub", collection_dir.join("missing.epub")),
        ] {
            let format = if file_name.ends_with("epub") {
                BookFormat::Epub
            } else {
                BookFormat::Pdf
            };
            let mut book =
                BookDetails::new(file_name.to_string(), "Reference".to_string(), &path, format);
            book.checksum = checksum;
            book.state = BookState::Ready;
            repository.save_book(&book).await.unwrap();
        }

        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, _receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository.clone(), sender, root.path());

        checker.check_book_information().await.unwrap();

        let books = repository.list_all_books().await.unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].checksum, 201);
    }

    #[tokio::test]
    async fn nested_list_failure_does_not_delete_orphan_rows() {
        let root = TestDir::new("nested-list-failure");
        let collection_dir = root.path().join("broken");
        tokio::fs::create_dir_all(&collection_dir).await.unwrap();

        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let missing_path = collection_dir.join("missing.pdf");
        let mut orphan = BookDetails::new(
            "missing.pdf".to_string(),
            "broken".to_string(),
            &missing_path,
            BookFormat::Pdf,
        );
        orphan.checksum = 301;
        orphan.state = BookState::Ready;
        repository.save_book(&orphan).await.unwrap();

        let inner: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let storer: FileStorer = Arc::new(ControlledListStore {
            inner,
            failing_collection: Some("broken".to_string()),
            relocation: Mutex::new(None),
        });
        let (sender, _receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository.clone(), sender, root.path());

        let error = checker.check_book_information().await.unwrap_err();

        assert!(error.to_string().contains("forced nested list failure"));
        assert_eq!(repository.retrieve_book(301).await.unwrap().file_name, "missing.pdf");
    }

    #[tokio::test]
    async fn preserves_checksum_relocated_after_scanner_snapshot() {
        let root = TestDir::new("relocation-race");
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let mut original = BookDetails::new(
            "Dune.epub".to_string(),
            "Old".to_string(),
            &root.path().join("Old/Dune.epub"),
            BookFormat::Epub,
        );
        original.checksum = 401;
        original.state = BookState::Ready;
        repository.save_book(&original).await.unwrap();
        let mut relocated = original.clone();
        relocated.collection = "New".to_string();
        relocated.file_name = "Relocated.epub".to_string();

        let inner: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let storer: FileStorer = Arc::new(ControlledListStore {
            inner,
            failing_collection: None,
            relocation: Mutex::new(Some((repository.clone(), relocated))),
        });
        let (sender, _receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository.clone(), sender, root.path());

        checker.check_book_information().await.unwrap();

        let current = repository.retrieve_book(401).await.unwrap();
        assert_eq!(current.collection, "New");
        assert_eq!(current.file_name, "Relocated.epub");
    }

    #[tokio::test]
    async fn publishes_scanner_events_only_after_orphan_reconciliation() {
        let root = TestDir::new("event-order");
        let new_path = root.path().join("new.pdf");
        tokio::fs::write(&new_path, b"book").await.unwrap();
        let (sender, mut receiver) = mpsc::channel(8);
        let repository: Repository =
            Arc::new(SqlRepository::new(":memory:", Some(sender.clone())).await.unwrap());
        let mut orphan = BookDetails::new(
            "missing.epub".to_string(),
            "Missing".to_string(),
            &root.path().join("Missing/missing.epub"),
            BookFormat::Epub,
        );
        orphan.checksum = 402;
        orphan.state = BookState::Ready;
        repository.save_book(&orphan).await.unwrap();
        receiver.try_recv().unwrap();
        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let checker = BookCheck::new_with_root(storer, repository, sender, root.path());

        checker.check_book_information().await.unwrap();

        let LocalMessage::Book(deleted) = receiver.try_recv().expect("delete event first") else {
            panic!("orphan reconciliation must happen before scanner publication");
        };
        assert_eq!(deleted.checksum, "402");
        let LocalMessage::Media(MediaEvent::MediaAvailable(available)) =
            receiver.try_recv().expect("media event second")
        else {
            panic!("expected scanner media event after reconciliation");
        };
        assert_eq!(available.full_path, new_path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn invalid_collection_after_valid_book_fails_without_delete_or_publication() {
        let root = TestDir::new("invalid-collection");
        tokio::fs::create_dir_all(root.path().join("a-good")).await.unwrap();
        tokio::fs::write(root.path().join("a-good/new.pdf"), b"book")
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.path().join("z:invalid"))
            .await
            .unwrap();
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let mut orphan = BookDetails::new(
            "missing.pdf".to_string(),
            "missing".to_string(),
            &root.path().join("missing/missing.pdf"),
            BookFormat::Pdf,
        );
        orphan.checksum = 403;
        orphan.state = BookState::Ready;
        repository.save_book(&orphan).await.unwrap();
        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, mut receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository.clone(), sender, root.path());

        let error = checker.check_book_information().await.unwrap_err();

        assert!(error.to_string().contains("portable path"));
        assert!(receiver.try_recv().is_err());
        assert_eq!(repository.retrieve_book(403).await.unwrap().file_name, "missing.pdf");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn skips_external_directory_symlink() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("external-symlink-root");
        let outside = TestDir::new("external-symlink-target");
        let outside_book = outside.path().join("outside.pdf");
        tokio::fs::write(&outside_book, b"outside").await.unwrap();
        symlink(outside.path(), root.path().join("linked-outside")).unwrap();
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, mut receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository, sender, root.path());

        timeout(Duration::from_secs(1), checker.check_book_information())
            .await
            .expect("scan should complete")
            .unwrap();

        assert!(receiver.try_recv().is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn skips_cyclic_directory_symlink_and_completes() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("cyclic-symlink");
        let nested = root.path().join("nested");
        tokio::fs::create_dir_all(&nested).await.unwrap();
        symlink(root.path(), nested.join("cycle")).unwrap();
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let storer: FileStorer = Arc::new(FileSystemStore::new(root.path().to_str().unwrap()));
        let (sender, mut receiver) = mpsc::channel(8);
        let checker = BookCheck::new_with_root(storer, repository, sender, root.path());

        timeout(Duration::from_secs(1), checker.check_book_information())
            .await
            .expect("cyclic symlink must not recurse forever")
            .unwrap();

        assert!(receiver.try_recv().is_err());
    }
}
