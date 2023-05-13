use reqwest::Url;
use std::env;
use std::env::VarError;
use std::path::PathBuf;

// Environment Variables
const CLIENT_DIR: &str = "CLIENT_DIR";
const DATABASE_URL: &str = "DATABASE_URL";
const DATABASE_MIGRATION_DIR: &str = "DATABASE_MIGRATION_DIR";
const ENABLE_VLC: &str = "ENABLE_VLC";
pub const GOOGLE_KEY: &str = "GOOGLE_KEY";
const MOVIE_DIR: &str = "MOVIE_DIR";
const TORRENT_DIR: &str = "TORRENT_DIR";
const TRANSMISSION_USER: &str = "TRANSMISSION_USER";
const TRANSMISSION_PWD: &str = "TRANSMISSION_PWD";
const TRANSMISSION_URL: &str = "TRANSMISSION_URL";
const PIRATE_BAY_PROXY_URL: &str = "PIRATE_BAY_PROXY_URL";
const DELAY_REAPING_TASKS_SECS: &str = "DELAY_REAPING_TASKS_SECS";
const THUMBNAIL_DIR: &str = "THUMBNAIL_DIR";
const OPENAI_API_KEY: &str = "OPENAI_API_KEY";

//  Defaults
const DEFAULT_DATABASE_URL: &str = ":memory";
const DEFAULT_MIGRATIONS_DIR: &str = "./migrations";
const DEFAULT_TRANSMISSION_URL: &str = "http://higo.abamaxa.com:9091/transmission/rpc";
const DEFAULT_CLIENT_DIR: &str = "client";
const DEFAULT_PB_URL: &str = "https://thehiddenbay.com";
const DEFAULT_DELAY_REAPING_TASKS_SECS: i64 = 60;

pub fn get_movie_dir() -> String {
    env::var(MOVIE_DIR).expect("MOVIE_DIR environment variable is not set")
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

pub fn get_database_migration_dir() -> String {
    env::var(DATABASE_MIGRATION_DIR).unwrap_or_else(|_| String::from(DEFAULT_MIGRATIONS_DIR))
}

pub fn get_torrent_dir(default: Option<&String>) -> String {
    env::var(TORRENT_DIR).unwrap_or_else(|_| {
        default
            .expect("TORRENT_DIR not set and default available")
            .clone()
    })
}

pub fn get_google_key() -> String {
    env::var(GOOGLE_KEY).unwrap_or_default()
}

pub fn get_transmission_url() -> Url {
    let url = env::var(TRANSMISSION_URL).unwrap_or(String::from(DEFAULT_TRANSMISSION_URL));
    url.parse().expect("TRANSMISSION_URL is malformed")
}

pub fn get_transmission_credentials() -> (Result<String, VarError>, Result<String, VarError>) {
    (env::var(TRANSMISSION_USER), env::var(TRANSMISSION_PWD))
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

pub fn get_openai_api_key() -> String {
    env::var(OPENAI_API_KEY).unwrap_or_default()
}
