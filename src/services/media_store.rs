use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::{fs, io};

use crate::adaptors::AsyncCommand;
use crate::domain::models::VideoEntry;
use crate::domain::traits::MediaStorer;

#[derive(Clone, Debug)]
pub struct MediaStore {
    root: String,
}

impl MediaStore {
    pub fn from(root: &str) -> MediaStore {
        MediaStore {
            root: root.to_string(),
        }
    }

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
            || name.ends_with(".jpg")
            || name.ends_with(".png")
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

        fs::rename(path, new_path).await
    }

    async fn rename(&self, current: &str, new_path: &str) -> io::Result<()> {
        tracing::debug!("rename file {} to {}", current, new_path);

        fs::rename(self.as_path("", current), self.as_path("", new_path)).await
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

    async fn convert_to_mp4(&self, path: &Path) -> anyhow::Result<bool> {
        // ffmpeg -i 'the lord of the rings the rings of power s01e05.mkv' -c:v libx265 -vtag hvc1 -vprofile main -c:a copy -pix_fmt yuv420p output.mp4

        let mut new_path = self.get_new_video_path(path).await?;

        new_path.set_extension("mp4");

        tracing::debug!(
            "converting {} to mp4 {}",
            path.to_str().unwrap_or_default(),
            new_path.to_str().unwrap_or_default()
        );

        convert_to_mp4(path, &new_path).await
    }
}

async fn convert_to_mp4(src: &Path, dest: &Path) -> anyhow::Result<bool> {
    let args = vec![
        "-i",
        src.to_str().unwrap_or_default(),
        "-c:v",
        "copy",
        "-c:a",
        "copy",
        "-y",
        dest.to_str().unwrap_or_default(),
    ];
    AsyncCommand::command("ffmpeg", args).await
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
