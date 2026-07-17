use std::path::Path;

use anyhow::Result;
// Assuming these are defined in your codebase:
use super::media_kind::is_video_extension;
use crate::domain::models::VideoDetails;
use crate::domain::traits::Repository;

/// Asynchronously retrieves videos for a series or a given id.
///
/// If `series_or_id` can be parsed as an integer, it attempts to retrieve a single video
/// using that checksum. If that call fails or is not parseable as an integer, it splits
/// the input into series and season parts and calls `list_series_details`.
pub async fn get_videos_for_series_or_id(
    repo: Repository,
    series_or_id: &str,
) -> Result<Vec<VideoDetails>> {
    // Try to parse the input as a checksum (i64)
    if let Ok(checksum) = series_or_id.parse::<i64>() {
        match repo.retrieve_video(checksum).await {
            Ok(video) => {
                // Return the single video wrapped in a vector
                return Ok(vec![video]);
            }
            Err(_) => {
                // If retrieval by checksum fails, fall back to splitting the series string.
            }
        }
    }

    // Split the input to get series and season (if any)
    let (series, season) = split_at_last_slash(series_or_id);

    // Call the async method to list series details.
    // We pass the season as an Option<&str> (using as_deref() to convert Option<String> to Option<&str>)
    repo.list_series_details(&series, season.as_deref())
        .await
        .map_err(|e| anyhow::anyhow!("Error listing series details: {}", e))
}

pub fn skip_file(name: &str) -> bool {
    // Check if the name starts with a dot.
    if name.starts_with('.') {
        return true;
    }

    // Check if the name ends with ".tmp.mp4".
    if name.ends_with(".tmp.mp4") {
        return true;
    }

    // Check if the name is exactly "TV", "downloads", or "Downloads".
    if name == "TV" || name == "downloads" || name == "Downloads" {
        return true;
    }

    // Convert the file name to lowercase and extract the extension.
    let name_lowercase = name.to_lowercase();
    let file_ext = {
        let path = Path::new(&name_lowercase);
        match path.extension().and_then(|s| s.to_str()) {
            Some(ext) => ext.to_string(),
            None => String::new(),
        }
    };

    // If the file extension is one of the accepted ones, don't skip the file.
    if file_ext.is_empty()
        || is_video_extension(&file_ext)
        || matches!(file_ext.as_str(), "pdf" | "epub")
    {
        return false;
    }

    // Skip the file if none of the conditions for acceptance are met.
    true
}

pub fn is_video_scan_candidate(name: &str) -> bool {
    if skip_file(name) {
        return false;
    }
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        None | Some("") => true,
        Some(extension) => is_video_extension(extension),
    }
}

/// Splits the input string at the last occurrence of '/'.
/// Returns a tuple containing:
/// - The series part (everything before the last slash)
/// - An optional season part (everything after the last slash, if any)
fn split_at_last_slash(s: &str) -> (String, Option<String>) {
    if let Some(index) = s.rfind('/') {
        let series = s[..index].to_string();
        let season = s[index + 1..].to_string();
        (series, Some(season))
    } else {
        (s.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_file() {
        let test_cases = [
            ("TV", true),
            ("TV2", false),
            ("file.py", true),
            ("file.mp4", false),
            ("book.pdf", false),
            ("BOOK.PDF", false),
            ("book.epub", false),
            ("BOOK.EPUB", false),
            ("file.jpg", true),
            ("file.png", true),
        ];

        for (name, expected) in test_cases {
            assert_eq!(skip_file(name), expected);
        }
    }

    #[test]
    fn video_scan_rejects_books_and_preserves_extensionless_legacy_files() {
        for name in ["manual.pdf", "MANUAL.PDF", "novel.epub", "NOVEL.EPUB"] {
            assert!(!is_video_scan_candidate(name), "movie scan admitted {name}");
        }
        for name in ["movie.mp4", "MOVIE.MKV", "legacy-file"] {
            assert!(is_video_scan_candidate(name), "movie scan rejected {name}");
        }
        for name in [".hidden.mp4", "partial.tmp.mp4", "cover.jpg"] {
            assert!(!is_video_scan_candidate(name), "movie scan admitted {name}");
        }
    }
}
