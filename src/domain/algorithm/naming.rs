use crate::domain::config::get_movie_dir;
#[cfg(not(feature = "webserver"))]
use crate::domain::config::get_thumbnail_dir;
use mockall::lazy_static;
use regex::Regex;
use std::{collections::HashSet, path::{Path, PathBuf}};
use titlecase::titlecase;

pub fn replace_extension(path: &str, new_extension: &str) -> String {
    let path_obj = Path::new(path);
    let stem = path_obj.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    
    match path_obj.parent() {
        Some(parent) => parent.join(format!("{}{}", stem, new_extension))
            .to_str()
            .unwrap_or(path)
            .to_string(),
        None => format!("{}{}", stem, new_extension),
    }
}

pub fn get_next_version_name(filename: &str, ext: Option<&str>) -> Option<String> {
    let path = Path::new(filename);

    let mut stem = path.file_stem()?.to_str()?.to_string();

    let extension = match ext {
        Some(ext) => ext,
        None => path.extension()?.to_str()?,
    };

    let mut new_path = PathBuf::from(path.parent()?);

    while new_path.exists() {
        stem = _get_next_version(&stem);

        new_path = PathBuf::from(path.parent()?);

        new_path.push(format!("{}.{}", stem, extension));
    }

    Some(new_path.to_str()?.to_string())
}

fn _get_next_version(file_stem: &str) -> String {
    lazy_static! {
        static ref VERSION_FINDER: Regex = Regex::new(r"(_|-| )v(\d+)$").unwrap();
    }

    match VERSION_FINDER.find(file_stem) {
        Some(version) => {
            let current_version = version.as_str()[2..].parse::<i32>().unwrap_or(1);
            let end_pos = file_stem.len() - (version.end() - version.start());
            format!("{}-v{}", &file_stem[..end_pos], current_version + 1)
        }
        _ => format!("{}-v2", file_stem),
    }
}

pub fn generate_display_name(name: &Option<String>) -> String {
    lazy_static! {
        static ref SEASON_FINDER: Regex = Regex::new(r"(S|s)\d{2}(E|e)\d{2}").unwrap();
        static ref RES_FINDER: Regex =
            Regex::new(r"4320|2160|1080|720|576|480|8K|4K|2K|HD|SD").unwrap();
        static ref EXT_FINDER: Regex = Regex::new(r" (\w+)$").unwrap();
    }

    let mut new_name = match name {
        Some(val) => val.clone(),
        None => String::from("mystery file.mkv"),
    };

    if new_name.len() < 24 {
        return titlecase(&new_name);
    }

    let mut start_pos = 0;

    if let Some(m) = SEASON_FINDER.find(&new_name) {
        start_pos = m.end();
    } else if let Some(m) = RES_FINDER.find(&new_name[8..]) {
        start_pos = m.start() + 7;
    }

    if start_pos != 0 {
        let end_pos: usize = new_name.rfind('.').unwrap_or(new_name.len());
        new_name.replace_range(start_pos..end_pos, "");
    }

    let end_pos: usize = new_name.rfind('.').unwrap_or(new_name.len());

    let result = new_name[0..end_pos].replace('.', " ") + &new_name[end_pos..];

    titlecase(&result)
}

pub fn get_collection_from_path(path: &Path) -> String {
    let short_path = match path.strip_prefix(&get_movie_dir()) {
        Ok(p) => PathBuf::from(p),
        _ => PathBuf::from(path),
    };

    if path.is_dir() {
        return short_path.to_str().unwrap_or_default().to_string();
    }

    match short_path.parent() {
        Some(parent) => parent.to_str().unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

pub fn get_collection_and_video_from_path(path: &Path) -> (String, String) {
    let short_path = match path.strip_prefix(&get_movie_dir()) {
        Ok(p) => PathBuf::from(p),
        _ => PathBuf::from(path),
    };

    let parent = match short_path.parent() {
        Some(parent) => parent.to_str().unwrap_or_default().to_string(),
        _ => String::new(),
    };

    (
        parent,
        short_path
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
            .to_string(),
    )
}

pub fn title_case(input: &str) -> String {
    // Define the exception words
    let exceptions: HashSet<&str> = ["of", "in", "the"].iter().cloned().collect();
    
    // Split the input by whitespace into words.
    let words: Vec<&str> = input.split_whitespace().collect();
    let mut result = Vec::with_capacity(words.len());

    for (i, &word) in words.iter().enumerate() {
        // Count the number of uppercase letters in the word.
        let num_capitals = word.chars().filter(|c| c.is_uppercase()).count();

        // If the word has more than one capital letter,
        // or it's the first word and has one capital letter,
        // or the word starts with '.', do not modify it.
        if num_capitals > 1 || (num_capitals == 1 && i == 0) || word.starts_with('.') {
            result.push(word.to_string());
        } else {
            // If it's the first word or the word is not in the exceptions set,
            // convert it to title case (first letter uppercase, rest lowercase).
            if i == 0 || !exceptions.contains(&word.to_lowercase()[..]) {
                let lower = word.to_lowercase();
                let mut chars = lower.chars();
                if let Some(first) = chars.next() {
                    let title_word = first.to_uppercase().to_string() + chars.as_str();
                    result.push(title_word);
                } else {
                    result.push(lower);
                }
            } else {
                // Otherwise, simply convert the word to lowercase.
                result.push(word.to_lowercase());
            }
        }
    }

    result.join(" ")
}

#[cfg(feature = "webserver")]
pub fn get_video_url(collection: &str, video: &str) -> String {
    if collection.is_empty() {
        format!("/api/stream/{}", video)
    } else {
        format!("/api/stream/{}/{}", collection, video)
    }
}

#[cfg(not(feature = "webserver"))]
pub fn get_video_url(collection: &str, video: &str) -> String {
    if collection.is_empty() {
        format!("{}/{}", get_movie_dir(), video)
    } else {
        format!("{}/{}/{}", get_movie_dir(), collection, video)
    }
}

#[cfg(feature = "webserver")]
pub fn get_thumbnails_url(thumbnails: &Vec<String>) -> Vec<String> {
    thumbnails
        .iter()
        .map(|t| format!("/api/thumbnails/{}", t))
        .collect::<Vec<String>>()
}

#[cfg(not(feature = "webserver"))]
pub fn get_thumbnails_url(thumbnails: &Vec<String>) -> Vec<String> {
    let thumbnail_dir = get_thumbnail_dir(&get_movie_dir()).to_str().unwrap_or_default().to_string();
    thumbnails
        .iter()
        .map(|t| format!("{}/{}", thumbnail_dir, t))
        .collect::<Vec<String>>()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_title_case() {
        // First word gets title-case conversion, exceptions remain lowercase (unless first word).
        let input = "a tale of two cities";
        assert_eq!(title_case(input), "A Tale of Two Cities");

        // If the first word already has a capital letter, it remains unchanged.
        //let input2 = "THE LORD OF THE RINGS";
        //assert_eq!(title_case(input2), "The Lord of the Rings");

        // Words starting with a dot are not modified.
        let input3 = ".hidden word"; 
        assert_eq!(title_case(input3), ".hidden Word");
    }

    #[test]
    fn test_get_next_version() {
        let test_cases = [
            ("file", "file-v2"),
            ("file_v3", "file-v4"),
            ("file_v10", "file-v11"),
            ("file v1", "file-v2"),
            ("file-v10", "file-v11"),
        ];

        for (test, expected) in test_cases {
            assert_eq!(_get_next_version(test), expected);
        }
    }

    #[test]
    fn test_generate_display_name() {
        let test_cases = [
            ("The.Lord.of.the.Rings.The.Rings.of.Power.S01E07.The.Eye.1080p.AMZN.WEB-DL.DDP5.1.Atmos.H.264-CMRG[eztv.re].mkv",
             "The Lord of the Rings the Rings of Power S01E07.mkv"),
            ("The.Lord.of.the.Rings.The.Rings.of.Power.The.Eye.1080p.AMZN.WEB-DL.DDP5.1.Atmos.H.264-CMRG[eztv.re].mkv",
             "The Lord of the Rings the Rings of Power the Eye.mkv"),
            ("1080p.mp4", "1080p.mp4"),
            ("1080 The file name is long, at least 1080p characters.mp4", "1080 the File Name Is Long, at least.mp4"),
            ("the.lord.of.the.rings.the.rings.of.power.s01e02.1080p.web.h264-cakes.mkv", "The Lord of the Rings the Rings of Power S01e02.mkv"),
        ];

        for (name, expected) in test_cases {
            assert_eq!(generate_display_name(&Some(String::from(name))), expected);
        }
    }

    #[test]
    fn test_get_next_version_name() {
        let input_path = PathBuf::from("tests/fixtures/media_dir/collection2/test_collection2-v1.mkv");
        let expected_path = PathBuf::from("tests/fixtures/media_dir/collection2")
            .join("test_collection2-v3.mp4");

        assert_eq!(
            get_next_version_name(input_path.to_str().unwrap(), Some("mp4")),
            Some(expected_path.to_str().unwrap().to_string())
        );
    }
}
