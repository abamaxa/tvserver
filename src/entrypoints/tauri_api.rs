#[cfg(not(feature = "webserver"))]
use std::sync::Arc;
#[cfg(not(feature = "webserver"))]
use tauri::ipc::Invoke;
#[cfg(not(feature = "webserver"))]
use crate::adaptors::TauriChannelPlayer;
#[cfg(not(feature = "webserver"))]
use crate::domain::messages::{
    ClientLogMessage, Command, ConversionRequest, CopyFromServerRequest, DownloadRequest, MediaItem, PlayRequest, PlayerList, Response
};
#[cfg(not(feature = "webserver"))]
use crate::domain::models::{
    BookCollectionDetails, BookDetails, Conversion, DownloadableItem, SearchResults,
    TaskListResults, AVAILABLE_CONVERSIONS,
};
#[cfg(not(feature = "webserver"))]
use crate::domain::traits::{MediaSharer, Repository};
#[cfg(not(feature = "webserver"))]
use crate::domain::{SearchEngineType, TaskType};
#[cfg(not(feature = "webserver"))]
use crate::services::BookStore;
#[cfg(not(feature = "webserver"))]
use super::context::Context;
#[cfg(not(feature = "webserver"))]
use super::{AvailableBookRuntime, BookRuntime, BOOK_LIBRARY_UNAVAILABLE};

#[cfg(not(feature = "webserver"))]
pub type SharedState = Arc<Context>;

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_root_collection(
    state: tauri::State<'_, SharedState>
) -> Result<MediaItem, String> {
    list_media(&state, "").await
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_collection(
    state: tauri::State<'_, SharedState>,
    collection: String
) -> Result<MediaItem, String> {
    list_media(&state, &collection).await
}

#[cfg(not(feature = "webserver"))]
async fn list_media(state: &SharedState, collection: &str) -> Result<MediaItem, String> {
    match state.get_store().list(collection).await {
        Ok(result) => Ok(result),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_root_books(
    state: tauri::State<'_, SharedState>,
) -> Result<BookCollectionDetails, String> {
    let runtime = require_available_books(&state.get_book_runtime())?;
    list_books_core(runtime.store.as_ref(), "").await
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_books(
    state: tauri::State<'_, SharedState>,
    collection: String,
) -> Result<BookCollectionDetails, String> {
    let runtime = require_available_books(&state.get_book_runtime())?;
    list_books_core(runtime.store.as_ref(), &collection).await
}

#[cfg(not(feature = "webserver"))]
fn require_available_books(runtime: &BookRuntime) -> Result<Arc<AvailableBookRuntime>, String> {
    runtime
        .available()
        .ok_or_else(|| BOOK_LIBRARY_UNAVAILABLE.to_string())
}

#[cfg(not(feature = "webserver"))]
async fn list_books_core(
    book_store: &BookStore,
    collection: &str,
) -> Result<BookCollectionDetails, String> {
    book_store
        .list(collection)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "webserver"))]
fn parse_book_checksum(checksum: &str) -> Result<i64, String> {
    checksum
        .parse::<i64>()
        .map_err(|error| format!("Invalid book checksum '{checksum}': {error}"))
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn get_book(
    state: tauri::State<'_, SharedState>,
    checksum: String,
) -> Result<BookDetails, String> {
    require_available_books(&state.get_book_runtime())?;
    let repository = state.get_repository();
    get_book_core(&repository, &checksum).await
}

#[cfg(not(feature = "webserver"))]
async fn get_book_core(
    repository: &Repository,
    checksum: &str,
) -> Result<BookDetails, String> {
    let checksum = parse_book_checksum(checksum)?;
    repository
        .retrieve_book(checksum)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn delete_book(
    state: tauri::State<'_, SharedState>,
    checksum: String,
) -> Result<Response, String> {
    let runtime = require_available_books(&state.get_book_runtime())?;
    delete_book_core(runtime.store.as_ref(), &checksum).await
}

#[cfg(not(feature = "webserver"))]
async fn delete_book_core(book_store: &BookStore, checksum: &str) -> Result<Response, String> {
    let checksum = parse_book_checksum(checksum)?;
    book_store
        .delete(checksum)
        .await
        .map(|()| Response::success("success".to_string()))
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn log_client_message(
    payload: ClientLogMessage
) -> Result<(), String> {
    for message in &payload.messages {
        tracing::info!("Client Log: {} - {}", payload.level, message);
    }
    Ok(())
}

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_player(
    state: tauri::State<'_, SharedState>
) -> Result<PlayerList, String> {
    let players = PlayerList::new(state.get_messenger().list_players().await);
    Ok(players)
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn delete_video(
    state: tauri::State<'_, SharedState>,
    video_id: String
) -> Result<Response, String> {
    match state.get_store().delete(video_id).await {
        Ok(()) => Ok(Response::success("success".to_string())),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn list_conversions() -> Result<SearchResults<Conversion>, String> {
    Ok(SearchResults::success(AVAILABLE_CONVERSIONS.to_vec()))
}

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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

#[cfg(not(feature = "webserver"))]
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
        list_root_books,
        list_books,
        get_book,
        delete_book,
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

#[cfg(all(test, not(feature = "webserver")))]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use crate::{
        adaptors::{FileSystemStore, SqlRepository},
        domain::{
            models::{BookDetails, DEFAULT_BOOK_THUMBNAIL},
            traits::{FileStorer, Repository},
        },
        services::BookStore,
    };

    use super::{delete_book_core, get_book_core, list_books_core, require_available_books};
    use crate::entrypoints::{BookRuntime, BOOK_LIBRARY_UNAVAILABLE};

    #[test]
    fn unavailable_book_runtime_returns_stable_command_error() {
        let runtime = BookRuntime::Unavailable {
            message: Arc::from(BOOK_LIBRARY_UNAVAILABLE),
        };

        let error = require_available_books(&runtime)
            .err()
            .expect("unavailable runtime should return an error");

        assert_eq!(error, BOOK_LIBRARY_UNAVAILABLE);
    }

    fn sample_book(checksum: i64, collection: &str, file_name: &str) -> BookDetails {
        BookDetails {
            checksum,
            collection: collection.to_string(),
            file_name: file_name.to_string(),
            title: file_name.to_string(),
            thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            ..BookDetails::default()
        }
    }

    async fn test_book_store() -> (Arc<BookStore>, Repository, PathBuf) {
        let test_root = std::env::temp_dir().join(format!(
            "tvserver-tauri-book-commands-{:032x}",
            rand::random::<u128>()
        ));
        let book_root = test_root.join("books");
        let thumbnail_root = test_root.join("book-thumbnails");
        tokio::fs::create_dir_all(&book_root).await.unwrap();
        tokio::fs::create_dir_all(&thumbnail_root).await.unwrap();

        let repository: Repository =
            Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let book_files: FileStorer = Arc::new(FileSystemStore::new(
            book_root.to_str().expect("book test root should be UTF-8"),
        ));
        let thumbnail_files: FileStorer = Arc::new(FileSystemStore::new(
            thumbnail_root
                .to_str()
                .expect("thumbnail test root should be UTF-8"),
        ));
        let store = Arc::new(BookStore::new_with_roots(
            book_files,
            thumbnail_files,
            repository.clone(),
            &book_root,
            &thumbnail_root,
        ));

        (store, repository, test_root)
    }

    #[tokio::test]
    async fn lists_root_books_through_book_store() {
        let (store, repository, test_root) = test_book_store().await;
        repository
            .save_book(&sample_book(1, "", "root.epub"))
            .await
            .unwrap();
        repository
            .save_book(&sample_book(2, "Shelf", "nested.epub"))
            .await
            .unwrap();

        let result = list_books_core(store.as_ref(), "").await.unwrap();

        assert_eq!(result.collection, "");
        assert_eq!(result.books.len(), 1);
        assert_eq!(result.books[0].checksum, 1);
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn lists_requested_collection_through_book_store() {
        let (store, repository, test_root) = test_book_store().await;
        repository
            .save_book(&sample_book(3, "Shelf", "on-shelf.epub"))
            .await
            .unwrap();
        repository
            .save_book(&sample_book(4, "Other", "elsewhere.epub"))
            .await
            .unwrap();

        let result = list_books_core(store.as_ref(), "Shelf").await.unwrap();

        assert_eq!(result.collection, "Shelf");
        assert_eq!(result.books.len(), 1);
        assert_eq!(result.books[0].checksum, 3);
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn retrieves_book_for_full_width_i64_checksum() {
        let (store, repository, test_root) = test_book_store().await;
        let book = sample_book(i64::MAX, "Shelf", "largest.epub");
        repository.save_book(&book).await.unwrap();

        let result = get_book_core(&repository, &i64::MAX.to_string())
            .await
            .unwrap();

        assert_eq!(result.checksum, i64::MAX);
        assert_eq!(result.file_name, "largest.epub");
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn deletes_book_through_book_store_and_returns_success() {
        let (store, repository, test_root) = test_book_store().await;
        let book = sample_book(42, "Shelf", "delete-me.epub");
        repository.save_book(&book).await.unwrap();
        let book_path = test_root.join("books/Shelf/delete-me.epub");
        tokio::fs::create_dir_all(book_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&book_path, b"book").await.unwrap();

        let response = delete_book_core(store.as_ref(), "42").await.unwrap();

        assert_eq!(response.message, "success");
        assert!(response.errors.is_empty());
        assert_eq!(
            serde_json::to_value(&response).unwrap()["message"],
            "success"
        );
        assert!(repository.retrieve_book(42).await.is_err());
        assert!(tokio::fs::metadata(&book_path).await.is_err());
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn command_cores_reject_malformed_book_checksum() {
        let (store, repository, test_root) = test_book_store().await;

        let get_error = get_book_core(&repository, "not-a-checksum")
            .await
            .unwrap_err();
        let delete_error = delete_book_core(store.as_ref(), "not-a-checksum")
            .await
            .unwrap_err();

        assert!(get_error.starts_with("Invalid book checksum 'not-a-checksum':"));
        assert_eq!(delete_error, get_error);
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn command_cores_map_backend_errors_to_strings() {
        let (store, repository, test_root) = test_book_store().await;
        let expected = sqlx::Error::RowNotFound.to_string();

        let get_error = get_book_core(&repository, "404").await.unwrap_err();
        let delete_error = delete_book_core(store.as_ref(), "404").await.unwrap_err();

        assert_eq!(get_error, expected);
        assert_eq!(delete_error, expected);
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }
}
