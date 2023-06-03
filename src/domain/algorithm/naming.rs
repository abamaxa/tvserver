use crate::domain::config::get_movie_dir;
use mockall::lazy_static;
use regex::Regex;
use std::path::{Path, PathBuf};
use titlecase::titlecase;

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

/*
pub fn get_collection_and_video(path: &str) -> (String, String) {
    if let Some(pos) = path.rfind("/") {
        return (path[..pos].to_string(), path[pos..].to_string());
    } else {
        return (String::new(), path.to_string());
    }
}*/

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

#[cfg(test)]
mod test {
    use super::*;

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
        let path = "tests/fixtures/media_dir/collection2/test_collection2-v1.mkv";

        assert_eq!(
            get_next_version_name(path, Some("mp4")),
            Some("tests/fixtures/media_dir/collection2/test_collection2-v3.mp4".to_string())
        );
    }
}
