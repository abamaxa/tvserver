use crate::domain::models::VideoDetails;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::{fs, io::AsyncWriteExt};

use crate::domain::traits::{FileStore, Filer, StoreObject};

pub struct FileSystemStore {
    root: String,
}

impl FileSystemStore {
    pub fn new(root: &str) -> Self {
        Self {
            root: root.to_string(),
        }
    }

    fn get_real_path(&self, path: &str) -> PathBuf {
        let first_char = path.chars().nth(0);
        if first_char == Some('/') || path.starts_with(&self.root) {
            PathBuf::from(path)
        } else {
            Path::new(&self.root).join(path)
        }
    }
}

pub struct FileStoreObject {
    file: PathBuf,
}

impl FileStoreObject {
    pub fn new(file: &Path) -> Self {
        Self {
            file: PathBuf::from(file),
        }
    }

    async fn read_struct_from_json_file<T: DeserializeOwned>(
        file_path: &Path,
    ) -> anyhow::Result<T> {
        // Read the file content
        let file_content = fs::read(file_path).await?;

        // Deserialize the JSON content into the target struct
        let deserialized_struct = serde_json::from_slice(&file_content)?;
        Ok(deserialized_struct)
    }

    async fn write_struct_to_json_file<T: Serialize>(data: &T, file_path: &Path) -> Result<()> {
        // Serialize the struct to a JSON string with indentation.
        let json_string = serde_json::to_string_pretty(data)?;

        // Open the file in write mode or create it if it doesn't exist.
        let mut file = fs::File::create(file_path).await?;

        // Write the JSON string to the file.
        file.write_all(json_string.as_bytes()).await?;

        // Flush and close the file.
        file.flush().await?;

        Ok(())
    }
}

#[async_trait]
impl Filer for FileStoreObject {
    fn is_dir(&self) -> bool {
        self.file.is_dir()
    }

    async fn get_metadata(&self) -> Result<VideoDetails> {
        let data_file = self.file.with_extension("json");
        if !data_file.exists() && self.file.exists() {
            return Ok(VideoDetails{..Default::default()})
        }

        Self::read_struct_from_json_file(&data_file).await
    }

    async fn save_metadata(&self, details: VideoDetails) -> Result<()> {
        let data_file = self.file.with_extension("json");

        Self::write_struct_to_json_file(&details, &data_file).await
    }
}

#[async_trait]
impl FileStore for FileSystemStore {
    async fn create_folder(&self, path: &str) -> Result<()> {
        let dest_dir = self.get_real_path(path);
        if !dest_dir.exists() {
            fs::create_dir_all(&dest_dir).await?;
        }
        Ok(())
    }

    async fn list_folder(&self, _path: &str) -> Result<(Vec<String>, Vec<String>)> {
        let path = self.get_real_path(_path);
        let mut directories: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();

        if !path.is_dir() {
            return Err(anyhow!("{} is not a directory", path.to_string_lossy()));
        }

        let mut read_dir = fs::read_dir(path).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            if let Ok(mut name) = entry.file_name().into_string() {
                if entry.path().is_dir() {
                    if !_path.is_empty() {
                        name = format!("{}/{}", _path, name);
                    }
                    directories.push(name);
                } else {
                    files.push(name);
                }
            }
        }

        directories.sort();
        files.sort();

        Ok((directories, files))
    }

    async fn ensure_path_exists(&self, _path: &str) -> Result<()> {
        let path = self.get_real_path(_path);
        if !path.exists() {
            fs::create_dir_all(path).await?;
        } else if !path.is_dir() {
            return Err(anyhow!(
                "a file already exists with that name: {}",
                path.to_string_lossy()
            ));
        }
        Ok(())
    }

    async fn rename(&self, _old_path: &str, _new_path: &str) -> Result<()> {
        let mut old_path = PathBuf::from(_old_path);
        if !old_path.exists() {
            old_path = self.get_real_path(_old_path);
        }
        let new_path = self.get_real_path(_new_path);

        if let Err(_) = fs::rename(&old_path, &new_path).await {
            fs::copy(&old_path, &new_path).await?;
            fs::remove_file(&old_path).await?;
        }
        Ok(())
    }

    async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
        let obj = FileStoreObject::new(&self.get_real_path(path));
        Ok(Arc::new(obj))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let file_path = self.get_real_path(path);
        if !file_path.exists() {
            return Ok(());
        }

        match fs::remove_file(file_path).await {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow!(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::path::PathBuf;

    const TEST_DIR: &str = "tests/fixtures/media_dir";

    #[tokio::test]
    async fn test_list_directory() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        let results = store.list_folder("").await?;

        assert_eq!(
            results.0,
            vec![".thumbnails", "TV", "collection1", "collection2"]
        );
        assert_eq!(
            results.1,
            vec!["empty.mp4", "test.jpg", "test.mp4", "test.png", "test.py"]
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_list_directory_that_does_not_exists() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        if let Ok(_) = store.list_folder("not here").await {
            panic!("{}", "expected call to fail");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_ensure_directory_exists() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        let mut path = PathBuf::from(TEST_DIR);

        path.push("TV");
        path.push("does not exist");

        if path.exists() {
            fs::remove_dir_all(&path).await?;
        }

        assert!(!path.exists());

        store.ensure_path_exists("TV/does not exist").await?;

        assert!(path.exists());

        fs::remove_dir_all(&path).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_rename() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        let mut path = PathBuf::from(TEST_DIR);

        path.push("collection1");

        let results = store.list_folder("collection1").await?;

        assert!(results.1.len() > 0);

        let existing = results.1.first().unwrap();

        let new_name = String::from("new file name.mp4");

        assert!(!results.1.contains(&new_name));

        let mut source_path = path.clone();
        source_path.push(existing);

        let mut dest_path = path.clone();
        dest_path.push(&new_name);

        store
            .rename(source_path.to_str().unwrap(), dest_path.to_str().unwrap())
            .await?;

        let results = store.list_folder(path.to_str().unwrap()).await?;

        assert!(results.1.contains(&new_name));

        store
            .rename(dest_path.to_str().unwrap(), source_path.to_str().unwrap())
            .await?;

        Ok(())
    }

    // TODO,rename ensure_path_exists -> ensure_directory_exists and test when there is a file
    // with the same name as the desired directory. Test rename with non-existent destination
    // directory (should be created) and non-existent source, which should fail.
}
