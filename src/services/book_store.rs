use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use crate::domain::{
    algorithm::title_case,
    config::{get_book_dir, get_book_thumbnail_dir},
    models::{
        is_default_book_thumbnail, BookCollectionDetails, BookCollectionItem,
        DEFAULT_BOOK_THUMBNAIL,
    },
    traits::{FileStorer, Repository},
};

#[derive(Clone)]
pub struct BookStore {
    store: FileStorer,
    thumbnail_store: FileStorer,
    repo: Repository,
    book_root: PathBuf,
    thumbnail_root: PathBuf,
}

impl BookStore {
    pub fn new(store: FileStorer, thumbnail_store: FileStorer, repo: Repository) -> Self {
        let book_root = PathBuf::from(get_book_dir());
        let thumbnail_root = get_book_thumbnail_dir(book_root.to_str().unwrap_or_default());
        Self::new_with_roots(store, thumbnail_store, repo, book_root, thumbnail_root)
    }

    pub fn new_with_roots(
        store: FileStorer,
        thumbnail_store: FileStorer,
        repo: Repository,
        book_root: impl AsRef<Path>,
        thumbnail_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            store,
            thumbnail_store,
            repo,
            book_root: book_root.as_ref().to_path_buf(),
            thumbnail_root: thumbnail_root.as_ref().to_path_buf(),
        }
    }

    pub async fn list(&self, collection: &str) -> Result<BookCollectionDetails> {
        let child_collections = self
            .repo
            .list_book_collections(collection)
            .await?
            .into_iter()
            .map(|collection| BookCollectionItem {
                collection,
                thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            })
            .collect();
        let books = self.repo.list_books(collection).await?;

        Ok(BookCollectionDetails::new(
            collection.to_string(),
            child_collections,
            books,
        ))
    }

    pub async fn add_file(
        &self,
        full_path: &Path,
        suggested_collection: Option<String>,
    ) -> Result<PathBuf> {
        let collection = suggested_collection
            .map(|collection| title_case(&collection))
            .unwrap_or_else(|| self.collection_from_source(full_path));
        let collection = safe_relative_collection(&collection)?;
        let destination_directory = self.book_root.join(collection);
        self.store.create_folder(&destination_directory).await?;
        let destination = destination_directory.join(full_path.file_name().ok_or_else(|| {
            anyhow::anyhow!("book path has no file name: {}", full_path.display())
        })?);

        if full_path == destination {
            return Ok(destination);
        }

        self.store
            .rename(
                full_path.to_str().unwrap_or_default(),
                destination.to_str().unwrap_or_default(),
            )
            .await?;
        Ok(destination)
    }

    fn collection_from_source(&self, full_path: &Path) -> String {
        let Some(parent) = full_path.parent() else {
            return String::new();
        };

        parent
            .strip_prefix(&self.book_root)
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .or_else(|| parent.file_name().map(Path::new))
            .and_then(Path::to_str)
            .unwrap_or_default()
            .to_string()
    }

    pub async fn delete(&self, checksum: i64) -> Result<()> {
        let book = self.repo.retrieve_book(checksum).await?;

        if !is_default_book_thumbnail(&book.thumbnail) {
            let thumbnail_path = self.thumbnail_root.join(&book.thumbnail);
            if let Err(error) = self
                .thumbnail_store
                .delete(thumbnail_path.to_str().unwrap_or_default())
                .await
            {
                tracing::warn!(
                    "Failed to delete book thumbnail {}: {}",
                    thumbnail_path.display(),
                    error
                );
            }
        }

        let book_path = self.book_root.join(book.get_download_path());
        self.store
            .delete(book_path.to_str().unwrap_or_default())
            .await?;
        self.repo.delete_book(checksum).await?;
        Ok(())
    }
}

fn safe_relative_collection(collection: &str) -> Result<PathBuf> {
    let mut relative = PathBuf::new();
    for component in Path::new(collection).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => {
                return Err(anyhow::anyhow!(
                    "book collection must be a relative path without traversal: {collection}"
                ));
            }
        }
    }
    Ok(relative)
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    };

    use chrono::Local;

    use crate::{
        adaptors::{FileSystemStore, SqlRepository},
        domain::{
            models::{BookDetails, BookFormat, BookState, DEFAULT_BOOK_THUMBNAIL},
            traits::{FileStorer, Repository},
        },
    };

    use super::BookStore;

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestLayout {
        base: PathBuf,
        source_root: PathBuf,
        book_root: PathBuf,
        thumbnail_root: PathBuf,
        movie_root: PathBuf,
    }

    impl TestLayout {
        fn new(label: &str) -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "tvserver-book-store-{label}-{}-{id}",
                std::process::id()
            ));
            let layout = Self {
                source_root: base.join("downloads"),
                book_root: base.join("books"),
                thumbnail_root: base.join("book-thumbnails"),
                movie_root: base.join("movies"),
                base,
            };
            for directory in
                [&layout.source_root, &layout.book_root, &layout.thumbnail_root, &layout.movie_root]
            {
                std::fs::create_dir_all(directory).unwrap();
            }
            layout
        }
    }

    impl Drop for TestLayout {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn sample_book(checksum: i64, collection: &str, file_name: &str) -> BookDetails {
        let now = Local::now().naive_local();
        BookDetails {
            file_name: file_name.to_string(),
            collection: collection.to_string(),
            title: file_name.trim_end_matches(".epub").to_string(),
            format: BookFormat::Epub,
            thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            checksum,
            state: BookState::Ready,
            created_on: now,
            updated_on: now,
            ..BookDetails::default()
        }
    }

    async fn store_for_roots(book_root: &Path, thumbnail_root: &Path) -> (BookStore, Repository) {
        let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let book_files: FileStorer = Arc::new(FileSystemStore::new(
            book_root.to_str().expect("book root should be UTF-8"),
        ));
        let thumbnail_files: FileStorer = Arc::new(FileSystemStore::new(
            thumbnail_root
                .to_str()
                .expect("thumbnail root should be UTF-8"),
        ));
        (
            BookStore::new_with_roots(
                book_files,
                thumbnail_files,
                repository.clone(),
                book_root,
                thumbnail_root,
            ),
            repository,
        )
    }

    #[tokio::test]
    async fn list_returns_child_collections_and_books() {
        let book_root = Path::new("/tmp/tvserver-book-store-list-books");
        let thumbnail_root = Path::new("/tmp/tvserver-book-store-list-thumbnails");
        let (store, repository) = store_for_roots(book_root, thumbnail_root).await;
        repository
            .save_book(&sample_book(1, "Fiction", "Dune.epub"))
            .await
            .unwrap();
        repository
            .save_book(&sample_book(2, "Fiction/Classics", "Emma.epub"))
            .await
            .unwrap();

        let result = store.list("Fiction").await.unwrap();

        assert_eq!(result.collection, "Fiction");
        assert_eq!(result.books.len(), 1);
        assert_eq!(result.books[0].file_name, "Dune.epub");
        assert_eq!(result.child_collections.len(), 1);
        assert_eq!(result.child_collections[0].collection, "Classics");
        assert_eq!(result.child_collections[0].thumbnail, DEFAULT_BOOK_THUMBNAIL);
    }

    #[tokio::test]
    async fn add_file_uses_suggested_collection_under_book_root() {
        let layout = TestLayout::new("suggested-collection");
        let source = layout.source_root.join("Dune.epub");
        tokio::fs::write(&source, b"book").await.unwrap();
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

        let destination = store
            .add_file(&source, Some("science fiction".to_string()))
            .await
            .unwrap();

        assert_eq!(destination, layout.book_root.join("Science Fiction/Dune.epub"));
        assert!(destination.exists());
        assert!(!source.exists());
        assert!(!layout.movie_root.join("Science Fiction/Dune.epub").exists());
    }

    #[tokio::test]
    async fn add_file_derives_collection_from_source_parent() {
        let layout = TestLayout::new("derived-collection");
        let source_directory = layout.source_root.join("Classics");
        tokio::fs::create_dir_all(&source_directory).await.unwrap();
        let source = source_directory.join("Emma.epub");
        tokio::fs::write(&source, b"book").await.unwrap();
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

        let destination = store.add_file(&source, None).await.unwrap();

        assert_eq!(destination, layout.book_root.join("Classics/Emma.epub"));
        assert!(destination.exists());
        assert!(!source.exists());
    }

    #[tokio::test]
    async fn add_file_rejects_collection_path_traversal_without_creating_directories() {
        let layout = TestLayout::new("add-traversal");
        let source = layout.source_root.join("Secrets.epub");
        tokio::fs::write(&source, b"book").await.unwrap();
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let escaped_directory = layout.base.join("escaped");

        let result = store
            .add_file(&source, Some("../escaped".to_string()))
            .await;

        assert!(result.is_err());
        assert!(source.exists());
        assert!(!escaped_directory.exists());
    }

    #[tokio::test]
    async fn delete_removes_book_generated_thumbnail_and_repository_row() {
        let layout = TestLayout::new("delete-generated-thumbnail");
        let book_path = layout.book_root.join("Fiction/Dune.epub");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"book").await.unwrap();
        let thumbnail_path = layout.thumbnail_root.join("dune-cover.jpg");
        tokio::fs::write(&thumbnail_path, b"cover").await.unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let mut book = sample_book(10, "Fiction", "Dune.epub");
        book.thumbnail = "dune-cover.jpg".to_string();
        repository.save_book(&book).await.unwrap();

        store.delete(book.checksum).await.unwrap();

        assert!(!book_path.exists());
        assert!(!thumbnail_path.exists());
        assert!(matches!(
            repository.retrieve_book(book.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }

    #[tokio::test]
    async fn delete_preserves_default_thumbnail() {
        let layout = TestLayout::new("delete-default-thumbnail");
        let book_path = layout.book_root.join("Fiction/Dune.epub");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"book").await.unwrap();
        let default_thumbnail_path = layout.thumbnail_root.join(DEFAULT_BOOK_THUMBNAIL);
        tokio::fs::write(&default_thumbnail_path, b"default cover")
            .await
            .unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let book = sample_book(11, "Fiction", "Dune.epub");
        repository.save_book(&book).await.unwrap();

        store.delete(book.checksum).await.unwrap();

        assert!(!book_path.exists());
        assert!(default_thumbnail_path.exists());
        assert!(matches!(
            repository.retrieve_book(book.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }

    #[tokio::test]
    async fn thumbnail_cleanup_failure_does_not_prevent_book_and_row_deletion() {
        let layout = TestLayout::new("thumbnail-cleanup-failure");
        let book_path = layout.book_root.join("Fiction/Dune.epub");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"book").await.unwrap();
        let undeletable_thumbnail = layout.thumbnail_root.join("cover-directory");
        tokio::fs::create_dir(&undeletable_thumbnail).await.unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let mut book = sample_book(12, "Fiction", "Dune.epub");
        book.thumbnail = "cover-directory".to_string();
        repository.save_book(&book).await.unwrap();

        store.delete(book.checksum).await.unwrap();

        assert!(!book_path.exists());
        assert!(undeletable_thumbnail.exists());
        assert!(matches!(
            repository.retrieve_book(book.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }

    #[tokio::test]
    async fn delete_does_not_follow_persisted_paths_outside_roots() {
        let layout = TestLayout::new("delete-traversal");
        let outside_book = layout.base.join("outside.epub");
        let outside_thumbnail = layout.base.join("outside.jpg");
        tokio::fs::write(&outside_book, b"keep book").await.unwrap();
        tokio::fs::write(&outside_thumbnail, b"keep cover")
            .await
            .unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let mut book = sample_book(13, "../", "../outside.epub");
        book.thumbnail = "../outside.jpg".to_string();
        repository.save_book(&book).await.unwrap();

        store.delete(book.checksum).await.unwrap();

        assert_eq!(tokio::fs::read(&outside_book).await.unwrap(), b"keep book");
        assert_eq!(tokio::fs::read(&outside_thumbnail).await.unwrap(), b"keep cover");
        assert!(matches!(
            repository.retrieve_book(book.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }
}
