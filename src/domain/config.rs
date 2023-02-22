use std::env;

use crate::domain::{MOVIE_DIR, TRANSMISSION_DIR};

pub fn get_movie_dir() -> String {
    env::var(MOVIE_DIR).expect("MOVIE_DIR environment variable is not set")
}

pub fn get_torrents_dir() -> String {
    env::var(TRANSMISSION_DIR).unwrap_or_else(|_| get_movie_dir())
}

