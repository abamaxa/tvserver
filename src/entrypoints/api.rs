use std::cmp::Ordering;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use crate::adaptors::RemoteBrowserPlayer;
use crate::domain::messages::{
    ClientLogMessage, Command, ConversionRequest, DownloadRequest, LocalCommand,
    LocalMessageReceiver, LocalMessageSender, MediaItem, PlayRequest, PlayerList, RenameRequest,
    Response,
};
use crate::domain::models::{Conversion, SearchResults, TaskListResults, AVAILABLE_CONVERSIONS};
use crate::domain::messagebus::MessageExchange;
use crate::domain::traits::{MediaDownloader, Player, ProcessSpawner, Repository, Storer};
use crate::domain::{SearchEngineType, Searcher, TaskType};
use crate::services::{stream_video, SearchService, TaskManager, TransmissionDaemon};
use axum::{
    debug_handler,
    extract::ws::WebSocketUpgrade,
    extract::{ConnectInfo, Path, Query, State},
    headers::HeaderMap,
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};

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
    messenger: MessageExchange,
    player: Option<Arc<dyn Player>>,
    task_manager: Arc<TaskManager>,
    repository: Repository,
}

impl Context {
    pub fn new(
        store: Storer,
        search: SearchService,
        messenger: MessageExchange,
        player: Option<Arc<dyn Player>>,
        task_manager: Arc<TaskManager>,
        repository: Repository,
    ) -> Context {
        Context {
            store,
            search,
            messenger,
            player,
            task_manager,
            repository,
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

    pub fn get_repository(&self) -> Repository {
        self.repository.clone()
    }

    pub fn get_local_sender(&self) -> LocalMessageSender {
        self.messenger.get_local_sender()
    }

    pub fn get_local_receiver(&self) -> LocalMessageReceiver {
        self.messenger.get_local_receiver()
    }
}

pub type SharedState = Arc<Context>;

pub fn register(shared_state: SharedState) -> Router {
    Router::new()
        .route("/api/tasks", post(tasks_add))
        .route("/api/tasks", get(tasks_list))
        .route("/api/tasks/:type/*path", delete(tasks_delete))
        .route("/api/log", post(log_client_message))
        .route("/api/media", get(list_root_collection))
        .route("/api/media/*media", get(list_collection))
        .route("/api/media/*media", delete(delete_video))
        .route("/api/media/*media", put(rename_video))
        .route("/api/media/*media", post(convert_video))
        .route("/api/vlc/control", post(local_command))
        .route("/api/vlc/play", post(local_play))
        .route("/api/remote", get(list_player))
        .route("/api/remote/control", post(remote_command))
        .route("/api/remote/play", post(remote_play))
        .route("/api/remote/ws", get(ws_player_handler))
        .route("/api/control/ws", get(ws_control_handler))
        .route("/api/alt-stream/*path", get(video))
        .route("/api/search/pirate", get(pirate_search))
        .route("/api/search/youtube", get(youtube_search))
        .route("/api/conversion", get(list_conversions))
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

async fn list_media(state: &SharedState, collection: &str) -> (StatusCode, Json<MediaItem>) {
    match state.store.list(collection).await {
        Ok(result) => (OK, Json(result)),
        Err(e) => (NOT_FOUND, Json(MediaItem::from(e))),
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

    let (client, response) = RemoteBrowserPlayer::create(ws, addr, state.messenger.get_sender());

    state.messenger.add_player(addr, Arc::new(client)).await;

    response
}

pub async fn ws_control_handler(
    state: State<SharedState>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    tracing::info!("opened websocket from remote control: {}", addr);

    let (client, response) = RemoteBrowserPlayer::create(ws, addr, state.messenger.get_sender());

    state.messenger.add_control(addr, Arc::new(client)).await;

    response
}

#[debug_handler]
async fn video(
    State(state): State<SharedState>,
    Path(video_path): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let video_file = state.store.as_local_path("", &video_path);

    stream_video(&video_file, headers).await
}

#[debug_handler]
async fn remote_play(state: State<SharedState>, Json(payload): Json<PlayRequest>) -> StdResponse {
    let key = payload.address();
    state
        .messenger
        .execute(key, payload.make_remote_command())
        .await
}

#[debug_handler]
async fn remote_command(
    State(state): State<SharedState>,
    Json(payload): Json<Command>,
) -> StdResponse {
    state
        .messenger
        .execute(payload.address(), payload.message)
        .await
}

#[debug_handler]
async fn list_player(State(state): State<SharedState>) -> (StatusCode, Json<PlayerList>) {
    let players = PlayerList::new(state.messenger.list_players().await);
    (OK, Json(players))
}

#[debug_handler]
async fn delete_video(state: State<SharedState>, Path(collection): Path<String>) -> StdResponse {
    /*
    Cannot delete filenames with the `#` character in the name, think this is due
    to axum seeing everything past the # as being part of the query instead of the
    path. e.g. the following files cannot currently be deleted

    'Dragons Den - S19EP4 - OPAL ECO, Lewis #dragonsdennew [j5-D7HmSL9k].webm'
    'Dragons Den - S19EP5 - Berczy, Nick & Nick #dragonsdennew [Zlb1y7bLAlQ].webm'
    '#Dragons Dens - S19EP6 - LONDON NOOTROPICS [y9W2MTHwGLE].webm'
     */
    match state.store.delete(&collection).await {
        Ok(()) => (OK, Json(Response::success(collection))),
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
        let collection = state.store.as_local_path("", &collection);
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
