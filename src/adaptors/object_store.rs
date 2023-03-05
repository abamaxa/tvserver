use std::path::Path;
use tokio::fs;

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use crate::domain::traits::StoreReaderWriter;

pub struct FileSystemStore{}

#[async_trait]
impl StoreReaderWriter for FileSystemStore {
    async fn list_directory(&self, path: &Path) -> Result<(Vec<String>, Vec<String>)> {
        let mut directories: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();

        if !path.is_dir() {
            return Err(anyhow!("{} is not a directory", path.to_string_lossy()));
        }

        let mut read_dir = fs::read_dir(path).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                if entry.path().is_dir() {
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

    async fn ensure_path_exists(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            fs::create_dir_all(path).await?;
        }
        Ok(())
    }

    async fn rename(&self, old_path: &Path, new_path: &Path) -> Result<()> {
        fs::rename(old_path, new_path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use super::*;

    const TEST_DIR: &str = "tests/fixtures/media_dir";

    #[tokio::test]
    async fn test_list_directory() -> Result<()> {
        let store: &dyn StoreReaderWriter = &FileSystemStore {} ;

        let results = store.list_directory(Path::new(TEST_DIR)).await?;

        assert_eq!(results.0, vec!["TV", "collection1", "collection2"]);
        assert_eq!(results.1, vec!["test.jpg", "test.mp4", "test.png", "test.py"]);

        Ok(())
    }

    #[tokio::test]
    async fn test_list_directory_that_does_not_exists() -> Result<()> {
        let store: &dyn StoreReaderWriter = &FileSystemStore {} ;

        if let Ok(_) = store.list_directory(Path::new("not here")).await {
            panic!("{}", "expected call to fail");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_ensure_directory_exists() -> Result<()> {
        let store: &dyn StoreReaderWriter = &FileSystemStore {} ;
        Ok(())
    }

    #[tokio::test]
    async fn test_rename() -> Result<()> {
        let store: &dyn StoreReaderWriter = &FileSystemStore {} ;
        Ok(())
    }

    #[tokio::test]
    async fn test_rename_fails_with_directory_that_does_not_exist() -> Result<()> {
        let store: &dyn StoreReaderWriter = &FileSystemStore {} ;
        Ok(())
    }
}
