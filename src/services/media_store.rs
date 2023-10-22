//! # MediaStore
//!
//! `MediaStore` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaStorer.
use crate::domain::algorithm::get_collection_and_video_from_path;
use crate::domain::config::get_movie_dir;
use crate::domain::messages::{LocalMessage, LocalMessageSender, MediaEvent, MediaItem};
use async_recursion::async_recursion;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io;

use crate::domain::models::CollectionDetails;
use crate::domain::traits::{FileStorer, MediaStorer, Repository};

#[derive(Clone)]
pub struct MediaStore {
    store: FileStorer,
    sender: LocalMessageSender,
}

impl MediaStore {
    pub fn new(store: FileStorer, sender: LocalMessageSender) -> MediaStore {
        MediaStore { store, sender }
    }

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

        self.store_video_info(destination);

        Ok(())
    }

    fn store_video_info(&self, path: &Path) {
        let event = MediaEvent::new_media(path, None);

        if let Err(e) = self.sender.send(LocalMessage::Media(event)) {
            tracing::error!("could not queue Media event")
        }
    }

    #[async_recursion]
    async fn process_directory(&self, path: PathBuf, repo: Repository) -> io::Result<()> {
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
                self.process_directory(path.clone(), repo.clone()).await?;
            } else if let Some(extension) = path.extension() {
                if extension != "json" {
                    let json_path = path.with_extension("json");
                    if !json_path.exists() {
                        tracing::warn!("queuing meteadata for {:?}", path);
                        self.store_video_info(&path);
                    }
                }
            }
        }

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

    async fn check_video_information(&self, repo: Repository) -> anyhow::Result<()> {
        self.process_directory(PathBuf::from(&get_movie_dir()), repo).await?;

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
    use crate::adaptors::{FileSystemStore, SqlRepository};
    use anyhow::Result;
    use std::sync::Arc;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_video_entry_creation() -> Result<()> {
        let (tx, mut rx1) = broadcast::channel(16);

        let filer: FileStorer = Arc::new(FileSystemStore::new("tests/fixtures/media_dir"));

        let store = MediaStore::new(filer, tx);

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
        let (tx, mut rx1) = broadcast::channel(16);
        let filer: FileStorer = Arc::new(FileSystemStore::new("/Users/chris2/Movies"));
        let store = MediaStore::new(filer, tx);
        let repo: Repository = Arc::new(SqlRepository::new(":memory:").await.unwrap());

        store.check_video_information(repo).await?;

        Ok(())
    }
}
