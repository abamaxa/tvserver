use super::SharedState;
use serde_json::{json, Value};

#[tauri::command]
pub async fn android_player_open(
    state: tauri::State<'_, SharedState>,
    video_id: String,
    start_time: Option<f64>,
) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        let payload = library_player_payload(state.inner(), &video_id, start_time).await?;
        return native_call("openPlayer", payload.to_string()).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (state, video_id, start_time);
        Err("Native playback is only available on Android".into())
    }
}

#[cfg(target_os = "android")]
async fn library_player_payload(
    state: &SharedState,
    video_id: &str,
    start_time: Option<f64>,
) -> Result<Value, String> {
    let checksum = video_id.parse::<i64>().map_err(|_| "Invalid video ID")?;
    let video = state
        .get_repository()
        .retrieve_video(checksum)
        .await
        .map_err(|error| error.to_string())?;
    let root = std::path::Path::new(&crate::domain::config::get_movie_dir())
        .canonicalize()
        .map_err(|error| error.to_string())?;
    video_player_payload(&video, &root, start_time)
}

#[cfg(any(target_os = "android", test))]
fn video_player_payload(
    video: &crate::domain::models::VideoDetails,
    root: &std::path::Path,
    start_time: Option<f64>,
) -> Result<Value, String> {
    let path = playable_library_path(video, root)?;
    let audio_only = video.metadata.width == 0
        && video.metadata.height == 0
        && video.metadata.audio_tracks > 0;
    Ok(json!({ "videoId": video.checksum.to_string(), "video": video.video,
        "collection": video.collection, "path": path, "audioOnly": audio_only,
        "startTime": start_time.filter(|time| time.is_finite() && *time >= 0.0).unwrap_or(0.0) }))
}

#[tauri::command]
pub async fn android_player_enqueue(
    state: tauri::State<'_, SharedState>,
    video_id: String,
) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        let item = library_player_payload(state.inner(), &video_id, None).await?;
        return native_call("controlPlayer", json!({"action": "enqueue", "item": item}).to_string()).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (state, video_id);
        Err("Native playback is only available on Android".into())
    }
}

#[cfg(any(target_os = "android", test))]
fn playable_library_path(
    video: &crate::domain::models::VideoDetails,
    root: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    use crate::domain::models::VideoState;
    if !matches!(video.state, VideoState::Ready | VideoState::NeedThumbnail) {
        return Err("This library item is not ready to play".into());
    }
    let path = video
        .get_full_path()
        .canonicalize()
        .map_err(|_| "This library item is unavailable")?;
    if !path.starts_with(root) || !path.is_file() {
        return Err("Video is outside the library or unavailable".into());
    }
    Ok(path)
}

#[tauri::command]
pub async fn android_playlist_open(
    state: tauri::State<'_, SharedState>,
    playlist_id: String,
    start_video_id: Option<String>,
    shuffle: Option<bool>,
) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        let playlist = state
            .get_repository()
            .retrieve_playlist(&playlist_id)
            .await
            .map_err(|error| error.to_string())?;
        let root = std::path::Path::new(&crate::domain::config::get_movie_dir())
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let payload = playlist_player_payload(
            playlist,
            &root,
            start_video_id.as_deref(),
            shuffle.unwrap_or(false),
        )?;
        return native_call("openPlayer", payload.to_string()).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (state, playlist_id, start_video_id, shuffle);
        Err("Native playback is only available on Android".into())
    }
}

#[cfg(any(target_os = "android", test))]
fn playlist_player_payload(
    playlist: crate::domain::models::PlaylistDetails,
    root: &std::path::Path,
    start_video_id: Option<&str>,
    shuffle: bool,
) -> Result<Value, String> {
    let items: Vec<Value> = playlist
        .videos
        .iter()
        .filter_map(|video| video_player_payload(video, root, None).ok())
        .collect();
    if items.is_empty() {
        return Err("This playlist has no available items to play".into());
    }
    if let Some(id) = start_video_id {
        if !items
            .iter()
            .any(|item| item["videoId"].as_str() == Some(id))
        {
            return Err("This playlist item is unavailable".into());
        }
    }
    Ok(json!({ "playlistId": playlist.id, "playlistName": playlist.name,
        "items": items, "startVideoId": start_video_id, "shuffle": shuffle }))
}

#[tauri::command]
pub async fn android_player_control(
    action: String,
    seconds: Option<i32>,
    track: Option<i32>,
    rate: Option<f64>,
    mode: Option<String>,
) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        if action == "show" {
            return native_call("openPlayer", json!({}).to_string()).await;
        }
        native_call("controlPlayer", json!({"action": action, "seconds": seconds, "track": track.unwrap_or(-1), "rate": rate, "mode": mode}).to_string()).await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (action, seconds, track, rate, mode);
        Err("Native playback is only available on Android".into())
    }
}

#[tauri::command]
pub fn android_player_status() -> Value {
    #[cfg(target_os = "android")]
    {
        crate::adaptors::android_platform::player_state()
    }
    #[cfg(not(target_os = "android"))]
    {
        json!({})
    }
}

#[tauri::command]
pub async fn android_runtime_info() -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        crate::adaptors::youtube_runtime::runtime_info()
            .await
            .map_err(|error| error.to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        Err("Bundled Python is only available on Android".into())
    }
}

#[tauri::command]
pub async fn android_video_share(
    state: tauri::State<'_, SharedState>,
    video_id: String,
) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        let files = library_files(state.inner(), &video_id).await?;
        native_call("shareVideos", json!({ "files": files }).to_string()).await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (state, video_id);
        Err("Native sharing is only available on Android".into())
    }
}

#[tauri::command]
pub async fn android_video_export(
    state: tauri::State<'_, SharedState>,
    video_id: String,
) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        video_id.parse::<i64>().map_err(|_| "Invalid video ID")?;
        let files = library_files(state.inner(), &video_id).await?;
        if files.len() != 1 {
            return Err("Select one video to download".into());
        }
        native_call("exportVideo", files[0].to_string()).await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (state, video_id);
        Err("Native file export is only available on Android".into())
    }
}

#[cfg(target_os = "android")]
async fn library_files(state: &SharedState, video_id: &str) -> Result<Vec<Value>, String> {
    let videos =
        crate::domain::algorithm::get_videos_for_series_or_id(state.get_repository(), video_id)
            .await
            .map_err(|error| error.to_string())?;
    if videos.is_empty() {
        return Err("No videos found".into());
    }
    let root = std::path::Path::new(&crate::domain::config::get_movie_dir())
        .canonicalize()
        .map_err(|error| error.to_string())?;
    videos
        .into_iter()
        .map(|video| {
            let path = video
                .get_full_path()
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if !path.starts_with(&root) || !path.is_file() {
                return Err("Video is outside the library or unavailable".into());
            }
            Ok(json!({ "path": path, "video": video.video }))
        })
        .collect()
}

#[cfg(target_os = "android")]
async fn native_call(method: &'static str, payload: String) -> Result<Value, String> {
    let output = tokio::task::spawn_blocking(move || {
        crate::adaptors::android_platform::call(method, Some(&payload))
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    serde_json::from_str(&output).map_err(|error| error.to_string())
}

#[cfg(test)]
mod playlist_tests {
    use super::*;
    use crate::domain::models::{PlaylistDetails, VideoDetails, VideoState};

    #[test]
    fn queue_uses_only_ready_files_inside_the_library_and_keeps_ids_as_strings() {
        let root =
            std::env::temp_dir().join(format!("playlist-paths-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("ready.mp3"), b"audio").unwrap();
        std::fs::create_dir(root.join("directory.mp3")).unwrap();
        let video = VideoDetails {
            checksum: i64::MAX,
            video: "ready.mp3".into(),
            dir_path: Some(root.clone()),
            ..Default::default()
        };
        let playlist = || PlaylistDetails {
            id: "playlist".into(),
            name: "Mix".into(),
            videos: vec![
                video.clone(),
                VideoDetails {
                    checksum: 2,
                    video: "missing.mp3".into(),
                    ..video.clone()
                },
                VideoDetails {
                    checksum: 3,
                    state: VideoState::Exception,
                    ..video.clone()
                },
                VideoDetails {
                    checksum: 4,
                    video: "directory.mp3".into(),
                    ..video.clone()
                },
                VideoDetails {
                    checksum: 5,
                    video: "../outside.mp3".into(),
                    ..video.clone()
                },
            ],
        };
        let payload = playlist_player_payload(playlist(), &root, None, true).unwrap();
        assert_eq!(payload["items"].as_array().unwrap().len(), 1);
        assert_eq!(payload["items"][0]["videoId"], i64::MAX.to_string());
        assert_eq!(payload["items"][0]["startTime"], 0.0);
        // The default mode follows the stream metadata, including audio in a
        // video-named file. Native queue transitions retain the session's mode.
        let mut audio = video.clone();
        audio.metadata.width = 0;
        audio.metadata.height = 0;
        audio.metadata.audio_tracks = 1;
        assert_eq!(video_player_payload(&audio, &root, None).unwrap()["audioOnly"], true);
        audio.metadata.width = 1920;
        audio.metadata.height = 1080;
        assert_eq!(video_player_payload(&audio, &root, Some(42.0)).unwrap()["audioOnly"], false);
        assert_eq!(video_player_payload(&audio, &root, Some(42.0)).unwrap()["startTime"], 42.0);
        assert!(playlist_player_payload(playlist(), &root, Some("2"), false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
