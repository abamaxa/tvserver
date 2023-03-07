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
use tokio::sync::{RwLock, RwLockReadGuard};

use super::{
    media_stream::stream_video, pirate_bay::PirateClient, torrents::TransmissionDaemon,
    youtube::YoutubeClient,
};
use crate::adaptors::{RemoteBrowserPlayer, RemotePlayer};
use crate::domain::messages::{
    ClientLogMessage, Command, DownloadRequest, LocalCommand, PlayRequest, RemoteMessage, Response,
};
use crate::domain::models::{DownloadableItem, SearchResults, VideoEntry, YoutubeResponse};
use crate::domain::traits::{
    DownloadClient, JsonFetcher, MediaStorer, Player, SearchEngine, TextFetcher,
};
use crate::domain::{SearchEngineType, GOOGLE_KEY};

type QueryParams = Query<HashMap<String, String>>;

#[derive(Clone)]
pub struct Context {
    pub store: Arc<dyn MediaStorer>,
    pub client_map: Arc<RwLock<HashMap<String, Arc<dyn RemotePlayer>>>>,
    pub remote_player: Arc<RwLock<Option<Arc<dyn RemotePlayer>>>>,
    pub youtube_fetcher: Arc<dyn JsonFetcher<YoutubeResponse>>,
    pub pirate_fetcher: Arc<dyn TextFetcher>,
    // TODO: wrap Player in Mutex, remove RWLock from Context
    pub player: Option<Arc<dyn Player>>,
}

impl Context {
    pub fn from(
        store: Arc<dyn MediaStorer>,
        youtube_fetcher: Arc<dyn JsonFetcher<YoutubeResponse>>,
        pirate_fetcher: Arc<dyn TextFetcher>,
        player: Option<Arc<dyn Player>>,
    ) -> Context {
        Context {
            player,
            store,
            youtube_fetcher,
            pirate_fetcher,
            client_map: Arc::new(RwLock::new(HashMap::<String, Arc<dyn RemotePlayer>>::new())),
            remote_player: Arc::new(RwLock::new(None)),
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
        let command = format!(
            "add file://{}",
            context.store.as_path(payload.collection, payload.video)
        );

        if let Err(err) = player.send_command("clear", 1) {
            tracing::warn!("{:?}", err);
        }

        match player.send_command(&command, 0) {
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
    State(state): State<SharedState>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let key = addr.to_string();

    tracing::info!("opened websocket from: {}", key);

    let (client, response) = RemoteBrowserPlayer::from(ws, addr);
    let client_arc = Arc::new(client);
    if let Err(e) = client_arc.clone().send(RemoteMessage::Stop).await {
        tracing::error!("failed to talk to new socket {}", e);
    }

    {
        let mut client_map = state.client_map.write().await;
        client_map.insert(key, client_arc.clone());
    }

    {
        let mut remote_player = state.remote_player.write().await;
        *remote_player = Some(client_arc);
    }

    response
}

#[debug_handler]
async fn video(
    State(state): State<SharedState>,
    Path(video_path): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let video_file = state.store.as_path("".to_string(), video_path);

    stream_video(&video_file, headers).await
}

#[debug_handler]
async fn remote_play(
    State(state): State<SharedState>,
    Json(payload): Json<PlayRequest>,
) -> (StatusCode, Json<Response>) {
    let url: String = if payload.collection.is_empty() {
        format!("/stream/{}  ", payload.video)
    } else {
        format!("/stream/{}/{}", payload.collection, payload.video)
    };

    let command = Command {
        remote_address: payload.remote_address,
        message: RemoteMessage::Play { url: url.clone() },
    };

    execute_remote(&state, command).await
}

#[debug_handler]
async fn remote_command(
    State(state): State<SharedState>,
    Json(payload): Json<Command>,
) -> (StatusCode, Json<Response>) {
    execute_remote(&state, payload).await
}

async fn execute_remote(state: &SharedState, command: Command) -> (StatusCode, Json<Response>) {
    let key = command.remote_address.unwrap_or_default();
    let remote_client: Arc<dyn RemotePlayer> = match state.client_map.read().await.get(&key) {
        Some(client) => client.clone(),
        _ => {
            let remote_player: RwLockReadGuard<Option<Arc<dyn RemotePlayer>>> =
                state.remote_player.read().await;
            match remote_player.clone() {
                Some(client) => client,
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(Response::error("missing remote_address".to_string())),
                    )
                }
            }
        }
    };

    match remote_client.send(command.message).await {
        Ok(result) => (result, Json(Response::success("todo".to_string()))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e))),
    }
}
