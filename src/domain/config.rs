use reqwest::Url;
use std::env;
use std::path::PathBuf;

// Environment Variables
const CLIENT_DIR: &str = "CLIENT_DIR";
const DATABASE_URL: &str = "DATABASE_URL";
const DATABASE_MIGRATION_DIR: &str = "DATABASE_MIGRATION_DIR";
const ENABLE_VLC: &str = "ENABLE_VLC";
pub const GOOGLE_KEY: &str = "GOOGLE_KEY";
pub const MOVIE_DIR: &str = "MOVIE_DIR";
pub const BOOK_DIR: &str = "BOOK_DIR";
const DOWNLOAD_DIR: &str = "DOWNLOAD_DIR";
const PIRATE_BAY_PROXY_URL: &str = "PIRATE_BAY_PROXY_URL";
const DELAY_REAPING_TASKS_SECS: &str = "DELAY_REAPING_TASKS_SECS";
const THUMBNAIL_DIR: &str = "THUMBNAIL_DIR";
pub const BOOK_THUMBNAIL_DIR: &str = "BOOK_THUMBNAIL_DIR";
const OPENAI_API_KEY: &str = "OPENAI_API_KEY";
const SHARING_SERVER: &str = "SHARING_SERVER";
const TELEGRAM_TOKEN: &str = "TELEGRAM_TOKEN";
const TELEGRAM_CHAT_ID: &str = "TELEGRAM_CHAT_ID";
const AUTH_CREDENTIALS: &str = "AUTH_CREDENTIALS";
//  Defaults
const DEFAULT_DATABASE_URL: &str = "sqlite::memory:";
const DEFAULT_CLIENT_DIR: &str = "client";
const DEFAULT_PB_URL: &str = "https://apibay.org";
const DEFAULT_DELAY_REAPING_TASKS_SECS: i64 = 60;
const DEFAULT_DOWNLOAD_DIR: &str = ".downloads";

pub fn get_movie_dir() -> String {
    env::var(MOVIE_DIR).expect("MOVIE_DIR environment variable is not set")
}

pub fn get_book_dir() -> String {
    env::var(BOOK_DIR).expect("BOOK_DIR environment variable is not set")
}

pub fn enable_vlc_player() -> bool {
    let enable_vlc = env::var(ENABLE_VLC).unwrap_or_default();
    matches!(enable_vlc.as_str(), "1" | "true")
}

pub fn get_client_path(subdir: &str) -> PathBuf {
    let root_dir = env::var(CLIENT_DIR).unwrap_or(String::from(DEFAULT_CLIENT_DIR));
    PathBuf::from(root_dir.as_str()).join(subdir)
}

pub fn get_database_url() -> String {
    env::var(DATABASE_URL).unwrap_or_else(|_| String::from(DEFAULT_DATABASE_URL))
}

pub fn get_database_migration_dir() -> PathBuf {
    if let Ok(dir) = env::var(DATABASE_MIGRATION_DIR) {
        return PathBuf::from(dir);
    }

    let candidates = [
        PathBuf::from("migrations"),
        PathBuf::from("src-tauri").join("migrations"),
    ];

    for candidate in &candidates {
        if candidate.is_dir() {
            return candidate.clone();
        }
    }

    panic!(
        "Database migrations directory not found. Set DATABASE_MIGRATION_DIR or ensure '{}' or '{}' exists.",
        candidates[0].display(), candidates[1].display()
    );
}

pub fn get_downloads_dir() -> String {
    env::var(DOWNLOAD_DIR).unwrap_or_else(|_| {
        PathBuf::from(get_movie_dir())
            .join(DEFAULT_DOWNLOAD_DIR)
            .to_string_lossy()
            .to_string()
    })
}

pub fn get_google_key() -> String {
    env::var(GOOGLE_KEY).unwrap_or_default()
}

pub fn get_pirate_bay_url() -> Url {
    let url = env::var(PIRATE_BAY_PROXY_URL).unwrap_or(String::from(DEFAULT_PB_URL));
    url.parse().expect("PIRATE_BAY_URL is malformed")
}

pub fn delay_reaping_tasks() -> i64 {
    match env::var(DELAY_REAPING_TASKS_SECS) {
        Ok(delay) => delay
            .parse::<i64>()
            .unwrap_or(DEFAULT_DELAY_REAPING_TASKS_SECS),
        _ => DEFAULT_DELAY_REAPING_TASKS_SECS,
    }
}

pub fn get_thumbnail_dir(movie_dir: &str) -> PathBuf {
    match env::var(THUMBNAIL_DIR) {
        Ok(dir) => PathBuf::from(dir),
        _ => PathBuf::from(movie_dir).join(".thumbnails"),
    }
}

pub fn get_book_thumbnail_dir(book_dir: &str) -> PathBuf {
    match env::var(BOOK_THUMBNAIL_DIR) {
        Ok(dir) => PathBuf::from(dir),
        _ => PathBuf::from(book_dir).join(".thumbnails"),
    }
}

pub fn get_openai_api_key() -> String {
    env::var(OPENAI_API_KEY).unwrap_or_default()
}

pub fn get_sharing_server() -> String {
    env::var(SHARING_SERVER).unwrap_or_default()
}

pub fn get_telegram_token() -> String {
    env::var(TELEGRAM_TOKEN).unwrap_or_default()
}

pub fn get_telegram_chat_id() -> String {
    env::var(TELEGRAM_CHAT_ID).unwrap_or_default()
}

pub fn get_auth_credentials() -> String {
    env::var(AUTH_CREDENTIALS).unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::{get_book_dir, get_book_thumbnail_dir, BOOK_DIR, BOOK_THUMBNAIL_DIR};
    use std::env;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::path::PathBuf;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        originals: Vec<(&'static str, Option<String>)>,
    }

    impl EnvVarGuard {
        fn new(names: &[&'static str]) -> Self {
            Self {
                originals: names
                    .iter()
                    .map(|name| (*name, env::var(name).ok()))
                    .collect(),
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            for (name, value) in self.originals.iter() {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn book_dir_is_required_and_thumbnail_dir_defaults_under_book_dir() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvVarGuard::new(&[BOOK_DIR, BOOK_THUMBNAIL_DIR]);

        env::remove_var(BOOK_DIR);
        env::remove_var(BOOK_THUMBNAIL_DIR);
        assert!(catch_unwind(AssertUnwindSafe(get_book_dir)).is_err());

        env::set_var(BOOK_DIR, "/library/books");
        assert_eq!(get_book_dir(), "/library/books");
        assert_eq!(
            get_book_thumbnail_dir("/library/books"),
            PathBuf::from("/library/books/.thumbnails")
        );

        env::set_var(BOOK_THUMBNAIL_DIR, "/library/book-covers");
        assert_eq!(
            get_book_thumbnail_dir("/library/books"),
            PathBuf::from("/library/book-covers")
        );
    }
}
