//! # MediaStore
//!
//! `MediaStore` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaStorer.
use crate::domain::config::get_thumbnail_dir;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::{fs, io};

use crate::domain::models::VideoEntry;
use crate::domain::traits::MediaStorer;
use crate::services::video_information::store_video_info;

#[derive(Clone, Debug)]
pub struct MediaStore {
    root: String,
}

impl From<String> for MediaStore {
    fn from(root: String) -> MediaStore {
        MediaStore { root }
    }
}

impl From<&str> for MediaStore {
    fn from(root: &str) -> MediaStore {
        MediaStore {
            root: root.to_string(),
        }
    }
}

impl MediaStore {
    async fn get_new_video_path(&self, path: &Path) -> io::Result<PathBuf> {
        let dest_dir = Path::new(&self.root).join("New");
        if !dest_dir.exists() {
            fs::create_dir_all(&dest_dir).await?;
        }

        Ok(dest_dir.join(path.file_name().unwrap_or_default()))
    }

    fn skip_file(name: &str) -> bool {
        name.starts_with('.')
            || name == "TV"
            || name.ends_with(".py")
            || name.ends_with(".png")
            || name.ends_with(".jpg")
            || name.starts_with(".")
    }

    async fn rename_or_copy_and_delete(&self, src: &Path, destination: &Path) -> io::Result<()> {
        match fs::rename(src, destination).await {
            Ok(_) => self.store_video_info(destination).await,
            Err(_) => {
                fs::copy(src, destination).await?;
                fs::remove_file(src).await?;
                self.store_video_info(destination).await
            }
        }
    }

    async fn store_video_info(&self, path: &Path) -> io::Result<()> {
        let thumbnail_dir = get_thumbnail_dir(&self.root);
        if let Err(err) = store_video_info(thumbnail_dir, path).await {
            tracing::error!("could not store info for video {:?}, error was: {}", path, err);
        }
        Ok(())
    }
}

#[async_trait]
impl MediaStorer for MediaStore {
    async fn list(&self, collection: &str) -> Result<VideoEntry, io::Error> {
        let mut child_collections: Vec<String> = Vec::new();
        let mut videos: Vec<String> = Vec::new();
        let dir = Path::new(&self.root).join(collection);

        if dir.is_dir() {
            let mut read_dir = fs::read_dir(dir).await?;
            while let Ok(Some(entry)) = read_dir.next_entry().await {
                let mut name = entry.file_name().to_str().unwrap().to_string();
                if !Self::skip_file(&name) {
                    if entry.path().is_dir() {
                        if !collection.is_empty() {
                            name = format!("{}/{}", collection, name);
                        }
                        child_collections.push(name);
                    } else {
                        videos.push(name);
                    }
                }
            }
        }

        child_collections.sort();
        videos.sort();

        Ok(VideoEntry::from(collection, child_collections, videos))
    }

    async fn move_file(&self, path: &Path) -> io::Result<()> {
        let new_path = self.get_new_video_path(path).await?;

        tracing::debug!(
            "move file {} to {}",
            path.to_str().unwrap_or_default(),
            new_path.to_str().unwrap_or_default()
        );

        self.rename_or_copy_and_delete(path, &new_path).await
    }

    async fn rename(&self, current: &str, new_path: &str) -> io::Result<()> {
        tracing::debug!("rename file {} to {}", current, new_path);
        let src = self.as_path("", current);
        let destination = self.as_path("", new_path);

        self.rename_or_copy_and_delete((&src).as_ref(), (&destination).as_ref())
            .await
    }

    async fn delete(&self, path: &str) -> io::Result<bool> {
        let full_path = self.as_path("", path);
        let file_path = Path::new(&full_path);
        if !file_path.exists() {
            return Ok(false);
        }

        match fs::remove_file(file_path).await {
            Ok(()) => Ok(true),
            Err(e) => Err(e),
        }
    }

    fn as_path(&self, collection: &str, video: &str) -> String {
        // generates the path component of a URI to a video
        if collection.is_empty() {
            format!("{}/{}", self.root, video)
        } else {
            format!("{}/{}/{}", self.root, collection, video)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_video_entry_creation() -> Result<()> {
        let store = MediaStore::from("tests/fixtures/media_dir");

        let results = store.list("").await?;

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
}
