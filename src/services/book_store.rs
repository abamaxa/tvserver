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
        let configured_book_root = get_book_dir();
        let thumbnail_root = get_book_thumbnail_dir(&configured_book_root);
        let book_root = PathBuf::from(configured_book_root);
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
        let source_path = full_path.to_str().ok_or_else(|| {
            anyhow::anyhow!("book source path is not valid UTF-8: {}", full_path.display())
        })?;
        ensure_regular_file(full_path, "book source").await?;
        let collection = match suggested_collection {
            Some(collection) => title_case(&collection),
            None => self.collection_from_source(full_path)?,
        };
        let collection = safe_relative_collection(&collection)?;
        let destination_directory = self.book_root.join(collection);
        let destination = destination_directory.join(full_path.file_name().ok_or_else(|| {
            anyhow::anyhow!("book path has no file name: {}", full_path.display())
        })?);
        let destination_path = destination.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "book destination path is not valid UTF-8: {}",
                destination.display()
            )
        })?;
        ensure_path_within_root(
            &destination_directory,
            &self.book_root,
            "book destination directory",
        )
        .await?;
        self.store.create_folder(&destination_directory).await?;

        let comparable_source = absolute_path(full_path, "book source")?;
        let comparable_destination = absolute_path(&destination, "book destination")?;
        if comparable_source == comparable_destination {
            return Ok(full_path.to_path_buf());
        }

        self.store.rename(source_path, destination_path).await?;
        Ok(destination)
    }

    fn collection_from_source(&self, full_path: &Path) -> Result<String> {
        let Some(parent) = full_path.parent() else {
            return Ok(String::new());
        };
        let comparable_parent = absolute_path(parent, "book source parent")?;
        let comparable_root = absolute_path(&self.book_root, "configured book root")?;

        match comparable_parent.strip_prefix(comparable_root) {
            Ok(relative) => relative
                .to_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("book collection path is not valid UTF-8")),
            Err(_) => comparable_parent
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "book source parent is not valid UTF-8: {}",
                        comparable_parent.display()
                    )
                }),
        }
    }

    pub async fn delete(&self, checksum: i64) -> Result<()> {
        let book = self.repo.retrieve_book(checksum).await?;
        let collection = safe_relative_collection(&book.collection)?;
        let file_name = safe_file_name(&book.file_name, "book file")?;

        let book_path = self.book_root.join(collection).join(file_name);
        ensure_path_within_root(&book_path, &self.book_root, "book file").await?;
        ensure_regular_file(&book_path, "book file").await?;
        let staged_book_path = staging_path(&book_path, "book-delete")?;
        rename_file(&self.store, &book_path, &staged_book_path).await?;

        let mut staged_thumbnail = None;
        if !is_default_book_thumbnail(&book.thumbnail) {
            let unvalidated_path = self.thumbnail_root.join(&book.thumbnail);
            match safe_file_name(&book.thumbnail, "book thumbnail") {
                Ok(thumbnail) => {
                    let thumbnail_path = self.thumbnail_root.join(thumbnail);
                    let staging_result = async {
                        ensure_path_within_root(
                            &thumbnail_path,
                            &self.thumbnail_root,
                            "book thumbnail",
                        )
                        .await?;
                        ensure_regular_file(&thumbnail_path, "book thumbnail").await?;
                        let staged_path = staging_path(&thumbnail_path, "book-thumbnail-delete")?;
                        rename_file(&self.thumbnail_store, &thumbnail_path, &staged_path).await?;
                        Result::<PathBuf>::Ok(staged_path)
                    }
                    .await;
                    match staging_result {
                        Ok(staged_path) => staged_thumbnail = Some((thumbnail_path, staged_path)),
                        Err(error) => {
                            tracing::warn!(
                                "Failed to stage book thumbnail {} for deletion: {}",
                                thumbnail_path.display(),
                                error
                            );
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        "Failed to delete book thumbnail {}: {}",
                        unvalidated_path.display(),
                        error
                    );
                }
            }
        }

        if let Err(delete_error) = self.repo.delete_book(checksum).await {
            let thumbnail_restore_error =
                if let Some((thumbnail_path, staged_path)) = staged_thumbnail.as_ref() {
                    restore_file(&self.thumbnail_store, staged_path, thumbnail_path)
                        .await
                        .err()
                } else {
                    None
                };
            let book_restore_error = restore_file(&self.store, &staged_book_path, &book_path)
                .await
                .err();

            return match (book_restore_error, thumbnail_restore_error) {
                (None, None) => Err(delete_error.into()),
                (book_restore_error, thumbnail_restore_error) => Err(anyhow::anyhow!(
                    "database deletion failed: {delete_error}; book restore error: {}; thumbnail restore error: {}",
                    book_restore_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    thumbnail_restore_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "none".to_string())
                )),
            };
        }

        delete_file(&self.store, &staged_book_path).await?;
        if let Some((_, staged_path)) = staged_thumbnail {
            if let Err(error) = delete_file(&self.thumbnail_store, &staged_path).await {
                tracing::warn!(
                    "Failed to delete staged book thumbnail {}: {}",
                    staged_path.display(),
                    error
                );
            }
        }
        Ok(())
    }
}

fn staging_path(path: &Path, label: &str) -> Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!("file path has no parent for staging: {}", path.display())
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!("file name is not valid UTF-8 for staging: {}", path.display())
        })?;
    Ok(parent.join(format!(".{file_name}.{label}-{:032x}", rand::random::<u128>())))
}

async fn rename_file(store: &FileStorer, source: &Path, destination: &Path) -> Result<()> {
    let source = source
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("source path is not valid UTF-8: {}", source.display()))?;
    let destination = destination.to_str().ok_or_else(|| {
        anyhow::anyhow!("destination path is not valid UTF-8: {}", destination.display())
    })?;
    store.rename(source, destination).await
}

async fn delete_file(store: &FileStorer, path: &Path) -> Result<()> {
    let path = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("file path is not valid UTF-8: {}", path.display()))?;
    store.delete(path).await
}

async fn restore_file(store: &FileStorer, staged_path: &Path, original_path: &Path) -> Result<()> {
    let staged_path = staged_path.to_str().ok_or_else(|| {
        anyhow::anyhow!("staged path is not valid UTF-8: {}", staged_path.display())
    })?;
    let original_path = original_path.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "original path is not valid UTF-8: {}",
            original_path.display()
        )
    })?;
    store.restore(staged_path, original_path).await
}

async fn ensure_regular_file(path: &Path, description: &str) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        anyhow::anyhow!("failed to inspect {description} ({}): {error}", path.display())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow::anyhow!(
            "{description} must be a regular file and not a symlink: {}",
            path.display()
        ));
    }
    Ok(())
}

fn absolute_path(path: &Path, description: &str) -> Result<PathBuf> {
    std::path::absolute(path).map_err(|error| {
        anyhow::anyhow!(
            "failed to make {description} absolute ({}): {error}",
            path.display()
        )
    })
}

async fn ensure_path_within_root(path: &Path, root: &Path, description: &str) -> Result<()> {
    let resolved_root = resolve_existing_path_prefix(root).await?;
    let resolved_path = resolve_existing_path_prefix(path).await?;
    if resolved_path.starts_with(&resolved_root) {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{description} escapes configured root: {}",
            path.display()
        ))
    }
}

async fn resolve_existing_path_prefix(path: &Path) -> Result<PathBuf> {
    let absolute = absolute_path(path, "path boundary")?;
    let mut current = absolute.clone();
    let mut missing = Vec::new();

    loop {
        match tokio::fs::canonicalize(&current).await {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let component = current.file_name().ok_or_else(|| {
                    anyhow::anyhow!(
                        "failed to resolve an existing path prefix for {}",
                        absolute.display()
                    )
                })?;
                missing.push(component.to_os_string());
                current = current.parent().map(Path::to_path_buf).ok_or_else(|| {
                    anyhow::anyhow!(
                        "failed to resolve an existing path prefix for {}",
                        absolute.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "failed to resolve path boundary for {}: {error}",
                    path.display()
                ));
            }
        }
    }
}

fn safe_file_name(file_name: &str, description: &str) -> Result<PathBuf> {
    let original = Path::new(file_name);
    let mut components = original.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None) => {
            let canonical = PathBuf::from(name);
            if original.as_os_str() == canonical.as_os_str() {
                Ok(canonical)
            } else {
                Err(anyhow::anyhow!(
                    "{description} must already be canonical: {file_name}"
                ))
            }
        }
        _ => Err(anyhow::anyhow!(
            "{description} must be a single file name without traversal: {file_name}"
        )),
    }
}

fn safe_relative_collection(collection: &str) -> Result<PathBuf> {
    let original = Path::new(collection);
    let mut relative = PathBuf::new();
    for component in original.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => {
                return Err(anyhow::anyhow!(
                    "book collection must be a relative path without traversal: {collection}"
                ));
            }
        }
    }
    if original.as_os_str() == relative.as_os_str() {
        Ok(relative)
    } else {
        Err(anyhow::anyhow!(
            "book collection must already be canonical: {collection}"
        ))
    }
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
    async fn add_file_without_derivable_collection_stays_at_book_root() {
        let layout = TestLayout::new("root-fallback");
        let source = layout.book_root.join("Root.epub");
        tokio::fs::write(&source, b"book").await.unwrap();
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

        let destination = store.add_file(&source, None).await.unwrap();

        assert_eq!(destination, source);
        assert_eq!(tokio::fs::read(&source).await.unwrap(), b"book");
        assert!(!layout.book_root.join("books/Root.epub").exists());
    }

    #[tokio::test]
    async fn absolute_source_at_relative_book_root_does_not_duplicate_collection() {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let relative_base = PathBuf::from("target").join(format!(
            "tvserver-book-store-relative-root-{}-{id}",
            std::process::id()
        ));
        let absolute_base = std::env::current_dir().unwrap().join(&relative_base);
        let layout = TestLayout {
            base: absolute_base.clone(),
            source_root: absolute_base.join("downloads"),
            book_root: relative_base.join("books"),
            thumbnail_root: relative_base.join("book-thumbnails"),
            movie_root: absolute_base.join("movies"),
        };
        tokio::fs::create_dir_all(absolute_base.join("books"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(absolute_base.join("book-thumbnails"))
            .await
            .unwrap();
        let source = absolute_base.join("books/Root.epub");
        tokio::fs::write(&source, b"book").await.unwrap();
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

        let destination = store.add_file(&source, None).await.unwrap();

        assert_eq!(destination, source);
        assert_eq!(tokio::fs::read(&source).await.unwrap(), b"book");
        assert!(!absolute_base.join("books/books/Root.epub").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn add_file_rejects_non_utf8_source_before_filesystem_writes() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let layout = TestLayout::new("non-utf8-source");
        let source = layout
            .source_root
            .join(OsString::from_vec(b"Dune-\xff.epub".to_vec()));
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

        let error = store
            .add_file(&source, Some("Fiction".to_string()))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("not valid UTF-8"));
        assert!(!layout.book_root.join("Fiction").exists());
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

    #[cfg(unix)]
    #[tokio::test]
    async fn add_file_rejects_collection_symlink_that_escapes_book_root() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new("add-symlink-escape");
        let outside = layout.base.join("outside-add");
        tokio::fs::create_dir(&outside).await.unwrap();
        symlink(&outside, layout.book_root.join("Fiction")).unwrap();
        let source = layout.source_root.join("Dune.epub");
        tokio::fs::write(&source, b"book").await.unwrap();
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

        let result = store.add_file(&source, Some("Fiction".to_string())).await;

        assert!(result.is_err());
        assert!(source.exists());
        assert!(!outside.join("Dune.epub").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn add_file_rejects_symlink_source() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new("add-symlink-source");
        let target = layout.source_root.join("target.epub");
        tokio::fs::write(&target, b"book").await.unwrap();
        let source = layout.source_root.join("Dune.epub");
        symlink(&target, &source).unwrap();
        let (store, _) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;

        let result = store.add_file(&source, Some("Fiction".to_string())).await;

        assert!(result.is_err());
        assert!(source.symlink_metadata().is_ok());
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"book");
        assert!(!layout.book_root.join("Fiction/Dune.epub").exists());
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

    #[cfg(unix)]
    #[tokio::test]
    async fn thumbnail_symlink_escape_is_warning_only_and_row_deletion_continues() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new("thumbnail-symlink-escape");
        let book_path = layout.book_root.join("Fiction/Dune.epub");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"book").await.unwrap();
        let outside_thumbnail = layout.base.join("outside-cover.jpg");
        tokio::fs::write(&outside_thumbnail, b"keep outside cover")
            .await
            .unwrap();
        let thumbnail_link = layout.thumbnail_root.join("dune-cover.jpg");
        symlink(&outside_thumbnail, &thumbnail_link).unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let mut book = sample_book(32, "Fiction", "Dune.epub");
        book.thumbnail = "dune-cover.jpg".to_string();
        repository.save_book(&book).await.unwrap();

        store.delete(book.checksum).await.unwrap();

        assert!(!book_path.exists());
        assert_eq!(
            tokio::fs::read(&outside_thumbnail).await.unwrap(),
            b"keep outside cover"
        );
        assert!(thumbnail_link.symlink_metadata().is_ok());
        assert!(matches!(
            repository.retrieve_book(book.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }

    #[tokio::test]
    async fn primary_book_delete_failure_preserves_thumbnail_and_repository_row() {
        let layout = TestLayout::new("primary-delete-failure");
        let undeletable_book = layout.book_root.join("Fiction/Dune.epub");
        tokio::fs::create_dir_all(&undeletable_book).await.unwrap();
        let thumbnail_path = layout.thumbnail_root.join("dune-cover.jpg");
        tokio::fs::write(&thumbnail_path, b"keep cover")
            .await
            .unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let mut book = sample_book(15, "Fiction", "Dune.epub");
        book.thumbnail = "dune-cover.jpg".to_string();
        repository.save_book(&book).await.unwrap();

        let result = store.delete(book.checksum).await;

        assert!(result.is_err());
        assert_eq!(tokio::fs::read(&thumbnail_path).await.unwrap(), b"keep cover");
        assert!(repository.retrieve_book(book.checksum).await.is_ok());
    }

    #[tokio::test]
    async fn repository_delete_failure_restores_book_and_thumbnail() {
        let layout = TestLayout::new("repository-delete-failure");
        let book_path = layout.book_root.join("Fiction/Dune.epub");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"book").await.unwrap();
        let thumbnail_path = layout.thumbnail_root.join("dune-cover.jpg");
        tokio::fs::write(&thumbnail_path, b"cover").await.unwrap();

        let database_url = format!("sqlite://{}", layout.base.join("books.sqlite").display());
        let repository: Repository = Arc::new(
            SqlRepository::new(&database_url, None)
                .await
                .expect("repository should initialize"),
        );
        let book_files: FileStorer = Arc::new(FileSystemStore::new(
            layout
                .book_root
                .to_str()
                .expect("book root should be UTF-8"),
        ));
        let thumbnail_files: FileStorer = Arc::new(FileSystemStore::new(
            layout
                .thumbnail_root
                .to_str()
                .expect("thumbnail root should be UTF-8"),
        ));
        let store = BookStore::new_with_roots(
            book_files,
            thumbnail_files,
            repository.clone(),
            &layout.book_root,
            &layout.thumbnail_root,
        );
        let mut book = sample_book(16, "Fiction", "Dune.epub");
        book.thumbnail = "dune-cover.jpg".to_string();
        repository.save_book(&book).await.unwrap();

        let pool = sqlx::SqlitePool::connect(&database_url).await.unwrap();
        sqlx::query(
            "CREATE TRIGGER fail_book_delete BEFORE DELETE ON books \
             BEGIN SELECT RAISE(FAIL, 'forced delete failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = store.delete(book.checksum).await;

        assert!(result.is_err());
        assert_eq!(tokio::fs::read(&book_path).await.unwrap(), b"book");
        assert_eq!(tokio::fs::read(&thumbnail_path).await.unwrap(), b"cover");
        assert!(repository.retrieve_book(book.checksum).await.is_ok());
    }

    #[tokio::test]
    async fn delete_rejects_malformed_book_identity_without_deleting_in_root_alias() {
        let layout = TestLayout::new("delete-book-alias");
        let victim_book = layout.book_root.join("victim.epub");
        tokio::fs::write(&victim_book, b"keep book").await.unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let book = sample_book(13, "../", "../victim.epub");
        repository.save_book(&book).await.unwrap();

        let result = store.delete(book.checksum).await;

        assert!(result.is_err());
        assert_eq!(tokio::fs::read(&victim_book).await.unwrap(), b"keep book");
        assert!(repository.retrieve_book(book.checksum).await.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_rejects_collection_symlink_that_escapes_book_root() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new("delete-symlink-escape");
        let outside = layout.base.join("outside-delete");
        tokio::fs::create_dir(&outside).await.unwrap();
        let victim_book = outside.join("victim.epub");
        tokio::fs::write(&victim_book, b"keep external book")
            .await
            .unwrap();
        symlink(&outside, layout.book_root.join("Fiction")).unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let book = sample_book(31, "Fiction", "victim.epub");
        repository.save_book(&book).await.unwrap();

        let result = store.delete(book.checksum).await;

        assert!(result.is_err());
        assert_eq!(
            tokio::fs::read(&victim_book).await.unwrap(),
            b"keep external book"
        );
        assert!(repository.retrieve_book(book.checksum).await.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_rejects_dangling_book_symlink_and_preserves_row() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new("delete-dangling-book-symlink");
        let collection = layout.book_root.join("Fiction");
        tokio::fs::create_dir(&collection).await.unwrap();
        let book_link = collection.join("Dune.epub");
        symlink(layout.base.join("missing.epub"), &book_link).unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let book = sample_book(33, "Fiction", "Dune.epub");
        repository.save_book(&book).await.unwrap();

        let result = store.delete(book.checksum).await;

        assert!(result.is_err());
        assert!(book_link.symlink_metadata().is_ok());
        assert!(repository.retrieve_book(book.checksum).await.is_ok());
    }

    #[tokio::test]
    async fn delete_rejects_noncanonical_primary_identities_without_deleting_aliases() {
        for (label, checksum, collection, file_name, victim_relative) in [
            (
                "collection-dot",
                21,
                "Fiction/./Classics",
                "victim.epub",
                "Fiction/Classics/victim.epub",
            ),
            (
                "collection-repeat",
                22,
                "Fiction//Classics",
                "victim.epub",
                "Fiction/Classics/victim.epub",
            ),
            (
                "collection-trailing",
                23,
                "Fiction/Classics/",
                "victim.epub",
                "Fiction/Classics/victim.epub",
            ),
            ("file-trailing", 24, "", "victim.epub/", "victim.epub"),
        ] {
            let layout = TestLayout::new(label);
            let victim_book = layout.book_root.join(victim_relative);
            tokio::fs::create_dir_all(victim_book.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(&victim_book, b"keep canonical book")
                .await
                .unwrap();
            let (store, repository) =
                store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
            let book = sample_book(checksum, collection, file_name);
            repository.save_book(&book).await.unwrap();

            let result = store.delete(book.checksum).await;

            assert!(result.is_err(), "{label} should be rejected");
            assert_eq!(
                tokio::fs::read(&victim_book).await.unwrap(),
                b"keep canonical book",
                "{label} deleted its canonical alias"
            );
            assert!(
                repository.retrieve_book(book.checksum).await.is_ok(),
                "{label} removed the malformed row"
            );
        }
    }

    #[tokio::test]
    async fn malformed_thumbnail_cleanup_does_not_prevent_book_and_row_deletion() {
        let layout = TestLayout::new("delete-thumbnail-alias");
        let book_path = layout.book_root.join("Bad.epub");
        tokio::fs::write(&book_path, b"keep malformed book")
            .await
            .unwrap();
        let victim_thumbnail = layout.thumbnail_root.join("victim.jpg");
        tokio::fs::write(&victim_thumbnail, b"keep cover")
            .await
            .unwrap();
        let (store, repository) = store_for_roots(&layout.book_root, &layout.thumbnail_root).await;
        let mut book = sample_book(14, "", "Bad.epub");
        book.thumbnail = "subdir/../victim.jpg".to_string();
        repository.save_book(&book).await.unwrap();

        let result = store.delete(book.checksum).await;

        assert!(result.is_ok());
        assert_eq!(tokio::fs::read(&victim_thumbnail).await.unwrap(), b"keep cover");
        assert!(!book_path.exists());
        assert!(matches!(
            repository.retrieve_book(book.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
    }
}
