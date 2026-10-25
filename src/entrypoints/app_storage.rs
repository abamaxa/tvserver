use std::{fs, io, path::PathBuf};

/// Private Android storage, resolved by Tauri before any backend service starts.
pub(super) struct AppStorage {
    data_dir: PathBuf,
    cache_dir: PathBuf,
    config_dir: PathBuf,
    movie_dir: PathBuf,
    book_dir: PathBuf,
    downloads_dir: PathBuf,
    thumbnail_dir: PathBuf,
    book_thumbnail_dir: PathBuf,
    database_path: PathBuf,
}

impl AppStorage {
    pub(super) fn initialize(data_dir: PathBuf, cache_dir: PathBuf) -> io::Result<Self> {
        let movie_dir = data_dir.join("movies");
        let book_dir = data_dir.join("books");
        let storage = Self {
            config_dir: data_dir.join("config"),
            downloads_dir: movie_dir.join(".downloads"),
            thumbnail_dir: movie_dir.join(".thumbnails"),
            book_thumbnail_dir: book_dir.join(".thumbnails"),
            database_path: data_dir.join("tvserver.sqlite"),
            data_dir,
            cache_dir,
            movie_dir,
            book_dir,
        };

        for directory in [
            &storage.data_dir,
            &storage.cache_dir,
            &storage.config_dir,
            &storage.movie_dir,
            &storage.book_dir,
            &storage.downloads_dir,
            &storage.thumbnail_dir,
            &storage.book_thumbnail_dir,
        ] {
            fs::create_dir_all(directory)?;
        }
        Ok(storage)
    }

    #[cfg(target_os = "android")]
    pub(super) fn configure_environment(&self) {
        // Reuse the backend's configuration contract with Android-owned paths.
        for (name, path) in [
            ("HOME", &self.data_dir),
            ("XDG_DATA_HOME", &self.data_dir),
            ("XDG_CONFIG_HOME", &self.config_dir),
            ("XDG_CACHE_HOME", &self.cache_dir),
            ("TMPDIR", &self.cache_dir),
            ("MOVIE_DIR", &self.movie_dir),
            ("BOOK_DIR", &self.book_dir),
            ("DOWNLOAD_DIR", &self.downloads_dir),
            ("THUMBNAIL_DIR", &self.thumbnail_dir),
            ("BOOK_THUMBNAIL_DIR", &self.book_thumbnail_dir),
        ] {
            std::env::set_var(name, path);
        }
        std::env::set_var("DATABASE_URL", self.database_url());
    }

    pub(super) fn database_url(&self) -> String {
        format!("sqlite://{}", self.database_path.display())
    }

    #[cfg(target_os = "android")]
    pub(super) fn media_directories(&self) -> [&PathBuf; 4] {
        [&self.movie_dir, &self.thumbnail_dir, &self.book_dir, &self.book_thumbnail_dir]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_private_storage_and_preserves_files_on_restart() {
        let root = std::env::temp_dir().join(format!("android-storage-{}", rand::random::<u64>()));
        let data = root.join("app data");
        let cache = root.join("cache");
        let storage = AppStorage::initialize(data.clone(), cache.clone()).unwrap();
        assert!(storage.downloads_dir.is_dir());
        assert!(storage.thumbnail_dir.is_dir());
        assert!(storage.book_thumbnail_dir.is_dir());
        assert!(storage.config_dir.is_dir());
        assert!(cache.is_dir());
        assert_eq!(
            storage.database_url(),
            format!("sqlite://{}", data.join("tvserver.sqlite").display())
        );

        let media = storage.movie_dir.join("keep.mp4");
        fs::write(&media, b"existing media").unwrap();
        fs::write(&storage.database_path, b"existing database").unwrap();
        let reopened = AppStorage::initialize(data, cache).unwrap();
        assert_eq!(fs::read(media).unwrap(), b"existing media");
        assert_eq!(fs::read(reopened.database_path).unwrap(), b"existing database");
        fs::remove_dir_all(root).unwrap();
    }
}
