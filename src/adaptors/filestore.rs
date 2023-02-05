use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VideoEntry {
    pub collection: String,
    pub parent_collection: String,
    pub child_collections: Vec<String>,
    pub videos: Vec<String>,
    pub errors: Vec<String>,
}

impl VideoEntry {
    pub fn from(
        collection: String,
        child_collections: Vec<String>,
        videos: Vec<String>,
    ) -> VideoEntry {
        let mut parent_collection = String::new();

        if let Some(_) = collection.find('/') {
            let v: Vec<&str> = collection.rsplitn(2, '/').collect();
            parent_collection = v[1].to_string();
        }

        VideoEntry {
            collection,
            parent_collection,
            child_collections,
            videos,
            ..Default::default()
        }
    }

    pub fn error(error: String) -> VideoEntry {
        VideoEntry {
            errors: vec![error],
            ..Default::default()
        }
    }
}

pub trait VideoStore: Send + Sync {
    fn list(&self, collection: String) -> Result<VideoEntry, io::Error>;
    fn as_path(&self, collection: String, video: String) -> String;
}

#[derive(Debug)]
pub struct FileStore {
    root: String,
}

impl FileStore {
    pub fn from(root: &str) -> FileStore {
        FileStore {
            root: root.to_string(),
        }
    }
}

impl VideoStore for FileStore {
    fn list(&self, collection: String) -> Result<VideoEntry, io::Error> {
        let mut child_collections: Vec<String> = Vec::new();
        let mut videos: Vec<String> = Vec::new();

        let store_path = format!("{}/{}", self.root, collection);
        let dir = Path::new(&store_path);

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

    fn as_path(&self, collection: String, video: String) -> String {
        if collection == "" {
            format!("{}/{}", self.root, video)
        } else {
            format!("{}/{}/{}", self.root, collection, video)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FileStore, VideoStore};

    #[test]
    fn test() {
        let store = FileStore::from("/Users/chris2/Movies");

        let results = store.list("".to_string()).unwrap();

        println!("children: {:?}", results.child_collections);
        println!("videos: {:?}", results.videos);
    }
}
