use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use crate::adaptors::RemoteBrowserPlayer;
use crate::domain::algorithm::{Conversion, AVAILABLE_CONVERSIONS};
use crate::domain::messages::{
    ClientLogMessage, Command, ConversionRequest, DownloadRequest, LocalCommand, PlayRequest,
    PlayerList, RenameRequest, Response,
};
use crate::domain::models::{SearchResults, TaskListResults, VideoEntry};
use crate::domain::traits::{MediaDownloader, Player, ProcessSpawner, Storer};
use crate::domain::{SearchEngineType, Searcher, TaskType};
use crate::services::{RemotePlayerService, SearchService, TaskManager, TransmissionDaemon};
use axum::{
    debug_handler,
    extract::ws::WebSocketUpgrade,
    extract::{ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use tokio::sync::RwLock;

type QueryParams = Query<HashMap<String, String>>;
type StdResponse = (StatusCode, Json<Response>);

const BAD_REQUEST: StatusCode = StatusCode::BAD_REQUEST;
const INTERNAL_SERVER_ERROR: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;
const OK: StatusCode = StatusCode::OK;
const NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;

#[derive(Clone)]
pub struct Context {
    store: Storer,
    search: SearchService,
    remote_players: Arc<RwLock<RemotePlayerService>>,
    player: Option<Arc<dyn Player>>,
    task_manager: Arc<TaskManager>,
}

impl Context {
    pub fn from(
        store: Storer,
        search: SearchService,
        remote_players: RemotePlayerService,
        player: Option<Arc<dyn Player>>,
        task_manager: Arc<TaskManager>,
    ) -> Context {
        Context {
            store,
            search,
            remote_players: Arc::new(RwLock::new(remote_players)),
            player,
            task_manager,
        }
    }

    pub fn get_store(&self) -> Storer {
        self.store.clone()
    }

    pub fn get_task_manager(&self) -> Arc<TaskManager> {
        self.task_manager.clone()
    }

    pub fn get_spawner(&self) -> Arc<impl ProcessSpawner> {
        self.task_manager.clone()
    }
}

pub type SharedState = Arc<Context>;

pub fn register(shared_state: SharedState) -> Router {
    Router::new()
        .route("/tasks", post(tasks_add))
        .route("/tasks", get(tasks_list))
        .route("/tasks/:type/*path", delete(tasks_delete))
        .route("/log", post(log_client_message))
        .route("/media", get(list_root_collection))
        .route("/media/*media", get(list_collection))
        .route("/media/*media", delete(delete_video))
        .route("/media/*media", put(rename_video))
        .route("/media/*media", post(convert_video))
        .route("/vlc/control", post(local_command))
        .route("/vlc/play", post(local_play))
        .route("/remote", get(list_player))
        .route("/remote/control", post(remote_command))
        .route("/remote/play", post(remote_play))
        .route("/remote/ws", get(ws_handler))
        //.route("/stream/*path", get(video))
        .route("/search/pirate", get(pirate_search))
        .route("/search/youtube", get(youtube_search))
        .route("/conversion", get(list_conversions))
        .with_state(shared_state)
}

#[debug_handler]
async fn tasks_add(state: State<SharedState>, payload: Json<DownloadRequest>) -> impl IntoResponse {
    let downloader = state.search.get_search_downloader(&payload.engine);
    match downloader.fetch(&payload.name, &payload.link).await {
        Ok(r) => (OK, Json(Response::success(r))),
        Err(err) => (INTERNAL_SERVER_ERROR, Json(Response::error(err))),
    }
}

#[debug_handler]
async fn tasks_delete(state: State<SharedState>, params: Path<(TaskType, String)>) -> StdResponse {
    // TODO: define a struct for these params
    let task_type = params.0 .0;
    let key = params.0 .1;
    // TODO: rationalize these two interfaces (one returns an error, the other a string on failure)
    if task_type == TaskType::Transmission {
        let daemon = TransmissionDaemon::new();
        match daemon.remove(&key, false).await {
            Ok(_) => (OK, Json(Response::success(String::from("success")))),
            Err(err) => (INTERNAL_SERVER_ERROR, Json(Response::error(err))),
        }
    } else {
        match state.task_manager.remove(&key, state.get_store()).await {
            Ok(_) => (OK, Json(Response::success(String::from("success")))),
            Err(err) => (INTERNAL_SERVER_ERROR, Json(Response::error(err.to_string()))),
        }
    }
}

#[debug_handler]
async fn tasks_list(state: State<SharedState>) -> impl IntoResponse {
    let mut tasks = state.search.get_task_states().await;
    tasks.extend_from_slice(&state.task_manager.get_current_state().await);
    Json(TaskListResults::success(tasks))
}

#[debug_handler]
async fn pirate_search(state: State<SharedState>, params: QueryParams) -> impl IntoResponse {
    let downloader = state.search.get_search_engine(&SearchEngineType::Torrent);
    do_search(downloader, &params).await
}

#[debug_handler]
async fn youtube_search(state: State<SharedState>, params: QueryParams) -> impl IntoResponse {
    let downloader = state.search.get_search_engine(&SearchEngineType::YouTube);
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
async fn local_command(
    state: State<SharedState>,
    payload: Json<LocalCommand>,
) -> impl IntoResponse {
    call_local_player(&state, |_, player| -> StdResponse {
        match player.send_command(&payload.command, 0) {
            Ok(result) => (OK, Json(Response::success(result))),
            Err(e) => (INTERNAL_SERVER_ERROR, Json(Response::error(e))),
        }
    })
    .await
}

#[debug_handler]
async fn local_play(state: State<SharedState>, Json(payload): Json<PlayRequest>) -> StdResponse {
    call_local_player(&state, |context, player| -> StdResponse {
        if let Err(err) = player.send_command("clear", 1) {
            tracing::warn!("{:?}", err);
        }

        match player.send_command(&payload.make_local_command(&context.store), 0) {
            Ok(result) => (OK, Json(Response::success(result))),
            Err(e) => (INTERNAL_SERVER_ERROR, Json(Response::error(e))),
        }
    })
    .await
}

async fn call_local_player<F>(state: &SharedState, f: F) -> StdResponse
where
    F: FnOnce(&Context, &Arc<dyn Player>) -> StdResponse,
{
    match &state.player {
        Some(player) => f(state, player),
        _ => (OK, Json(Response::success("no local player".to_string()))),
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

async fn list_media(state: &SharedState, collection: &str) -> (StatusCode, Json<VideoEntry>) {
    match state.store.list(collection).await {
        Ok(result) => (OK, Json(result)),
        Err(e) => (NOT_FOUND, Json(VideoEntry::error(e.to_string()))),
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
pub async fn ws_handler(
    state: State<SharedState>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let key = addr.to_string();

    tracing::info!("opened websocket from: {}", key);

    let (client, response) = RemoteBrowserPlayer::from(ws, addr);

    state
        .remote_players
        .write()
        .await
        .add(key, Arc::new(client));

    response
}

/*
#[debug_handler]
async fn video(
    State(state): State<SharedState>,
    Path(video_path): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let video_file = state.store.as_path("", &video_path);

    stream_video(&video_file, headers).await
}*/

#[debug_handler]
async fn remote_play(state: State<SharedState>, Json(payload): Json<PlayRequest>) -> StdResponse {
    RemotePlayerService::execute(&state.remote_players, payload.make_remote_command()).await
}

#[debug_handler]
async fn remote_command(
    State(state): State<SharedState>,
    Json(payload): Json<Command>,
) -> StdResponse {
    RemotePlayerService::execute(&state.remote_players, payload).await
}

#[debug_handler]
async fn list_player(State(state): State<SharedState>) -> (StatusCode, Json<PlayerList>) {
    let players = PlayerList::new(state.remote_players.read().await.list());
    (OK, Json(players))
}

#[debug_handler]
async fn delete_video(state: State<SharedState>, Path(collection): Path<String>) -> StdResponse {
    match state.store.delete(&collection).await {
        Ok(found) => match found {
            true => (OK, Json(Response::success(collection))),
            _ => std_error(NOT_FOUND, collection),
        },
        Err(e) => std_error(INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[debug_handler]
async fn rename_video(
    state: State<SharedState>,
    Path(collection): Path<String>,
    Json(params): Json<RenameRequest>,
) -> StdResponse {
    match state.store.rename(&collection, &params.new_name).await {
        Ok(_) => (OK, Json(Response::success(params.new_name))),
        _ => std_error(NOT_FOUND, collection),
    }
}

#[debug_handler]
async fn convert_video(
    state: State<SharedState>,
    collection: Path<String>,
    request: Json<ConversionRequest>,
) -> StdResponse {
    if let Some(conversion) = Conversion::find(&request.0.name) {
        let collection = state.store.as_path("", &collection);
        conversion.execute(state.get_spawner(), &collection).await;
        (OK, Json(Response::success("conversion queued".to_string())))
    } else {
        std_error(NOT_FOUND, format!("{} not recognized", request.0.name))
    }
}

#[debug_handler]
async fn list_conversions() -> (StatusCode, Json<SearchResults<Conversion>>) {
    (OK, Json(SearchResults::success(AVAILABLE_CONVERSIONS.to_vec())))
}

fn std_error(code: StatusCode, message: String) -> StdResponse {
    (code, Json(Response::error(message)))
}
