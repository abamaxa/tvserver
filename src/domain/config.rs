use std::env;

use crate::domain::MOVIE_DIR;

pub fn get_movie_dir() -> String {
    env::var(MOVIE_DIR).expect("MOVIE_DIR environment variable is not set")
}

