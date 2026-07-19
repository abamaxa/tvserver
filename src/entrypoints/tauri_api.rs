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
    BookCollectionDetails, BookDetails, BookProgress, Conversion, DownloadableItem,
    SaveBookProgressRequest, SearchResults, TaskListResults, AVAILABLE_CONVERSIONS,
};
#[cfg(not(feature = "webserver"))]
use crate::domain::services::BookProgressService;
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
pub async fn list_book_progress(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<BookProgress>, String> {
    require_available_books(&state.get_book_runtime())?;
    let service = BookProgressService::new(state.get_repository());
    list_book_progress_core(&service).await
}

#[cfg(not(feature = "webserver"))]
async fn list_book_progress_core(
    service: &BookProgressService,
) -> Result<Vec<BookProgress>, String> {
    service.list().await.map_err(|error| error.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn get_book_progress(
    state: tauri::State<'_, SharedState>,
    checksum: String,
) -> Result<Option<BookProgress>, String> {
    require_available_books(&state.get_book_runtime())?;
    let service = BookProgressService::new(state.get_repository());
    get_book_progress_core(&service, &checksum).await
}

#[cfg(not(feature = "webserver"))]
async fn get_book_progress_core(
    service: &BookProgressService,
    checksum: &str,
) -> Result<Option<BookProgress>, String> {
    service
        .get(checksum)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn save_book_progress(
    state: tauri::State<'_, SharedState>,
    checksum: String,
    progress: SaveBookProgressRequest,
) -> Result<BookProgress, String> {
    require_available_books(&state.get_book_runtime())?;
    let service = BookProgressService::new(state.get_repository());
    save_book_progress_core(&service, &checksum, progress).await
}

#[cfg(not(feature = "webserver"))]
async fn save_book_progress_core(
    service: &BookProgressService,
    checksum: &str,
    progress: SaveBookProgressRequest,
) -> Result<BookProgress, String> {
    service
        .save(checksum, progress)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "webserver"))]
#[tauri::command]
pub async fn delete_book_progress(
    state: tauri::State<'_, SharedState>,
    checksum: String,
) -> Result<(), String> {
    require_available_books(&state.get_book_runtime())?;
    let service = BookProgressService::new(state.get_repository());
    delete_book_progress_core(&service, &checksum).await
}

#[cfg(not(feature = "webserver"))]
async fn delete_book_progress_core(
    service: &BookProgressService,
    checksum: &str,
) -> Result<(), String> {
    service
        .delete(checksum)
        .await
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
        list_book_progress,
        get_book_progress,
        save_book_progress,
        delete_book_progress,
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
            models::{
                BookDetails, BookLocatorType, RawBookLocator, SaveBookProgressRequest,
                DEFAULT_BOOK_THUMBNAIL,
            },
            services::{BookProgressError, BookProgressService},
            traits::{FileStorer, Repository},
        },
        services::BookStore,
    };

    use super::{
        delete_book_core, delete_book_progress_core, get_book_core, get_book_progress_core,
        list_book_progress_core, list_books_core, require_available_books, save_book_progress_core,
    };
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

    fn raw_progress(
        locator_type: &str,
        value: &str,
        progression: Option<f64>,
    ) -> SaveBookProgressRequest {
        SaveBookProgressRequest {
            locator: RawBookLocator {
                locator_type: locator_type.to_string(),
                value: value.to_string(),
            },
            progression,
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

    #[tokio::test]
    async fn book_progress_command_cores_preserve_list_option_save_and_unit_delete_types() {
        let (store, repository, test_root) = test_book_store().await;
        repository
            .save_book(&sample_book(10, "Shelf", "first.epub"))
            .await
            .unwrap();
        repository
            .save_book(&sample_book(20, "Shelf", "second.pdf"))
            .await
            .unwrap();
        let service = BookProgressService::new(repository);

        let missing: Option<crate::domain::models::BookProgress> =
            get_book_progress_core(&service, "10").await.unwrap();
        assert_eq!(missing, None);

        let saved: crate::domain::models::BookProgress = save_book_progress_core(
            &service,
            "20",
            raw_progress("pdf-page", "page-label:iv", Some(0.25)),
        )
        .await
        .unwrap();
        assert_eq!(saved.checksum, 20);
        assert_eq!(saved.locator.locator_type, BookLocatorType::PdfPage);
        assert_eq!(saved.locator.value, "page-label:iv");
        assert_eq!(saved.progression, Some(0.25));

        let listed: Vec<crate::domain::models::BookProgress> =
            list_book_progress_core(&service).await.unwrap();
        assert_eq!(listed, vec![saved]);

        let deleted: () = delete_book_progress_core(&service, "20").await.unwrap();
        assert_eq!(deleted, ());
        assert_eq!(get_book_progress_core(&service, "20").await.unwrap(), None);
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn book_progress_command_cores_replace_locator_and_remove_progression() {
        let (store, repository, test_root) = test_book_store().await;
        repository
            .save_book(&sample_book(30, "Shelf", "replace.epub"))
            .await
            .unwrap();
        let service = BookProgressService::new(repository);

        save_book_progress_core(
            &service,
            "30",
            raw_progress("epub-cfi", "epubcfi(/6/2)", Some(0.1)),
        )
        .await
        .unwrap();
        let replacement = save_book_progress_core(
            &service,
            "30",
            raw_progress("pdf-page", "chapter-a", None),
        )
        .await
        .unwrap();

        assert_eq!(replacement.locator.locator_type, BookLocatorType::PdfPage);
        assert_eq!(replacement.locator.value, "chapter-a");
        assert_eq!(replacement.progression, None);
        assert_eq!(
            get_book_progress_core(&service, "30").await.unwrap(),
            Some(replacement)
        );
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn book_progress_command_cores_use_stable_domain_errors_for_checksums_and_unknown_books() {
        let (store, repository, test_root) = test_book_store().await;
        let service = BookProgressService::new(repository);
        let invalid_checksum = BookProgressError::InvalidChecksum.to_string();
        let not_found = BookProgressError::BookNotFound.to_string();

        for checksum in ["not-a-checksum", "9223372036854775808"] {
            assert_eq!(
                get_book_progress_core(&service, checksum).await.unwrap_err(),
                invalid_checksum
            );
            assert_eq!(
                save_book_progress_core(
                    &service,
                    checksum,
                    raw_progress("epub-cfi", "epubcfi(/6/2)", None),
                )
                .await
                .unwrap_err(),
                invalid_checksum
            );
            assert_eq!(
                delete_book_progress_core(&service, checksum)
                    .await
                    .unwrap_err(),
                invalid_checksum
            );
        }
        assert_eq!(
            get_book_progress_core(&service, "404").await.unwrap_err(),
            not_found
        );
        assert_eq!(
            save_book_progress_core(
                &service,
                "404",
                raw_progress("epub-cfi", "epubcfi(/6/2)", None),
            )
            .await
            .unwrap_err(),
            not_found
        );
        assert_eq!(
            delete_book_progress_core(&service, "404")
                .await
                .unwrap_err(),
            not_found
        );
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn book_progress_save_core_preserves_raw_validation_error_messages() {
        let (store, repository, test_root) = test_book_store().await;
        repository
            .save_book(&sample_book(40, "Shelf", "validate.epub"))
            .await
            .unwrap();
        let service = BookProgressService::new(repository);

        for (request, expected) in [
            (
                raw_progress("future-locator", "opaque", None),
                BookProgressError::InvalidLocatorType.to_string(),
            ),
            (
                raw_progress("epub-cfi", " \t", None),
                BookProgressError::BlankLocatorValue.to_string(),
            ),
            (
                raw_progress("epub-cfi", "epubcfi(/6/2)", Some(1.01)),
                BookProgressError::InvalidProgression.to_string(),
            ),
            (
                raw_progress("epub-cfi", "epubcfi(/6/2)", Some(f64::NAN)),
                BookProgressError::InvalidProgression.to_string(),
            ),
        ] {
            assert_eq!(
                save_book_progress_core(&service, "40", request)
                    .await
                    .unwrap_err(),
                expected
            );
        }
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[tokio::test]
    async fn book_progress_save_core_serializes_i64_max_checksum_as_a_string() {
        let (store, repository, test_root) = test_book_store().await;
        repository
            .save_book(&sample_book(i64::MAX, "Shelf", "largest.epub"))
            .await
            .unwrap();
        let service = BookProgressService::new(repository);

        let saved = save_book_progress_core(
            &service,
            &i64::MAX.to_string(),
            raw_progress("epub-cfi", "epubcfi(/6/2)", Some(1.0)),
        )
        .await
        .unwrap();

        assert_eq!(saved.checksum, i64::MAX);
        assert_eq!(
            serde_json::to_value(saved).unwrap()["checksum"],
            i64::MAX.to_string()
        );
        drop(store);
        tokio::fs::remove_dir_all(test_root).await.unwrap();
    }

    #[test]
    fn book_progress_commands_gate_unavailable_runtime_before_service_construction() {
        let runtime = BookRuntime::Unavailable {
            message: Arc::from(BOOK_LIBRARY_UNAVAILABLE),
        };
        assert_eq!(
            require_available_books(&runtime)
                .err()
                .expect("unavailable runtime should return an error"),
            BOOK_LIBRARY_UNAVAILABLE
        );

        let source = include_str!("tauri_api.rs");
        for command in [
            "list_book_progress",
            "get_book_progress",
            "save_book_progress",
            "delete_book_progress",
        ] {
            let marker = format!("pub async fn {command}(");
            let body = source
                .split_once(&marker)
                .unwrap_or_else(|| panic!("missing Tauri command {command}"))
                .1
                .split_once("\n}\n")
                .expect("command should have a complete function body")
                .0;
            let availability_check = body
                .find("require_available_books")
                .unwrap_or_else(|| panic!("{command} must require the book runtime"));
            let service_construction = body
                .find("BookProgressService::new")
                .unwrap_or_else(|| panic!("{command} must construct the shared service"));
            assert!(
                availability_check < service_construction,
                "{command} must check availability before service construction"
            );
        }
    }

    #[test]
    fn book_progress_commands_are_registered_inside_tauri_handler_list() {
        let source = include_str!("tauri_api.rs");
        let handler = source
            .split_once("tauri::generate_handler![")
            .expect("Tauri handler list should exist")
            .1
            .split_once("\n    ]")
            .expect("Tauri handler list should be closed")
            .0;
        let registered = handler
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();

        for command in [
            "list_book_progress",
            "get_book_progress",
            "save_book_progress",
            "delete_book_progress",
        ] {
            assert!(
                registered.contains(&command),
                "{command} must be inside generate_handler!, found {registered:?}"
            );
        }
    }
}
