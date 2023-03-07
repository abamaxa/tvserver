use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    debug_handler,
    extract::ws::WebSocketUpgrade,
    extract::{ConnectInfo, Path, Query, State},
    headers::HeaderMap,
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use tokio::sync::RwLock;

use crate::adaptors::RemoteBrowserPlayer;
use crate::domain::messages::{
    ClientLogMessage, Command, DownloadRequest, LocalCommand, PlayRequest, Response,
};
use crate::domain::models::{DownloadableItem, SearchResults, VideoEntry, YoutubeResponse};
use crate::domain::traits::{
    DownloadClient, JsonFetcher, MediaStorer, Player, SearchEngine, TextFetcher,
};
use crate::domain::{SearchEngineType, GOOGLE_KEY};
use crate::services::remote_player::RemotePlayerService;
use crate::services::{
    media_stream::stream_video, pirate_bay::PirateClient, torrents::TransmissionDaemon,
    youtube::YoutubeClient,
};

type QueryParams = Query<HashMap<String, String>>;

#[derive(Clone)]
pub struct Context {
    pub store: Arc<dyn MediaStorer>,
    pub youtube_fetcher: Arc<dyn JsonFetcher<YoutubeResponse>>,
    pub pirate_fetcher: Arc<dyn TextFetcher>,
    pub remote_players: Arc<RwLock<RemotePlayerService>>,
    // TODO: wrap Player in a Mutex
    pub player: Option<Arc<dyn Player>>,
}

impl Context {
    pub fn from(
        store: Arc<dyn MediaStorer>,
        youtube_fetcher: Arc<dyn JsonFetcher<YoutubeResponse>>,
        pirate_fetcher: Arc<dyn TextFetcher>,
        remote_players: RemotePlayerService,
        player: Option<Arc<dyn Player>>,
    ) -> Context {
        Context {
            store,
            youtube_fetcher,
            pirate_fetcher,
            remote_players: Arc::new(RwLock::new(remote_players)),
            player,
        }
    }
}

type SharedState = Arc<Context>;

pub fn register(shared_state: SharedState) -> Router {
    Router::new()
        .route("/downloads", post(downloads_add))
        .route("/downloads", get(downloads_list))
        .route("/downloads/*path", delete(downloads_delete))
        .route("/log", post(log_client_message))
        .route("/media", get(list_root_collection))
        .route("/media/*collection", get(list_collection))
        .route("/player/control", post(local_command))
        .route("/player/play", post(local_play))
        .route("/remote/control", post(remote_command))
        .route("/remote/play", post(remote_play))
        .route("/remote/ws", get(ws_handler))
        .route("/stream/*path", get(video))
        .route("/search/pirate", get(pirate_search))
        .route("/search/youtube", get(youtube_search))
        .with_state(shared_state)
}

#[debug_handler]
async fn downloads_add(
    state: State<SharedState>,
    payload: Json<DownloadRequest>,
) -> impl IntoResponse {
    let link = payload.link.as_str();
    match payload.engine {
        SearchEngineType::YouTube => {
            let key = env::var(GOOGLE_KEY).unwrap_or_default();
            download(
                &YoutubeClient::new(&key, state.youtube_fetcher.as_ref()),
                link,
            )
            .await
        }
        _ => download(&TransmissionDaemon::new(), link).await,
    }
}

async fn download(client: &dyn DownloadClient, link: &str) -> (StatusCode, Json<Response>) {
    match client.fetch(link).await {
        Ok(r) => (StatusCode::OK, Json(Response::success(r))),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response::error(err)),
        ),
    }
}

#[debug_handler]
async fn downloads_delete(Path(id): Path<i64>) -> (StatusCode, Json<Response>) {
    let daemon = TransmissionDaemon::new();
    match daemon.remove(id, false).await {
        Ok(_) => (
            StatusCode::OK,
            Json(Response::success(String::from("success"))),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response::error(err)),
        ),
    }
}

#[debug_handler]
async fn downloads_list() -> impl IntoResponse {
    let daemon: &dyn DownloadClient = &TransmissionDaemon::new();
    Json(daemon.list_in_progress().await)
}

#[debug_handler]
async fn pirate_search(state: State<SharedState>, params: QueryParams) -> impl IntoResponse {
    let client = PirateClient::new(None, state.pirate_fetcher.as_ref());
    do_search::<PirateClient, DownloadableItem>(&client, &params).await
}

#[debug_handler]
async fn youtube_search(state: State<SharedState>, params: QueryParams) -> impl IntoResponse {
    match env::var(GOOGLE_KEY) {
        Ok(key) => {
            let client = YoutubeClient::new(&key, state.youtube_fetcher.as_ref());
            do_search::<YoutubeClient, DownloadableItem>(&client, &params).await
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SearchResults::error("google api key is not configured")),
        ),
    }
}

async fn do_search<T, F>(
    client: &dyn SearchEngine<F>,
    params: &HashMap<String, String>,
) -> (StatusCode, Json<SearchResults<F>>) {
    match params.get("q") {
        Some(query) => match client.search(query).await {
            Ok(results) => (StatusCode::OK, Json(results)),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SearchResults::error(&e.to_string())),
            ),
        },
        _ => (
            StatusCode::OK,
            Json(SearchResults::error("missing q parameter")),
        ),
    }
}

#[debug_handler]
async fn local_command(
    state: State<SharedState>,
    payload: Json<LocalCommand>,
) -> impl IntoResponse {
    call_local_player(&state, |_, player| -> (StatusCode, Json<Response>) {
        match player.send_command(&payload.command, 0) {
            Ok(result) => (StatusCode::OK, Json(Response::success(result))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e))),
        }
    })
    .await
}

#[debug_handler]
async fn local_play(
    State(state): State<SharedState>,
    Json(payload): Json<PlayRequest>,
) -> (StatusCode, Json<Response>) {
    call_local_player(&state, |context, player| -> (StatusCode, Json<Response>) {
        if let Err(err) = player.send_command("clear", 1) {
            tracing::warn!("{:?}", err);
        }

        match player.send_command(&payload.make_local_command(&context.store), 0) {
            Ok(result) => (StatusCode::OK, Json(Response::success(result))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e))),
        }
    })
    .await
}

async fn call_local_player<F>(state: &SharedState, f: F) -> (StatusCode, Json<Response>)
where
    F: FnOnce(&Context, &Arc<dyn Player>) -> (StatusCode, Json<Response>),
{
    match &state.player {
        Some(player) => f(state, player),
        _ => (
            StatusCode::OK,
            Json(Response::success("no local player".to_string())),
        ),
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
        Ok(result) => (StatusCode::OK, Json(result)),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(VideoEntry::error(e.to_string())),
        ),
    }
}

#[debug_handler]
async fn log_client_message(Json(payload): Json<ClientLogMessage>) -> impl IntoResponse {
    for message in &payload.messages {
        tracing::info!("Client Log: {} - {}", payload.level, message);
    }
    StatusCode::OK
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
    let client_arc = Arc::new(client);

    state.remote_players.write().await.add(key, client_arc);

    response
}

#[debug_handler]
async fn video(
    State(state): State<SharedState>,
    Path(video_path): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let video_file = state.store.as_path("", &video_path);

    stream_video(&video_file, headers).await
}

#[debug_handler]
async fn remote_play(
    State(state): State<SharedState>,
    Json(payload): Json<PlayRequest>,
) -> (StatusCode, Json<Response>) {
    RemotePlayerService::execute(&state.remote_players, payload.make_remote_command()).await
}

#[debug_handler]
async fn remote_command(
    State(state): State<SharedState>,
    Json(payload): Json<Command>,
) -> (StatusCode, Json<Response>) {
    RemotePlayerService::execute(&state.remote_players, payload).await
}
