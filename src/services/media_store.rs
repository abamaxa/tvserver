use std::{fs, io,path::{Path, PathBuf}};
use async_trait::async_trait;

use crate::adaptors::subprocess::command;
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

    fn get_new_video_path(&self, path: &Path) -> io::Result<PathBuf> {
        let dest_dir = Path::new(&self.root).join("New");
        if !dest_dir.exists() {
            fs::create_dir_all(&dest_dir)?;
        }

        Ok(dest_dir.join(path.file_name().unwrap_or_default()))
    }
}

#[async_trait]
impl MediaStorer for MediaStore {

    fn list(&self, collection: String) -> Result<VideoEntry, io::Error> {
        let mut child_collections: Vec<String> = Vec::new();
        let mut videos: Vec<String> = Vec::new();
        let dir = Path::new(&self.root).join(&collection);

        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let mut name = entry.file_name().to_str().unwrap().to_string();
                if name.starts_with('.')
                    || name == "TV"
                    || name.ends_with(".py")
                    || name.ends_with(".jpg")
                    || name.ends_with(".png")
                {
                    continue;
                }

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

        child_collections.sort();
        videos.sort();

        Ok(VideoEntry::from(collection, child_collections, videos))
    }

    fn move_file(&self, path: &Path) -> io::Result<()> {
        let new_path = self.get_new_video_path(path)?;

        println!("moving file {} to {}",
                 path.to_str().unwrap_or_default(),
                 new_path.to_str().unwrap_or_default());

        fs::rename(path, new_path)
    }

    fn delete(&self, _path: String) -> io::Result<bool> {
        todo!()
    }

    fn as_path(&self, collection: String, video: String) -> String {
        if collection.is_empty() {
            format!("{}/{}", self.root, video)
        } else {
            format!("{}/{}/{}", self.root, collection, video)
        }
    }

    async fn convert_to_mp4(&self, path: &Path) -> anyhow::Result<bool> {
        let mut new_path = self.get_new_video_path(path)?;

        new_path.set_extension("mp4");

        println!("converting {} to mp4 {}",
                 path.to_str().unwrap_or_default(),
                 new_path.to_str().unwrap_or_default());

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
        dest.to_str().unwrap_or_default()
    ];
    command("ffmpeg", args).await
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let store = MediaStore::from("/Users/chris2/Movies");

        let results = store.list("".to_string()).unwrap();

        println!("children: {:?}", results.child_collections);
        println!("videos: {:?}", results.videos);
    }
}
