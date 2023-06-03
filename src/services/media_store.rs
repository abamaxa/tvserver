//! # MediaStore
//!
//! `MediaStore` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaStorer.
use crate::domain::algorithm::get_collection_and_video_from_path;
use crate::domain::config::{get_movie_dir, get_thumbnail_dir};
use crate::domain::messages::MediaItem;
use async_recursion::async_recursion;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io;
use tokio::task::JoinSet;

use crate::domain::models::CollectionDetails;
use crate::domain::traits::{FileStorer, MediaStorer};
use crate::services::video_information::store_video_info;

#[derive(Clone)]
pub struct MediaStore {
    store: FileStorer,
}

impl From<FileStorer> for MediaStore {
    fn from(store: FileStorer) -> MediaStore {
        MediaStore { store }
    }
}

impl MediaStore {
    async fn get_new_video_path(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let dest_dir = Path::new(&get_movie_dir()).join("New");

        self.store.create_folder("New").await?;

        Ok(dest_dir.join(path.file_name().unwrap_or_default()))
    }

    fn skip_file(name: &str) -> bool {
        name.starts_with('.')
            || name == "TV"
            || name.ends_with(".py")
            || name.ends_with(".json")
            || name.ends_with(".png")
            || name.ends_with(".jpg")
            || name.starts_with(".")
    }

    async fn rename_or_copy_and_delete(
        &self,
        src: &Path,
        destination: &Path,
    ) -> anyhow::Result<()> {
        self.store
            .rename(
                src.as_os_str().to_str().unwrap_or_default(),
                destination.as_os_str().to_str().unwrap_or_default(),
            )
            .await?;

        self.store_video_info(destination).await?;

        Ok(())
    }

    async fn store_video_info(&self, path: &Path) -> io::Result<()> {
        let thumbnail_dir = get_thumbnail_dir(&get_movie_dir());
        if let Err(err) = store_video_info(thumbnail_dir, PathBuf::from(path)).await {
            tracing::error!("could not store info for video {:?}, error was: {}", path, err);
        }
        Ok(())
    }

    #[async_recursion]
    async fn process_directory(thumbnail_dir: PathBuf, path: PathBuf) -> io::Result<()> {
        let mut tasks = JoinSet::new();
        let mut read_dir = fs::read_dir(path).await?;

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();

            if Self::skip_file(
                path.file_name()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default(),
            ) {
                continue;
            }

            if path.is_dir() {
                Self::process_directory(thumbnail_dir.clone(), path.clone()).await?;
            } else if let Some(extension) = path.extension() {
                if extension != "json" {
                    let json_path = path.with_extension("json");
                    if !json_path.exists() {
                        tasks.spawn(store_video_info(thumbnail_dir.clone(), path.clone()));
                    }
                }
            }
        }

        // Wait for all tasks to complete
        while let Some(_) = tasks.join_next().await {}

        Ok(())
    }

    async fn list_collection(&self, collection: &str) -> anyhow::Result<CollectionDetails> {
        let (raw_collections, raw_videos) = self.store.list_folder(collection).await?;

        let collections = raw_collections
            .into_iter()
            .filter(|f| !Self::skip_file(f))
            .collect();

        let videos = raw_videos
            .into_iter()
            .filter(|f| !Self::skip_file(f))
            .collect();

        Ok(CollectionDetails::from(collection, collections, videos))
    }
}

#[async_trait]
impl MediaStorer for MediaStore {
    async fn list(&self, collection: &str) -> anyhow::Result<MediaItem> {
        let item = self.store.get(collection).await?;
        if item.is_dir() {
            let details = self.list_collection(collection).await?;
            Ok(MediaItem::Collection(details))
        } else {
            let details = item.get_metadata().await?;
            Ok(MediaItem::Video(details))
        }
    }

    async fn add_file(&self, path: &Path) -> anyhow::Result<()> {
        let new_path = self.get_new_video_path(path).await?;

        tracing::debug!(
            "move file {} to {}",
            path.to_str().unwrap_or_default(),
            new_path.to_str().unwrap_or_default()
        );

        self.rename_or_copy_and_delete(path, &new_path).await?;

        Ok(())
    }

    async fn rename(&self, current: &str, new_path: &str) -> anyhow::Result<()> {
        tracing::debug!("rename file {} to {}", current, new_path);
        let item = self.store.get(current).await?;

        if !item.is_dir() {
            if let Ok(mut details) = item.get_metadata().await {
                (details.collection, details.video) =
                    get_collection_and_video_from_path(&Path::new(new_path));
                item.save_metadata(details).await?;
            }
        }

        self.store.rename(current, new_path).await
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        self.store.delete(path).await
    }

    async fn check_video_information(&self) -> anyhow::Result<()> {
        let thumbnail_dir = get_thumbnail_dir(&get_movie_dir());

        Self::process_directory(thumbnail_dir, PathBuf::from(&get_movie_dir())).await?;

        Ok(())
    }

    fn as_local_path(&self, collection: &str, video: &str) -> String {
        let root = get_movie_dir();
        // generates the path component of a URI to a video
        if collection.is_empty() {
            format!("{}/{}", root, video)
        } else {
            format!("{}/{}/{}", root, collection, video)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::FileSystemStore;
    use anyhow::Result;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_video_entry_creation() -> Result<()> {
        let filer: FileStorer = Arc::new(FileSystemStore::new("tests/fixtures/media_dir"));
        let store = MediaStore::from(filer);

        let results = store.list_collection("").await?;

        assert_eq!(results.child_collections, vec!["collection1", "collection2"]);
        assert_eq!(results.videos, vec!["empty.mp4", "test.mp4"]);

        Ok(())
    }

    #[test]
    fn test_skip_file() {
        let test_cases = [
            ("TV", true),
            ("TV2", false),
            ("file.py", true),
            ("file.mp4", false),
            ("file.jpg", true),
            ("file.png", true),
        ];

        for (name, expected) in test_cases {
            assert_eq!(MediaStore::skip_file(name), expected);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_check_video_info() -> Result<()> {
        let filer: FileStorer = Arc::new(FileSystemStore::new("/Users/chris2/Movies"));
        let store = MediaStore::from(filer);

        store.check_video_information().await?;

        Ok(())
    }
}
