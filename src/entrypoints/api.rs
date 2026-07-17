use std::cmp::Ordering;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use crate::adaptors::RemoteBrowserPlayer;
use crate::domain::messages::{
    ClientLogMessage, Command, ConversionRequest, DownloadRequest,
    MediaItem, PlayRequest, PlayerList, Response,
};
use crate::domain::models::{
    BookCollectionDetails, Conversion, SearchResults, TaskListResults, AVAILABLE_CONVERSIONS,
};
use crate::domain::traits::{MediaSharer, Searcher};
use crate::domain::{SearchEngineType, TaskType};
use axum::body::Body;
use axum::routing::any;
use axum::{
    debug_handler,
    extract::ws::WebSocketUpgrade,
    extract::{rejection::PathRejection, ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response as AxumResponse},
    routing::{delete, get, post, patch},
    Json, Router,
};
use super::context::Context;
use super::BOOK_LIBRARY_UNAVAILABLE;

type QueryParams = Query<HashMap<String, String>>;
type StdResponse = (StatusCode, Json<Response>);

const BAD_REQUEST: StatusCode = StatusCode::BAD_REQUEST;
const INTERNAL_SERVER_ERROR: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;
const SERVICE_UNAVAILABLE: StatusCode = StatusCode::SERVICE_UNAVAILABLE;
const OK: StatusCode = StatusCode::OK;
const NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;
const BOOK_NOT_FOUND_MESSAGE: &str = "book not found";
const INTERNAL_ERROR_MESSAGE: &str = "internal server error";

pub type SharedState = Arc<Context>;

pub fn register(shared_state: SharedState) -> Router {
    Router::new()
        .route("/api/tasks", post(tasks_add))
        .route("/api/tasks", get(tasks_list))
        .route("/api/tasks/{type}/{*path}", delete(tasks_delete))
        .route("/api/log", post(log_client_message))
        .route("/api/media", get(list_root_collection))
        .route("/api/media/{*media}", get(list_collection))
        .route("/api/media/{*media}", delete(delete_video))
        .route("/api/media/{*media}", post(convert_video))
        .route("/api/media/{*media}", patch(share_video))
        .route("/api/books", get(list_root_books))
        .route("/api/books/{*collection}", get(list_books))
        .route("/api/book/{checksum}", get(get_book))
        .route("/api/book/{checksum}", delete(delete_book))
        .route("/api/remote", get(list_player))
        .route("/api/remote/control", post(remote_command))
        .route("/api/remote/play", post(remote_play))
        .route("/api/remote/ws", any(ws_player_handler))
        .route("/api/control/ws", any(ws_control_handler))
        .route("/api/search/pirate", get(pirate_search))
        .route("/api/search/youtube", get(youtube_search))
        .route("/api/conversion", get(list_conversions))
        .with_state(shared_state)
}

#[debug_handler]
async fn tasks_add(state: State<SharedState>, payload: Json<DownloadRequest>) -> impl IntoResponse {
    match state.get_search().download(payload.0, state.get_local_sender()).await {
        Ok(_) => (OK, Json(Response::success("download queued".to_string()))),
        Err(err) => (INTERNAL_SERVER_ERROR, Json(Response::error(err.to_string()))),
    }
}

#[debug_handler]
async fn tasks_delete(state: State<SharedState>, params: Path<(TaskType, String)>) -> StdResponse {

    let key = params.0 .1;

    match state.get_task_manager().remove(&key, state.get_storer()).await {
        Ok(_) => (OK, Json(Response::success(String::from("success")))),
        Err(err) => (INTERNAL_SERVER_ERROR, Json(Response::error(err.to_string()))),
    }
}

#[debug_handler]
async fn tasks_list(state: State<SharedState>) -> impl IntoResponse {
    let mut tasks = state.get_task_manager().get_current_state().await;
    tasks.sort_by(|a, b| {
        let ord = a.display_name.cmp(&b.display_name);
        match ord {
            Ordering::Equal => a.key.cmp(&b.key),
            _ => ord,
        }
    });
    Json(TaskListResults::success(tasks))
}

#[debug_handler]
async fn pirate_search(state: State<SharedState>, params: QueryParams) -> impl IntoResponse {
    let search = state.get_search();
    let downloader = search.get_search_engine(&SearchEngineType::Torrent);
    do_search(downloader, &params).await
}

#[debug_handler]
async fn youtube_search(state: State<SharedState>, params: QueryParams) -> impl IntoResponse {
    let search = state.get_search();
    let downloader = search.get_search_engine(&SearchEngineType::YouTube);
    do_search(downloader, &params).await
}

async fn do_search(client: &Searcher, params: &QueryParams) -> impl IntoResponse {
    match params.get("q") {
        Some(query) => match client.search(query).await {
            Ok(results) => (StatusCode::OK, Json(results)),
            Err(e) => (INTERNAL_SERVER_ERROR, Json(SearchResults::error(&e.to_string()))),
        },
        _ => (BAD_REQUEST, Json(SearchResults::error("missing q parameter"))),
    }
}


#[debug_handler]
async fn list_root_collection(state: State<SharedState>) -> impl IntoResponse {
    list_media(&state, "").await
}

#[debug_handler]
async fn list_collection(state: State<SharedState>, collection: Path<String>) -> impl IntoResponse {
    list_media(&state, &collection).await
}

async fn list_media(state: &SharedState, collection: &str) -> (StatusCode, Json<MediaItem>) {
    match state.get_store().list(collection).await {
        Ok(result) => (OK, Json(result)),
        Err(e) => (NOT_FOUND, Json(MediaItem::from(e))),
    }
}

#[debug_handler]
async fn list_root_books(state: State<SharedState>) -> impl IntoResponse {
    list_book_collection(&state, "").await
}

#[debug_handler]
async fn list_books(
    state: State<SharedState>,
    collection: Path<String>,
) -> impl IntoResponse {
    list_book_collection(&state, &collection).await
}

async fn list_book_collection(
    state: &SharedState,
    collection: &str,
) -> AxumResponse {
    let Some(runtime) = state.get_available_book_runtime() else {
        return book_library_unavailable_response().into_response();
    };

    match runtime.store.list(collection).await {
        Ok(result) => (OK, Json(result)).into_response(),
        Err(error) => {
            tracing::error!("Failed to list book collection {}: {}", collection, error);
            (
                INTERNAL_SERVER_ERROR,
                Json(BookCollectionDetails::error(INTERNAL_ERROR_MESSAGE.to_string())),
            )
                .into_response()
        }
    }
}

fn book_library_unavailable_response() -> StdResponse {
    std_error(SERVICE_UNAVAILABLE, BOOK_LIBRARY_UNAVAILABLE.to_string())
}

fn repository_book_error_status(error: &sqlx::Error) -> StatusCode {
    if matches!(error, sqlx::Error::RowNotFound) {
        NOT_FOUND
    } else {
        INTERNAL_SERVER_ERROR
    }
}

fn book_store_error_status(error: &anyhow::Error) -> StatusCode {
    error
        .downcast_ref::<sqlx::Error>()
        .map(repository_book_error_status)
        .unwrap_or(INTERNAL_SERVER_ERROR)
}

fn parse_book_checksum(checksum: &str) -> Result<i64, StdResponse> {
    checksum
        .parse::<i64>()
        .map_err(|_| invalid_book_checksum_error())
}

fn invalid_book_checksum_error() -> StdResponse {
    std_error(BAD_REQUEST, "invalid book checksum".to_string())
}

#[debug_handler]
async fn get_book(
    state: State<SharedState>,
    checksum: Result<Path<String>, PathRejection>,
) -> AxumResponse {
    let Some(_runtime) = state.get_available_book_runtime() else {
        return book_library_unavailable_response().into_response();
    };
    let checksum = match checksum {
        Ok(checksum) => checksum,
        Err(_) => return invalid_book_checksum_error().into_response(),
    };
    let checksum = match parse_book_checksum(&checksum.0) {
        Ok(checksum) => checksum,
        Err(response) => return response.into_response(),
    };

    match state.get_repository().retrieve_book(checksum).await {
        Ok(book) => Json(book).into_response(),
        Err(error) => {
            let status = repository_book_error_status(&error);
            if status == INTERNAL_SERVER_ERROR {
                tracing::error!("Failed to retrieve book {}: {}", checksum, error);
            }
            let message = if status == NOT_FOUND {
                BOOK_NOT_FOUND_MESSAGE
            } else {
                INTERNAL_ERROR_MESSAGE
            };
            (status, Json(Response::error(message.to_string()))).into_response()
        }
    }
}

#[debug_handler]
async fn delete_book(
    state: State<SharedState>,
    checksum: Result<Path<String>, PathRejection>,
) -> StdResponse {
    let Some(runtime) = state.get_available_book_runtime() else {
        return book_library_unavailable_response();
    };
    let checksum = match checksum {
        Ok(checksum) => checksum,
        Err(_) => return invalid_book_checksum_error(),
    };
    let checksum = match parse_book_checksum(&checksum.0) {
        Ok(checksum) => checksum,
        Err(response) => return response,
    };

    match runtime.store.delete(checksum).await {
        Ok(()) => (OK, Json(Response::success("success".to_string()))),
        Err(error) => {
            let status = book_store_error_status(&error);
            if status == INTERNAL_SERVER_ERROR {
                tracing::error!("Failed to delete book {}: {}", checksum, error);
            }
            let message = if status == NOT_FOUND {
                BOOK_NOT_FOUND_MESSAGE
            } else {
                INTERNAL_ERROR_MESSAGE
            };
            std_error(status, message.to_string())
        }
    }
}

#[debug_handler]
async fn log_client_message(Json(payload): Json<ClientLogMessage>) -> impl IntoResponse {
    for message in &payload.messages {
        tracing::info!("Client Log: {} - {}", payload.level, message);
    }
    OK
}

#[debug_handler]
pub async fn ws_player_handler(
    state: State<SharedState>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let key = addr.to_string();

    tracing::info!("opened websocket from player: {}", key);

    let messenger = state.get_messenger();

    let (client, response) = RemoteBrowserPlayer::create(ws, addr, messenger.get_sender());

    messenger.add_player(addr.to_string(), Arc::new(client)).await;

    response
}

#[debug_handler]
pub async fn ws_control_handler(
    state: State<SharedState>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    tracing::info!("opened websocket from remote control: {}", addr);

    let (client, response) = RemoteBrowserPlayer::create(ws, addr, state.get_messenger().get_sender());

    state.get_messenger().add_control(addr.to_string(), Arc::new(client)).await;

    response
}


#[debug_handler]
async fn remote_play(state: State<SharedState>, Json(payload): Json<PlayRequest>) -> StdResponse {
    let key = payload.address().to_string();
    match state.get_messenger().execute(key, payload.make_remote_command()).await {
        Ok(()) => (OK, Json(Response::success("success".to_string()))),
        Err(e) => std_error(INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[debug_handler]
async fn remote_command(
    State(state): State<SharedState>,
    Json(payload): Json<Command>,
) -> StdResponse {
    match state
        .get_messenger()
        .execute(payload.address(), payload.message)
        .await {
            Ok(()) => (OK, Json(Response::success("success".to_string()))),
            Err(e) => std_error(INTERNAL_SERVER_ERROR, e.to_string()),
        }
}

#[debug_handler]
async fn list_player(State(state): State<SharedState>) -> (StatusCode, Json<PlayerList>) {
    let players = PlayerList::new(state.get_messenger().list_players().await);
    (OK, Json(players))
}

#[debug_handler]
async fn delete_video(state: State<SharedState>, video_id: Path<String>) -> StdResponse {
    /*
    Cannot delete filenames with the `#` character in the name, think this is due
    to axum seeing everything past the # as being part of the query instead of the
    path. e.g. the following files cannot currently be deleted

    'Dragons Den - S19EP4 - OPAL ECO, Lewis #dragonsdennew [j5-D7HmSL9k].webm'
    'Dragons Den - S19EP5 - Berczy, Nick & Nick #dragonsdennew [Zlb1y7bLAlQ].webm'
    '#Dragons Dens - S19EP6 - LONDON NOOTROPICS [y9W2MTHwGLE].webm'
     */
    match state.get_store().delete(video_id.0).await {
        Ok(()) => (OK, Json(Response::success("success".to_string()))),
        Err(e) => std_error(INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[debug_handler]
async fn share_video(state: State<SharedState>, video_id: Path<String>) -> StdResponse {
    match state.get_sharing() {
        Some(sharing) => {
            match sharing.share(&video_id.0).await {
                Ok(()) => (OK, Json(Response::success("success".to_string()))),
                Err(e) => std_error(INTERNAL_SERVER_ERROR, e.to_string()),
            }
        }
        None => std_error(INTERNAL_SERVER_ERROR, "sharing not enabled".to_string()),
    }
}

#[debug_handler]
async fn convert_video(
    state: State<SharedState>,
    video_id: Path<i64>,
    request: Json<ConversionRequest>,
) -> StdResponse {
    match Conversion::do_conversion(state.0, &request.0.name, video_id.0).await {
        Ok(_) => (OK, Json(Response::success("conversion queued".to_string()))),
        Err(e) => std_error(INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[debug_handler]
async fn list_conversions() -> (StatusCode, Json<SearchResults<Conversion>>) {
    (OK, Json(SearchResults::success(AVAILABLE_CONVERSIONS.to_vec())))
}

pub async fn stream_audio(
    Path((audio_index, file_path)): Path<(u32, String)>,
) -> impl IntoResponse {
    let movie_dir = crate::domain::config::get_movie_dir();
    let full_path = std::path::PathBuf::from(&movie_dir).join(&file_path);

    if !full_path.exists() {
        return axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("file not found"))
            .unwrap();
    }

    let canonical_movie = match std::fs::canonicalize(&movie_dir) {
        Ok(p) => p,
        Err(_) => {
            return axum::response::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("server error"))
                .unwrap();
        }
    };

    let canonical_file = match std::fs::canonicalize(&full_path) {
        Ok(p) => p,
        Err(_) => {
            return axum::response::Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("file not found"))
                .unwrap();
        }
    };

    if !canonical_file.starts_with(&canonical_movie) {
        return axum::response::Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("forbidden"))
            .unwrap();
    }

    let mut child = match tokio::process::Command::new("ffmpeg")
        .arg("-i")
        .arg(canonical_file.as_os_str())
        .args(["-map", "0:v:0"])
        .args(["-map", &format!("0:a:{}", audio_index)])
        .args(["-c", "copy"])
        .args(["-f", "matroska"])
        .arg("pipe:1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            return axum::response::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("failed to start ffmpeg"))
                .unwrap();
        }
    };

    let stdout = child.stdout.take().unwrap();
    let stream = tokio_util::io::ReaderStream::new(stdout);

    // Spawn a background task to wait on the child process to prevent zombies
    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "video/x-matroska")
        .header("Cache-Control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}

fn std_error(code: StatusCode, message: String) -> StdResponse {
    (code, Json(Response::error(message)))
}
