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
        let checksum = video_id.parse::<i64>().map_err(|_| "Invalid video ID")?;
        let video = state
            .get_repository()
            .retrieve_video(checksum)
            .await
            .map_err(|error| error.to_string())?;
        let path = video
            .get_full_path()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let root = std::path::Path::new(&crate::domain::config::get_movie_dir())
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !path.starts_with(root) {
            return Err("Video is outside the library".into());
        }
        let payload = json!({ "videoId": video_id, "video": video.video, "collection": video.collection,
            "path": path, "startTime": start_time.filter(|time| time.is_finite() && *time >= 0.0).unwrap_or(0.0) }).to_string();
        return native_call("openPlayer", payload).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (state, video_id, start_time);
        Err("Native playback is only available on Android".into())
    }
}

#[tauri::command]
pub async fn android_player_control(
    action: String,
    seconds: Option<i32>,
    track: Option<i32>,
    rate: Option<f64>,
) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        native_call("controlPlayer", json!({"action": action, "seconds": seconds, "track": track.unwrap_or(-1), "rate": rate}).to_string()).await
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (action, seconds, track, rate);
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
    let videos = crate::domain::algorithm::get_videos_for_series_or_id(
        state.get_repository(), video_id,
    ).await.map_err(|error| error.to_string())?;
    if videos.is_empty() {
        return Err("No videos found".into());
    }
    let root = std::path::Path::new(&crate::domain::config::get_movie_dir())
        .canonicalize().map_err(|error| error.to_string())?;
    videos.into_iter().map(|video| {
        let path = video.get_full_path().canonicalize().map_err(|error| error.to_string())?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err("Video is outside the library or unavailable".into());
        }
        Ok(json!({ "path": path, "video": video.video }))
    }).collect()
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
