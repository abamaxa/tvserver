use std::sync::Arc;
use tauri::ipc::Invoke;
use crate::adaptors::TauriChannelPlayer;
use crate::domain::messages::{
    ClientLogMessage, Command, ConversionRequest, CopyFromServerRequest, DownloadRequest, MediaItem, PlayRequest, PlayerList, Response
};
use crate::domain::models::{Conversion, DownloadableItem, SearchResults, TaskListResults, AVAILABLE_CONVERSIONS};
use crate::domain::traits::MediaSharer;
use crate::domain::{SearchEngineType, TaskType};
use super::context::Context;

pub type SharedState = Arc<Context>;

#[tauri::command]
pub async fn tasks_add(
    state: tauri::State<'_, SharedState>,
    payload: DownloadRequest
) -> Result<Response, String> {
    match state.get_search().download(payload, state.get_local_sender()).await {
        Ok(_) => Ok(Response::success("download queued".to_string())),
        Err(err) => Err(err.to_string()),
    }
}

#[tauri::command]
pub async fn tasks_delete(
    state: tauri::State<'_, SharedState>,
    _: TaskType,
    key: String
) -> Result<Response, String> {
    match state.get_task_manager().remove(&key, state.get_storer()).await {
        Ok(_) => Ok(Response::success(String::from("success"))),
        Err(err) => Err(err.to_string()),
    }
}

#[tauri::command]
pub async fn tasks_list(
    state: tauri::State<'_, SharedState>
) -> Result<TaskListResults, String> {
    let mut tasks = state.get_task_manager().get_current_state().await;
    tasks.sort_by(|a, b| {
        let ord = a.display_name.cmp(&b.display_name);
        match ord {
            std::cmp::Ordering::Equal => a.key.cmp(&b.key),
            _ => ord,
        }
    });
    Ok(TaskListResults::success(tasks))
}

#[tauri::command]
pub async fn pirate_search(
    state: tauri::State<'_, SharedState>,
    query: String
) -> Result<SearchResults<DownloadableItem>, String> {
    let search = state.get_search();
    let downloader = search.get_search_engine(&SearchEngineType::Torrent);
    match downloader.search(&query).await {
        Ok(results) => Ok(results),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn youtube_search(
    state: tauri::State<'_, SharedState>,
    query: String
) -> Result<SearchResults<DownloadableItem>, String> {
    let search = state.get_search();
    let downloader = search.get_search_engine(&SearchEngineType::YouTube);
    match downloader.search(&query).await {
        Ok(results) => Ok(results),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn list_root_collection(
    state: tauri::State<'_, SharedState>
) -> Result<MediaItem, String> {
    list_media(&state, "").await
}

#[tauri::command]
pub async fn list_collection(
    state: tauri::State<'_, SharedState>,
    collection: String
) -> Result<MediaItem, String> {
    list_media(&state, &collection).await
}

async fn list_media(state: &SharedState, collection: &str) -> Result<MediaItem, String> {
    match state.get_store().list(collection).await {
        Ok(result) => Ok(result),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn log_client_message(
    payload: ClientLogMessage
) -> Result<(), String> {
    for message in &payload.messages {
        tracing::info!("Client Log: {} - {}", payload.level, message);
    }
    Ok(())
}

#[tauri::command]
pub async fn remote_play(
    state: tauri::State<'_, SharedState>,
    payload: PlayRequest
) -> Result<Response, String> {
    let key = payload.address();
    match state.get_messenger().execute(key, payload.make_remote_command()).await {
        Ok(()) => Ok(Response::success("success".to_string())),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn remote_command(
    state: tauri::State<'_, SharedState>,
    payload: Command
) -> Result<Response, String> {
    match state.get_messenger().execute(payload.address(), payload.message).await {
        Ok(()) => Ok(Response::success("success".to_string())),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn list_player(
    state: tauri::State<'_, SharedState>
) -> Result<PlayerList, String> {
    let players = PlayerList::new(state.get_messenger().list_players().await);
    Ok(players)
}

#[tauri::command]
pub async fn delete_video(
    state: tauri::State<'_, SharedState>,
    video_id: String
) -> Result<Response, String> {
    let id = video_id.parse::<i64>().map_err(|e| format!("Invalid video ID: {}", e))?;
    match state.get_store().delete(id).await {
        Ok(()) => Ok(Response::success("success".to_string())),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn share_video(
    state: tauri::State<'_, SharedState>,
    video_id: String
) -> Result<Response, String> {
    match state.get_sharing() {
        Some(sharing) => {
            match sharing.share(&video_id).await {
                Ok(()) => Ok(Response::success("success".to_string())),
                Err(e) => Err(e.to_string()),
            }
        }
        None => Err("sharing not enabled".to_string()),
    }
}

#[tauri::command]
pub async fn convert_video(
    state: tauri::State<'_, SharedState>,
    video_id: String,
    request: ConversionRequest
) -> Result<Response, String> {
    let id = video_id.parse::<i64>().map_err(|e| format!("Invalid video ID: {}", e))?;
    match Conversion::do_conversion(state.inner().clone(), &request.name, id).await {
        Ok(_) => Ok(Response::success("conversion queued".to_string())),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn list_conversions() -> Result<SearchResults<Conversion>, String> {
    Ok(SearchResults::success(AVAILABLE_CONVERSIONS.to_vec()))
}

#[tauri::command]
pub async fn channel_connect(
    app: tauri::AppHandle,
    state: tauri::State<'_, SharedState>,
    channel_name: String,
) -> Result<Response, String> { 
    // Create a TauriChannelPlayer instance
    let channel_player = Arc::new(TauriChannelPlayer::create(
        app.clone(),
        channel_name.clone(),
    ));
    
    if channel_name == "remote-player" {
        // Add the player to the MessageExchange
        state.get_messenger().add_player(channel_name, channel_player).await;
    } else {    
        // Register the player as a control as well to receive updates
        state.get_messenger().add_control(channel_name, channel_player).await;
    }
    
    Ok(Response::success("Channel connection established".to_string()))
}

#[tauri::command]
pub async fn download_videos(
    state: tauri::State<'_, SharedState>,
    payload: CopyFromServerRequest
) -> Result<Response, String> {
    match state.get_search().download_videos(payload.host_url, payload.videos, state.get_local_sender()).await {
        Ok(_) => Ok(Response::success("videos download queued".to_string())),
        Err(err) => Err(err.to_string()),
    }
}

// Helper function to register all commands
pub fn register_commands() -> impl Fn(Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        tasks_add,
        tasks_delete,
        tasks_list,
        pirate_search,
        youtube_search,
        list_root_collection,
        list_collection,
        log_client_message,
        remote_play,
        remote_command,
        list_player,
        delete_video,
        share_video,
        convert_video,
        list_conversions,
        channel_connect,
        download_videos
    ]
} 