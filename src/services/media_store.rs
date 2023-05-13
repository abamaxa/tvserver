//! # MediaStore
//!
//! `MediaStore` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaStorer.
use crate::domain::config::get_thumbnail_dir;
use crate::domain::messages::MediaItem;
use async_recursion::async_recursion;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
use tokio::task::JoinSet;
use tokio::{fs, io};

use crate::domain::models::{CollectionDetails, VideoDetails};
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
            || name.ends_with(".json")
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
        if let Err(err) = store_video_info(thumbnail_dir, PathBuf::from(path)).await {
            tracing::error!("could not store info for video {:?}, error was: {}", path, err);
        }
        Ok(())
    }

    fn get_collection(&self, path: &Path) -> Option<String> {
        match path.strip_prefix(&self.root) {
            Ok(p) => {
                let str_collection = p.ancestors().nth(1)?.to_str()?;
                match str_collection.len() {
                    0 => None,
                    _ => Some(str_collection.to_string()),
                }
            }
            _ => None,
        }
    }

    #[async_recursion]
    async fn process_directory(thumbnail_dir: PathBuf, path: PathBuf) -> io::Result<()> {
        let mut tasks = JoinSet::new();
        let mut read_dir = fs::read_dir(path).await?;

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
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

    async fn get_video_details(&self, file_path: &Path) -> io::Result<VideoDetails> {
        let data_file = file_path.with_extension("json");

        read_struct_from_json_file(&data_file).await
    }

    async fn list_collection(&self, collection: &str) -> io::Result<CollectionDetails> {
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

        Ok(CollectionDetails::from(collection, child_collections, videos))
    }
}

#[async_trait]
impl MediaStorer for MediaStore {
    async fn list(&self, collection: &str) -> io::Result<MediaItem> {
        let full_path = Path::new(&self.root).join(collection);
        if full_path.is_dir() {
            let details = self.list_collection(collection).await?;
            Ok(MediaItem::Collection(details))
        } else {
            let details = self.get_video_details(&full_path).await?;
            Ok(MediaItem::Video(details))
        }
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

    async fn check_video_information(&self) -> io::Result<()> {
        let thumbnail_dir = get_thumbnail_dir(&self.root);

        Self::process_directory(thumbnail_dir, PathBuf::from(&self.root)).await
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

pub async fn read_struct_from_json_file<T: DeserializeOwned>(file_path: &Path) -> io::Result<T> {
    // Read the file content
    let file_content = fs::read(file_path).await?;

    // Deserialize the JSON content into the target struct
    let deserialized_struct = serde_json::from_slice(&file_content)?;
    Ok(deserialized_struct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_video_entry_creation() -> Result<()> {
        let store = MediaStore::from("tests/fixtures/media_dir");

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

    #[test]
    fn test_get_collection() {
        let test_cases = [
            ("/foo/bar/show 1/series 2/episode 1.mp4", Some("show 1/series 2")),
            ("/foo/bar/show 1/episode 1.mp4", Some("show 1")),
            ("/foo/bar/episode 1.mp4", None),
        ];

        let store = MediaStore::from("/foo/bar");

        for (test_case, expected) in test_cases {
            let path = PathBuf::from_str(test_case).unwrap();
            let collection = store.get_collection(&path);
            match expected {
                Some(expected) => {
                    assert!(collection.is_some());
                    assert_eq!(&collection.unwrap(), expected);
                }
                _ => {
                    assert!(collection.is_none());
                }
            };
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_check_video_info() -> Result<()> {
        let store = MediaStore::from("/Users/chris2/Movies");

        store.check_video_information().await?;

        Ok(())
    }
}
