use crate::domain::models::VideoDetails;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use cap_fs_ext::{
    FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsMaybeDirExt,
};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::{fs, io::AsyncWriteExt};

use crate::domain::traits::{FileStore, Filer, StoreObject};

#[derive(Clone)]
pub struct FileSystemStore {
    root: String,
    root_dir: Arc<OnceLock<Dir>>,
}

impl FileSystemStore {
    pub fn new(root: &str) -> Self {
        let store = Self {
            root: root.to_string(),
            root_dir: Arc::new(OnceLock::new()),
        };
        let _ = store.open_root();
        store
    }

    fn get_real_path(&self, path: &str) -> Result<PathBuf, anyhow::Error> {
        let requested = Path::new(path);
        let root = Path::new(&self.root);
        let resolved = if requested.is_absolute() || requested.starts_with(root) {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };

        // Normalize path components to prevent traversal (e.g. "../../etc/passwd")
        let mut normalized = PathBuf::new();
        for component in resolved.components() {
            match component {
                std::path::Component::ParentDir => { normalized.pop(); }
                _ => normalized.push(component),
            }
        }

        let root_normalized = {
            let mut r = PathBuf::new();
            for component in Path::new(&self.root).components() {
                match component {
                    std::path::Component::ParentDir => { r.pop(); }
                    _ => r.push(component),
                }
            }
            r
        };

        if !normalized.starts_with(&root_normalized) {
            return Err(anyhow!("path escapes root directory: {}", path));
        }

        Ok(normalized)
    }

    fn open_root(&self) -> Result<&Dir> {
        if self.root_dir.get().is_none() {
            let root_path = self.normalized_root()?;
            let anchor = root_path.ancestors().last().ok_or_else(|| {
                anyhow!("filesystem root has no volume anchor: {}", root_path.display())
            })?;
            let relative_root = root_path.strip_prefix(anchor)?;
            let mut pending = Vec::new();
            append_relative_components(&mut pending, relative_root)?;
            let mut pending = VecDeque::from(pending);
            let mut resolved = Vec::new();
            let mut symlink_hops = 0;
            let mut root = Dir::open_ambient_dir(anchor, ambient_authority())?;
            while let Some(component) = pending.pop_front() {
                let component = Path::new(&component);
                match root.create_dir(component) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                let expected = root.symlink_metadata(component)?;
                if expected.file_type().is_symlink() {
                    let target = root.read_link(component)?;
                    let observed = root.symlink_metadata(component)?;
                    if !observed.file_type().is_symlink()
                        || observed.dev() != expected.dev()
                        || observed.ino() != expected.ino()
                    {
                        return Err(anyhow!(
                            "filesystem root symlink changed while it was read: {}",
                            component.display()
                        ));
                    }
                    symlink_hops += 1;
                    if symlink_hops > 40 {
                        return Err(anyhow!(
                            "too many symlinks while opening filesystem root: {}",
                            root_path.display()
                        ));
                    }

                    let mut redirected = if target.is_absolute() {
                        let relative_target = target.strip_prefix(anchor).map_err(|_| {
                            anyhow!(
                                "filesystem root symlink changes volume: {}",
                                target.display()
                            )
                        })?;
                        let mut components = Vec::new();
                        append_relative_components(&mut components, relative_target)?;
                        components
                    } else {
                        let mut components = resolved.clone();
                        append_relative_components(&mut components, &target)?;
                        components
                    };
                    redirected.extend(pending);
                    pending = VecDeque::from(redirected);
                    resolved.clear();
                    root = Dir::open_ambient_dir(anchor, ambient_authority())?;
                    continue;
                }
                if !expected.is_dir() {
                    return Err(anyhow!(
                        "filesystem root component is not a directory: {}",
                        component.display()
                    ));
                }
                let mut options = OpenOptions::new();
                options
                    .read(true)
                    .maybe_dir(true)
                    .follow(FollowSymlinks::Yes);
                let child = root.open_with(component, &options)?;
                let opened = child.metadata()?;
                if !opened.is_dir()
                    || opened.dev() != expected.dev()
                    || opened.ino() != expected.ino()
                {
                    return Err(anyhow!(
                        "filesystem root component changed while it was opened: {}",
                        component.display()
                    ));
                }
                root = Dir::from_std_file(child.into_std());
                resolved.push(component.as_os_str().to_os_string());
            }
            let _ = self.root_dir.set(root);
        }
        self.root_dir
            .get()
            .ok_or_else(|| anyhow!("failed to retain filesystem root capability"))
    }

    fn normalized_root(&self) -> Result<PathBuf> {
        Ok(std::path::absolute(self.get_real_path("")?)?)
    }

    fn rooted_relative_path(&self, path: &Path) -> Result<PathBuf> {
        let path = path.to_str().ok_or_else(|| {
            anyhow!("filesystem path is not valid UTF-8: {}", path.display())
        })?;
        let resolved = std::path::absolute(self.get_real_path(path)?)?;
        let root = self.normalized_root()?;
        resolved
            .strip_prefix(&root)
            .map(Path::to_path_buf)
            .map_err(|_| anyhow!("path escapes root directory: {}", resolved.display()))
    }

    fn source_path(&self, path: &str) -> Result<PathBuf> {
        let direct = PathBuf::from(path);
        match std::fs::symlink_metadata(&direct) {
            Ok(_) => Ok(std::path::absolute(direct)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(std::path::absolute(self.get_real_path(path)?)?)
            }
            Err(error) => Err(error.into()),
        }
    }
}

fn append_relative_components(components: &mut Vec<OsString>, path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if components.pop().is_none() {
                    return Err(anyhow!(
                        "filesystem root symlink escapes its volume: {}",
                        path.display()
                    ));
                }
            }
            Component::Normal(component) => components.push(component.to_os_string()),
            Component::Prefix(_) | Component::RootDir => {
                return Err(anyhow!(
                    "expected a relative filesystem root component: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn is_cross_device(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::CrossesDevices
}

fn unique_staging_name(prefix: &str) -> String {
    format!(".{prefix}-{:032x}", rand::random::<u128>())
}

fn ensure_cap_regular_file(dir: &Dir, path: &Path, description: &str) -> Result<()> {
    let metadata = dir.symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!(
            "{description} must be a regular file and not a symlink: {}",
            path.display()
        ));
    }
    Ok(())
}

fn restore_staged_file(dir: &Dir, staged: &Path, original: &Path) -> Result<()> {
    ensure_cap_regular_file(dir, staged, "staged file")?;
    dir.hard_link(staged, dir, original).map_err(|error| {
        anyhow!(
            "failed to restore {} to {} without replacing an existing file: {error}",
            staged.display(),
            original.display()
        )
    })?;
    if let Err(error) = dir.remove_file(staged) {
        tracing::warn!(
            "Restored staged file to {} but could not remove {}: {}",
            original.display(),
            staged.display(),
            error
        );
    }
    Ok(())
}

fn copy_staged_file(
    source_dir: &Dir,
    staged_source: &Path,
    destination_dir: &Dir,
    destination: &Path,
) -> Result<()> {
    let mut source_options = OpenOptions::new();
    source_options.read(true).follow(FollowSymlinks::No);
    let mut source = source_dir.open_with(staged_source, &source_options)?;
    let source_metadata = source.metadata()?;
    if !source_metadata.is_file() {
        return Err(anyhow!("source must be a regular file"));
    }

    let destination_parent = destination.parent().unwrap_or_else(|| Path::new(""));
    let temporary = destination_parent.join(unique_staging_name("tvserver-copy"));
    let mut destination_options = OpenOptions::new();
    destination_options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut copied = destination_dir.open_with(&temporary, &destination_options)?;

    let result = (|| -> Result<()> {
        io::copy(&mut source, &mut copied)?;
        destination_dir.set_permissions(&temporary, source_metadata.permissions())?;
        copied.sync_all()?;
        drop(copied);
        drop(source);
        destination_dir.rename(&temporary, destination_dir, destination)?;
        if let Err(error) = source_dir.remove_file(staged_source) {
            tracing::warn!(
                "Published cross-device copy at {} but could not remove staged source {}: {}",
                destination.display(),
                staged_source.display(),
                error
            );
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = destination_dir.remove_file(&temporary);
    }
    result
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
    async fn create_folder(&self, path: &Path) -> Result<()> {
        let store = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let relative = store.rooted_relative_path(&path)?;
            store.open_root()?.create_dir_all(relative)?;
            Result::<()>::Ok(())
        })
        .await??;
        Ok(())
    }

    async fn list_folder(&self, _path: &str) -> Result<(Vec<String>, Vec<String>)> {
        let path = self.get_real_path(_path)?;
        let mut directories: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();

        if !path.is_dir() {
            return Err(anyhow!("{} is not a directory", path.to_string_lossy()));
        }

        let mut read_dir = fs::read_dir(path).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                if entry.path().is_dir() {
                    /*if !_path.is_empty() {
                        name = format!("{}/{}", _path, name);
                    }*/
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
        let path = self.get_real_path(_path)?;
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
        let store = self.clone();
        let old_path = _old_path.to_string();
        let new_path = PathBuf::from(_new_path);
        tokio::task::spawn_blocking(move || {
            let old_path = store.source_path(&old_path)?;
            let destination = store.rooted_relative_path(&new_path)?;
            let destination_dir = store.open_root()?;
            let root_path = store.normalized_root()?;
            let internal_source = old_path.strip_prefix(&root_path).ok().map(Path::to_path_buf);
            let ambient_source;
            let (source_dir, source_path) = if let Some(relative) = internal_source {
                (destination_dir, relative)
            } else {
                let old_parent = old_path.parent().ok_or_else(|| {
                    anyhow!("source path has no parent: {}", old_path.display())
                })?;
                let old_name = old_path.file_name().ok_or_else(|| {
                    anyhow!("source path has no file name: {}", old_path.display())
                })?;
                ambient_source = Dir::open_ambient_dir(old_parent, ambient_authority())?;
                (&ambient_source, PathBuf::from(old_name))
            };
            ensure_cap_regular_file(source_dir, &source_path, "source file")?;

            let staged_source = PathBuf::from(unique_staging_name("tvserver-move"));
            source_dir.rename(&source_path, source_dir, &staged_source)?;

            let move_result = (|| -> Result<()> {
                ensure_cap_regular_file(source_dir, &staged_source, "source file")?;
                match source_dir.rename(&staged_source, destination_dir, &destination) {
                    Ok(()) => ensure_cap_regular_file(
                        destination_dir,
                        &destination,
                        "destination file",
                    ),
                    Err(error) if is_cross_device(&error) => copy_staged_file(
                        source_dir,
                        &staged_source,
                        destination_dir,
                        &destination,
                    ),
                    Err(error) => Err(error.into()),
                }
            })();

            if let Err(error) = move_result {
                if source_dir.symlink_metadata(&staged_source).is_ok() {
                    restore_staged_file(source_dir, &staged_source, &source_path).map_err(
                        |restore_error| {
                            anyhow!(
                                "{error}; additionally failed to restore source {}: {restore_error}",
                                old_path.display()
                            )
                        },
                    )?;
                }
                return Err(error);
            }
            Ok(())
        })
        .await?
    }

    async fn restore(&self, staged_path: &str, original_path: &str) -> Result<()> {
        let store = self.clone();
        let staged_path = PathBuf::from(staged_path);
        let original_path = PathBuf::from(original_path);
        tokio::task::spawn_blocking(move || {
            let staged_path = store.rooted_relative_path(&staged_path)?;
            let original_path = store.rooted_relative_path(&original_path)?;
            restore_staged_file(store.open_root()?, &staged_path, &original_path)
        })
        .await?
    }

    async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
        let obj = FileStoreObject::new(&self.get_real_path(path)?);
        Ok(Arc::new(obj))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let store = self.clone();
        let path = PathBuf::from(path);
        tokio::task::spawn_blocking(move || {
            let relative = store.rooted_relative_path(&path)?;
            let root = store.open_root()?;
            match root.symlink_metadata(&relative) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
                    anyhow!("file must be a regular file and not a symlink: {}", path.display()),
                ),
                Ok(_) => {
                    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
                    let staged = parent.join(unique_staging_name("tvserver-delete"));
                    root.rename(&relative, root, &staged)?;
                    if let Err(error) = ensure_cap_regular_file(root, &staged, "staged file") {
                        return Err(anyhow!(
                            "{error}; unsafe replacement was quarantined at {}",
                            staged.display()
                        ));
                    }
                    Ok(root.remove_file(staged)?)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
        .await?
    }

    async fn remove_empty_dir(&self, path: &Path) -> Result<()> {
        fs::remove_dir(path).await?;
        Ok(())
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

    #[test]
    fn test_path_traversal_rejected() {
        let store = FileSystemStore::new(TEST_DIR);
        assert!(store.get_real_path("../../etc/passwd").is_err());
        assert!(store.get_real_path("subdir/../../../etc/shadow").is_err());
    }

    #[test]
    fn test_valid_paths_accepted() {
        let store = FileSystemStore::new(TEST_DIR);
        assert!(store.get_real_path("collection1/file.mp4").is_ok());
        assert!(store.get_real_path("").is_ok());
        // Relative parent within root should be fine
        assert!(store.get_real_path("collection1/../collection2").is_ok());
    }

    #[tokio::test]
    async fn test_delete_path_traversal() {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);
        let result = store.delete("../../etc/passwd").await;
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_rejects_dangling_symlink_without_removing_it() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-delete-link-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        fs::create_dir_all(&root).await.unwrap();
        let link = root.join("Dune.epub");
        symlink(base.join("missing.epub"), &link).unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store.delete(link.to_str().unwrap()).await;
        let link_exists = link.symlink_metadata().is_ok();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(link_exists);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rename_does_not_follow_destination_parent_symlink() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-rename-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        symlink(&outside, root.join("Fiction")).unwrap();
        let source = base.join("Dune.epub");
        fs::write(&source, b"book").await.unwrap();
        let destination = root.join("Fiction/Dune.epub");
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store
            .rename(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;
        let source_exists = source.exists();
        let outside_exists = outside.join("Dune.epub").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(source_exists);
        assert!(!outside_exists);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rename_stays_anchored_when_store_root_is_replaced() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-root-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let original_root = base.join("original-root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        fs::rename(&root, &original_root).await.unwrap();
        symlink(&outside, &root).unwrap();
        let source = base.join("Dune.epub");
        fs::write(&source, b"book").await.unwrap();
        let destination = root.join("Dune.epub");

        let result = store
            .rename(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;
        let outside_exists = outside.join("Dune.epub").exists();
        let preserved = result.is_err() && source.exists()
            || result.is_ok() && original_root.join("Dune.epub").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(!outside_exists);
        assert!(preserved);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rename_uses_root_capability_for_internal_source() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-source-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let original_collection = base.join("original-fiction");
        let outside = base.join("outside");
        fs::create_dir_all(root.join("Fiction")).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        fs::write(root.join("Fiction/Dune.epub"), b"inside")
            .await
            .unwrap();
        fs::write(outside.join("Dune.epub"), b"outside")
            .await
            .unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        fs::rename(root.join("Fiction"), &original_collection)
            .await
            .unwrap();
        symlink(&outside, root.join("Fiction")).unwrap();
        let source = root.join("Fiction/Dune.epub");
        let destination = root.join("quarantine.epub");

        let result = store
            .rename(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;
        let outside_contents = fs::read(outside.join("Dune.epub")).await.ok();
        let quarantined_contents = fs::read(&destination).await.ok();
        let original_contents = fs::read(original_collection.join("Dune.epub")).await.ok();
        let _ = fs::remove_dir_all(&base).await;

        assert_eq!(outside_contents.as_deref(), Some(b"outside".as_slice()));
        assert!(
            result.is_err() && original_contents.as_deref() == Some(b"inside".as_slice())
                || result.is_ok()
                    && quarantined_contents.as_deref() == Some(b"inside".as_slice())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn create_folder_does_not_follow_parent_symlink() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-create-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        symlink(&outside, root.join("Fiction")).unwrap();
        let destination = root.join("Fiction/Classics");
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store.create_folder(&destination).await;
        let outside_exists = outside.join("Classics").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(!outside_exists);
    }

    #[tokio::test]
    async fn create_folder_creates_missing_store_root() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-create-root-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let destination = root.join("Fiction");
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store.create_folder(&destination).await;
        let destination_exists = destination.is_dir();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_ok(), "{result:?}");
        assert!(destination_exists);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn create_folder_rejects_symlink_installed_at_missing_root() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-missing-root-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&outside).await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        if root.exists() {
            fs::remove_dir(&root).await.unwrap();
        }
        symlink(&outside, &root).unwrap();
        let destination = root.join("Fiction");

        let result = store.create_folder(&destination).await;
        let outside_exists = outside.join("Fiction").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(!outside_exists);
    }

    #[tokio::test]
    async fn create_folder_accepts_normalized_store_root() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-normalized-root-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(base.join("parent")).await.unwrap();
        let configured_root = base.join("parent/../root");
        let destination = base.join("root/Fiction");
        let store = FileSystemStore::new(configured_root.to_str().unwrap());

        let result = store.create_folder(&destination).await;
        let destination_exists = destination.is_dir();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_ok(), "{result:?}");
        assert!(destination_exists);
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_copy_replaces_destination_symlink_without_following_it() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-copy-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir(&destination_root).unwrap();
        std::fs::write(source_root.join("staged.epub"), b"book").unwrap();
        let outside = base.join("outside.epub");
        std::fs::write(&outside, b"outside").unwrap();
        symlink(&outside, destination_root.join("Dune.epub")).unwrap();
        let source_dir = Dir::open_ambient_dir(&source_root, ambient_authority()).unwrap();
        let destination_dir =
            Dir::open_ambient_dir(&destination_root, ambient_authority()).unwrap();

        let result = copy_staged_file(
            &source_dir,
            Path::new("staged.epub"),
            &destination_dir,
            Path::new("Dune.epub"),
        );
        let outside_contents = std::fs::read(&outside).unwrap();
        let destination_contents = std::fs::read(destination_root.join("Dune.epub")).unwrap();
        let source_exists = source_root.join("staged.epub").exists();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_ok());
        assert_eq!(outside_contents, b"outside");
        assert_eq!(destination_contents, b"book");
        assert!(!source_exists);
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_copy_is_committed_after_destination_publication() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-copy-commit-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir(&destination_root).unwrap();
        std::fs::write(source_root.join("staged.epub"), b"book").unwrap();
        let source_dir = Dir::open_ambient_dir(&source_root, ambient_authority()).unwrap();
        let destination_dir =
            Dir::open_ambient_dir(&destination_root, ambient_authority()).unwrap();
        std::fs::set_permissions(&source_root, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = copy_staged_file(
            &source_dir,
            Path::new("staged.epub"),
            &destination_dir,
            Path::new("Dune.epub"),
        );
        std::fs::set_permissions(&source_root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let destination_contents = std::fs::read(destination_root.join("Dune.epub")).unwrap();
        let source_exists = source_root.join("staged.epub").exists();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_ok());
        assert_eq!(destination_contents, b"book");
        assert!(source_exists);
    }

    #[tokio::test]
    async fn restore_does_not_overwrite_replacement() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-restore-collision-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("staged.epub"), b"original").unwrap();
        std::fs::write(base.join("Dune.epub"), b"replacement").unwrap();
        let store = FileSystemStore::new(base.to_str().unwrap());

        let result = store
            .restore(
                base.join("staged.epub").to_str().unwrap(),
                base.join("Dune.epub").to_str().unwrap(),
            )
            .await;
        let original = std::fs::read(base.join("staged.epub")).unwrap();
        let replacement = std::fs::read(base.join("Dune.epub")).unwrap();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_err());
        assert_eq!(original, b"original");
        assert_eq!(replacement, b"replacement");
    }

    // TODO,rename ensure_path_exists -> ensure_directory_exists and test when there is a file
    // with the same name as the desired directory. Test rename with non-existent destination
    // directory (should be created) and non-existent source, which should fail.
}
