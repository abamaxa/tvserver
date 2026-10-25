use super::SharedState;
use crate::domain::models::{PlaylistDetails, PlaylistSummary};

fn command_error(error: sqlx::Error) -> String {
    match error {
        sqlx::Error::RowNotFound => "The playlist or library item no longer exists".into(),
        sqlx::Error::Protocol(message) => message,
        error => error.to_string(),
    }
}

#[tauri::command]
pub async fn playlists_list(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<PlaylistSummary>, String> {
    state
        .get_repository()
        .list_playlists()
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn playlist_get(
    state: tauri::State<'_, SharedState>,
    playlist_id: String,
) -> Result<PlaylistDetails, String> {
    state
        .get_repository()
        .retrieve_playlist(&playlist_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn playlist_create(
    state: tauri::State<'_, SharedState>,
    name: String,
) -> Result<PlaylistDetails, String> {
    state
        .get_repository()
        .create_playlist(&name)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn playlist_rename(
    state: tauri::State<'_, SharedState>,
    playlist_id: String,
    name: String,
) -> Result<(), String> {
    state
        .get_repository()
        .rename_playlist(&playlist_id, &name)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn playlist_delete(
    state: tauri::State<'_, SharedState>,
    playlist_id: String,
) -> Result<(), String> {
    state
        .get_repository()
        .delete_playlist(&playlist_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn playlist_add_video(
    state: tauri::State<'_, SharedState>,
    playlist_id: String,
    video_id: String,
) -> Result<(), String> {
    let checksum = video_id.parse::<i64>().map_err(|_| "Invalid video ID")?;
    state
        .get_repository()
        .add_playlist_video(&playlist_id, checksum)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn playlist_remove_video(
    state: tauri::State<'_, SharedState>,
    playlist_id: String,
    video_id: String,
) -> Result<(), String> {
    let checksum = video_id.parse::<i64>().map_err(|_| "Invalid video ID")?;
    state
        .get_repository()
        .remove_playlist_video(&playlist_id, checksum)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn playlist_reorder(
    state: tauri::State<'_, SharedState>,
    playlist_id: String,
    video_ids: Vec<String>,
) -> Result<(), String> {
    let checksums = video_ids
        .iter()
        .map(|id| id.parse::<i64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Invalid video ID")?;
    state
        .get_repository()
        .reorder_playlist(&playlist_id, &checksums)
        .await
        .map_err(command_error)
}
