use crate::domain::models::{DownloadableItem, SearchResults};
use crate::domain::traits::MediaSearcher;
use crate::domain::SearchEngineType;
use async_trait::async_trait;

pub struct SoundcloudClient;

#[async_trait]
impl MediaSearcher<DownloadableItem> for SoundcloudClient {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<DownloadableItem>> {
        if query.trim().is_empty() {
            return Ok(SearchResults::success(vec![]));
        }
        let results = async {
            let data = super::ytdlp_search::search(query, "scsearch", "SoundCloud").await?;
            parse_results(&data)
        }
        .await;
        Ok(match results {
            Ok(items) => SearchResults::success(items),
            Err(error) => SearchResults::error(&error.to_string()),
        })
    }
}

fn parse_results(data: &[u8]) -> anyhow::Result<Vec<DownloadableItem>> {
    let response: serde_json::Value = serde_json::from_slice(data)?;
    let entries = response["entries"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("SoundCloud returned an invalid search response"))?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let title = entry["title"].as_str()?.trim();
            if title.is_empty() {
                return None;
            }
            // Flat results use an API URI for `url`; prefer the public track permalink.
            let link = ["webpage_url", "url"]
                .iter()
                .filter_map(|key| entry[key].as_str())
                .find(|link| is_track_url(link))?;
            let description = ["uploader", "description"]
                .iter()
                .filter_map(|key| entry[key].as_str())
                .find(|value| !value.trim().is_empty())
                .unwrap_or_default();
            Some(DownloadableItem {
                title: title.into(),
                description: description.into(),
                link: link.into(),
                engine: SearchEngineType::SoundCloud,
            })
        })
        .take(super::ytdlp_search::MAX_RESULTS)
        .collect())
}

fn is_track_url(link: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(link) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let path: Vec<_> = url
        .path()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    match url.host_str() {
        Some("soundcloud.com" | "www.soundcloud.com") => path.len() >= 2 && path[1] != "sets",
        Some("api.soundcloud.com" | "api-v2.soundcloud.com") => {
            path.len() == 2 && path[0] == "tracks"
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_results_and_keeps_public_track_links_and_artist_fallback() {
        let response = serde_json::json!({"entries": [
            null,
            {"title": " Flickermood ", "uploader": "Forss", "description": "Album track", "webpage_url": "https://soundcloud.com/forss/flickermood", "url": "https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A293"},
            {"title": "Another track", "description": "", "uploader": "Forss", "url": "https://soundcloud.com/forss/another-track"},
            {"title": "API track", "url": "https://api.soundcloud.com/tracks/293"},
            {"title": "Missing link", "id": "293"},
            {"title": " ", "url": "https://soundcloud.com/forss/empty"},
            {"title": "Other site", "url": "https://example.com/track"},
            {"title": "Playlist", "url": "https://soundcloud.com/forss/sets/album"},
            {"title": "Artist", "url": "https://soundcloud.com/forss"}
        ]});
        let results = parse_results(&serde_json::to_vec(&response).unwrap()).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].title, "Flickermood");
        assert_eq!(results[0].description, "Forss");
        assert_eq!(results[0].link, "https://soundcloud.com/forss/flickermood");
        assert_eq!(results[0].engine, SearchEngineType::SoundCloud);
        assert_eq!(results[1].description, "Forss");
        assert_eq!(results[2].link, "https://api.soundcloud.com/tracks/293");
    }

    #[test]
    fn handles_empty_and_malformed_responses_and_limits_results() {
        assert!(parse_results(br#"{"entries":[]}"#).unwrap().is_empty());
        for response in [b"not json".as_slice(), br#"{"entries":null}"#, b"{}"] {
            assert!(parse_results(response).is_err());
        }
        let entries: Vec<_> = (0..30).map(|index| serde_json::json!({
            "title": format!("Track {index}"), "url": format!("https://soundcloud.com/artist/track-{index}")
        })).collect();
        let response = serde_json::to_vec(&serde_json::json!({"entries": entries})).unwrap();
        assert_eq!(parse_results(&response).unwrap().len(), 25);
    }

    #[tokio::test]
    async fn empty_query_needs_no_runtime_or_credentials() {
        let response = SoundcloudClient.search("  ").await.unwrap();
        assert!(response.results.unwrap().is_empty());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a configured yt-dlp runtime and SoundCloud access"]
    async fn live_soundcloud_search() {
        let response = SoundcloudClient.search("forss flickermood").await.unwrap();
        assert!(response.error.is_none(), "{:?}", response.error);
        let results = response.results.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .all(|item| item.engine == SearchEngineType::SoundCloud && is_track_url(&item.link)));
    }
}
