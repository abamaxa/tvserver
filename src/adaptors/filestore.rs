use std::{fs, io,path::{Path, PathBuf}};
use async_trait::async_trait;

use crate::adaptors::subprocess::command;
use crate::domain::models::VideoEntry;
use crate::domain::traits::VideoStore;

#[derive(Clone, Debug)]
pub struct FileStore {
    root: String,
}

impl FileStore {
    pub fn from(root: &str) -> FileStore {
        FileStore {
            root: root.to_string(),
        }
    }

    fn get_new_video_path(&self, path: &Path) -> io::Result<PathBuf> {
        let dest_dir = Path::new(&self.root).join("New");
        if !dest_dir.exists() {
            fs::create_dir_all(&dest_dir)?;
        }

        Ok(dest_dir.join(path.file_name().unwrap_or_default()))
    }
}

#[async_trait]
impl VideoStore for FileStore {
    fn list(&self, collection: String) -> Result<VideoEntry, io::Error> {
        let mut child_collections: Vec<String> = Vec::new();
        let mut videos: Vec<String> = Vec::new();

        // let store_path = format!("{}/{}", self.root, collection);
        let dir = Path::new(&self.root).join(&collection);

        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let mut name = entry.file_name().to_str().unwrap().to_string();
                if name.starts_with(".")
                    || name == "TV"
                    || name.ends_with(".py")
                    || name.ends_with(".jpg")
                    || name.ends_with(".png")
                {
                    continue;
                }

                if entry.path().is_dir() {
                    if collection != "" {
                        name = format!("{}/{}", collection, name);
                    }
                    child_collections.push(name);
                } else {
                    videos.push(name);
                }
            }
        }

        child_collections.sort();
        videos.sort();

        Ok(VideoEntry::from(collection, child_collections, videos))
    }

    fn move_file(&self, path: &PathBuf) -> io::Result<()> {
        fs::rename(path, self.get_new_video_path(path)?)
    }

    async fn convert_to_mp4(&self, path: &PathBuf) -> anyhow::Result<bool> {
        convert_to_mp4(path, &self.get_new_video_path(path)?).await
    }

    fn delete(&self, _path: String) -> io::Result<bool> {
        todo!()
    }

    fn as_path(&self, collection: String, video: String) -> String {
        if collection == "" {
            format!("{}/{}", self.root, video)
        } else {
            format!("{}/{}/{}", self.root, collection, video)
        }
    }
}

async fn convert_to_mp4(src: &PathBuf, dest: &PathBuf) -> anyhow::Result<bool> {
    let args = vec![
        "-i",
        src.to_str().unwrap_or_default(),
        "-c:v",
        "copy",
        "-c:a",
        "copy",
        "-y",
        dest.to_str().unwrap_or_default()
    ];
    command("ffmpeg", args).await
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let store = FileStore::from("/Users/chris2/Movies");

        let results = store.list("".to_string()).unwrap();

        println!("children: {:?}", results.child_collections);
        println!("videos: {:?}", results.videos);
    }
}
