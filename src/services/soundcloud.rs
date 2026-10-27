use crate::domain::models::{DownloadableItem, SearchResults};
use crate::domain::traits::MediaSearcher;
use crate::domain::SearchEngineType;
use async_trait::async_trait;

pub struct SoundcloudClient;

// Inspect more candidates in one API request so previews do not consume the
// 25 visible slots. Keep search work bounded even for very broad queries.
const MAX_CANDIDATES: usize = 100;

#[async_trait]
impl MediaSearcher<DownloadableItem> for SoundcloudClient {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<DownloadableItem>> {
        if query.trim().is_empty() {
            return Ok(SearchResults::success(vec![]));
        }
        let results = async {
            let mut command = crate::adaptors::youtube_runtime::python_command(include_str!(
                "soundcloud_search.py"
            ))
            .await?;
            command.args.extend([
                query.trim().chars().take(200).collect::<String>(),
                MAX_CANDIDATES.to_string(),
            ]);
            let data = super::ytdlp_search::run(command, "SoundCloud").await?;
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
            if !has_full_audio(entry) {
                return None;
            }
            let title = entry["title"].as_str()?.trim();
            if title.is_empty() {
                return None;
            }
            // Prefer the public track permalink over an API URI.
            let link = ["permalink_url", "webpage_url", "url", "uri"]
                .iter()
                .filter_map(|key| entry[key].as_str())
                .find(|link| is_track_url(link))?;
            let description = [
                entry["user"]["username"].as_str(),
                entry["uploader"].as_str(),
                entry["description"].as_str(),
            ]
            .into_iter()
            .flatten()
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

fn has_full_audio(entry: &serde_json::Value) -> bool {
    if !matches!(entry["policy"].as_str(), Some("ALLOW" | "MONETIZE"))
        || entry["streamable"].as_bool() != Some(true)
        || entry
            .get("access")
            .is_some_and(|access| !access.is_null() && access.as_str() != Some("playable"))
    {
        return false;
    }
    let Some(streams) = entry["media"]["transcodings"].as_array() else {
        return false;
    };
    streams.iter().any(|stream| {
        if stream["snipped"].as_bool() != Some(false)
            || !matches!(
                stream["format"]["protocol"].as_str(),
                Some("hls" | "progressive" | "http")
            )
            || !matches!(
                stream["preset"]
                    .as_str()
                    .and_then(|preset| preset.split('_').next()),
                Some("aac" | "mp3" | "opus")
            )
        {
            return false;
        }
        let Some(url) = stream["url"]
            .as_str()
            .and_then(|url| reqwest::Url::parse(url).ok())
        else {
            return false;
        };
        if !matches!(url.scheme(), "http" | "https")
            || url.path().contains("/preview/")
            || url.path().contains("encrypted-hls")
        {
            return false;
        }
        match (entry["full_duration"].as_u64(), stream["duration"].as_u64()) {
            // Stream durations can differ slightly because of codec padding.
            (Some(full), Some(actual)) => actual.saturating_add(1000) >= full,
            _ => true,
        }
    })
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

    fn full_track(title: &str) -> serde_json::Value {
        serde_json::json!({
            "title": title,
            "url": "https://soundcloud.com/artist/track",
            "policy": "MONETIZE",
            "access": null,
            "streamable": true,
            "duration": 180_000,
            "full_duration": 180_000,
            "media": {"transcodings": [{
                "url": "https://api-v2.soundcloud.com/media/track/stream/hls",
                "preset": "aac_160k",
                "snipped": false,
                "duration": 180_000,
                "format": {"protocol": "hls"}
            }]}
        })
    }

    #[test]
    fn excludes_blocked_preview_and_unverifiable_tracks_but_keeps_short_full_audio() {
        let full = full_track("Full track");
        let mut short = full_track("Genuine 29-second track");
        short["duration"] = 29_000.into();
        short["full_duration"] = 29_000.into();
        short["media"]["transcodings"][0]["duration"] = 29_000.into();
        let mut blocked = full_track("Blocked");
        blocked["policy"] = "BLOCK".into();
        let mut preview = full_track("Go+ preview");
        preview["policy"] = "SNIP".into();
        preview["media"]["transcodings"][0]["snipped"] = true.into();
        let mut only_preview = full_track("Preview stream despite track policy");
        only_preview["media"]["transcodings"][0]["snipped"] = true.into();
        let mut unstreamable = full_track("Not streamable");
        unstreamable["streamable"] = false.into();
        let mut drm = full_track("DRM only");
        drm["media"]["transcodings"][0]["format"]["protocol"] = "ctr-encrypted-hls".into();
        let mut unknown = full_track("Unknown availability");
        unknown.as_object_mut().unwrap().remove("policy");
        let response = serde_json::json!({"entries": [
            blocked, preview, only_preview, unstreamable, drm, unknown, full, short
        ]});
        let results = parse_results(&serde_json::to_vec(&response).unwrap()).unwrap();
        assert_eq!(
            results
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["Full track", "Genuine 29-second track"]
        );
    }

    #[test]
    fn keeps_public_track_links_and_artist_fallback_for_available_results() {
        let mut response = serde_json::json!({"entries": [
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
        for entry in response["entries"].as_array_mut().unwrap() {
            if let Some(fields) = entry.as_object_mut() {
                for key in ["policy", "streamable", "media"] {
                    fields.insert(key.into(), full_track("")[key].clone());
                }
            }
        }
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
        let mut entries: Vec<_> = (0..30)
            .map(|index| {
                let mut track = full_track(&format!("Track {index}"));
                track["url"] = format!("https://soundcloud.com/artist/track-{index}").into();
                track
            })
            .collect();
        let mut blocked = full_track("Blocked result before the full tracks");
        blocked["policy"] = "BLOCK".into();
        entries.splice(0..0, std::iter::repeat_n(blocked, 25));
        let response = serde_json::to_vec(&serde_json::json!({"entries": entries})).unwrap();
        let results = parse_results(&response).unwrap();
        assert_eq!(results.len(), 25);
        assert_eq!(results[0].title, "Track 0");
        assert_eq!(results[24].title, "Track 24");
    }

    #[test]
    fn parses_raw_api_permalinks_and_artist_names() {
        let mut track = full_track(" Public track ");
        track.as_object_mut().unwrap().remove("url");
        track["permalink_url"] = "https://soundcloud.com/artist/public-track".into();
        track["uri"] = "https://api.soundcloud.com/tracks/293".into();
        track["user"] = serde_json::json!({"username": "Artist"});
        let response = serde_json::to_vec(&serde_json::json!({"entries": [track]})).unwrap();
        let results = parse_results(&response).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Public track");
        assert_eq!(results[0].description, "Artist");
        assert_eq!(results[0].link, "https://soundcloud.com/artist/public-track");
    }

    #[test]
    fn requires_a_supported_full_stream_even_when_track_policy_allows_playback() {
        let full = full_track("Track");
        for (pointer, value) in [
            ("/access", serde_json::json!("preview")),
            ("/access", serde_json::json!("blocked")),
            ("/access", serde_json::json!(false)),
            ("/media/transcodings", serde_json::json!([])),
            ("/media/transcodings/0/snipped", serde_json::Value::Null),
            ("/media/transcodings/0/url", serde_json::json!("not a URL")),
            (
                "/media/transcodings/0/url",
                serde_json::json!("file:///audio.mp3"),
            ),
            (
                "/media/transcodings/0/url",
                serde_json::json!("https://api-v2.soundcloud.com/preview/track/hls"),
            ),
            (
                "/media/transcodings/0/url",
                serde_json::json!("https://api-v2.soundcloud.com/track/ctr-encrypted-hls"),
            ),
            ("/media/transcodings/0/duration", serde_json::json!(30_000)),
            ("/media/transcodings/0/preset", serde_json::json!("abr_sq")),
            (
                "/media/transcodings/0/format/protocol",
                serde_json::json!("unknown"),
            ),
        ] {
            let mut track = full.clone();
            *track.pointer_mut(pointer).unwrap() = value;
            assert!(!has_full_audio(&track), "Must reject {pointer}: {track}");
        }
        let mut mixed = full.clone();
        let mut preview = full["media"]["transcodings"][0].clone();
        preview["snipped"] = true.into();
        mixed["media"]["transcodings"]
            .as_array_mut()
            .unwrap()
            .insert(0, preview);
        assert!(has_full_audio(&mixed));
        mixed["policy"] = "ALLOW".into();
        mixed["access"] = "playable".into();
        assert!(has_full_audio(&mixed));
        mixed["media"]["transcodings"]
            .as_array_mut()
            .unwrap()
            .remove(1);
        let response = serde_json::to_vec(&serde_json::json!({"entries": [mixed]})).unwrap();
        assert!(parse_results(&response).unwrap().is_empty());
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

    #[tokio::test]
    #[ignore = "requires a configured yt-dlp runtime and SoundCloud access"]
    async fn live_soundcloud_search_excludes_known_preview_only_tracks() {
        let response = SoundcloudClient
            .search("daft punk get lucky")
            .await
            .unwrap();
        assert!(response.error.is_none(), "{:?}", response.error);
        let results = response.results.unwrap();
        assert!(!results.is_empty());
        assert!(results.len() <= super::super::ytdlp_search::MAX_RESULTS);
        assert!(
            results.iter().all(|item| !item
                .link
                .starts_with("https://soundcloud.com/daftpunkofficialmusic/get-lucky")),
            "Known preview-only official tracks must not appear: {results:?}"
        );
    }
}
