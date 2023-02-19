use std::collections::HashMap;
use std::env;
use std::sync::{Arc, RwLock};
use std::net::SocketAddr;

use axum::{
    debug_handler,
    extract::{ConnectInfo, Path, Query, State},
    extract::ws::WebSocketUpgrade,
    headers::HeaderMap,
    routing::{delete, get, post},
    http::StatusCode,
    response::IntoResponse,
    Json,
    Router
};

use crate::adaptors::{VideoEntry, VideoStore, PirateClient, RemotePlayer, RemoteBrowserPlayer, TransmissionDaemon, TorrentDaemon};
use crate::adaptors::youtube::YoutubeClient;
use crate::domain::events::{
    DownloadRequest,
    LocalCommand,
    PlayRequest,
    Response,
    ClientLogMessage,
    RemoteMessage,
    Command
};
use crate::domain::GOOGLE_KEY;
use crate::domain::models::{DownloadableItem, SearchResults};
use crate::domain::traits::{Player, SearchEngine};
use crate::services::video_serving::stream_video;

#[derive(Clone)]
pub struct Context {
    player: Option<Arc<dyn Player>>,
    store: Arc<dyn VideoStore>,
    client_map: HashMap<String, Arc<dyn RemotePlayer>>,
    remote_player: Option<Arc<dyn RemotePlayer>>,
}

impl Context {
    pub fn from(player: Option<Arc<dyn Player>>, store: Arc<dyn VideoStore>) -> Context {
        Context{
            player,
            store,
            client_map: HashMap::<String, Arc<dyn RemotePlayer>>::new(),
            remote_player: None,
        }
    }
}

type SharedState = Arc<RwLock<Context>>;

pub fn register(player: Option<Arc<dyn Player>>, store: Arc<dyn VideoStore>) -> Router {
    let shared_state: SharedState = Arc::new(RwLock::new(Context::from(player, store)));

    Router::new()
        .route("/collections", get(list_collections))
        .route("/videos/*collection", get(list_videos))
        .route("/play", post(play))
        .route("/remote-play", post(remote_play))
        .route("/remote", post(handle_command))
        .route("/remote-control", post(remote_command))
        .route("/stream/*path", get(video))
        .route("/search/pirate", get(pirate_search))
        .route("/search/youtube", get(youtube_search))
        .route("/downloads/add", post(downloads_add))
        .route("/downloads/list", get(downloads_list))
        .route("/downloads/*path", delete(downloads_delete))
        .route("/log", post(log_client_message))
        .route("/ws", get(ws_handler))
        .with_state(shared_state)
}

#[debug_handler]
async fn downloads_add(Json(payload): Json<DownloadRequest>) -> (StatusCode, Json<Response>) {
    let daemon = TransmissionDaemon::new();
    match daemon.add(payload.link.as_str()).await {
        Ok(r) =>(StatusCode::OK, Json(Response::success(r))),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(err)))
    }
}

#[debug_handler]
async fn downloads_delete(Path(id): Path<i64>) -> (StatusCode, Json<Response>) {
    let daemon = TransmissionDaemon::new();
    match daemon.delete(id, false).await {
        Ok(_) =>(StatusCode::OK, Json(Response::success(String::from("success")))),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(err)))
    }
}

#[debug_handler]
async fn downloads_list() -> impl IntoResponse {
    let daemon = TransmissionDaemon::new();
    match daemon.list().await {
        Ok(r) =>(StatusCode::OK, Json(r)),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, Json(err))
    }
}

#[debug_handler]
async fn pirate_search(Query(params): Query<HashMap<String, String>>) -> (StatusCode, Json<SearchResults<DownloadableItem>>) {
    let client = PirateClient::new(None);
    do_search::<PirateClient, DownloadableItem>(&client, &params).await
}

#[debug_handler]
async fn youtube_search(Query(params): Query<HashMap<String, String>>) -> (StatusCode,Json<SearchResults<DownloadableItem>>) {
    match env::var(GOOGLE_KEY) {
        Ok(key) => {
            let client = YoutubeClient::new(&key);
            do_search::<YoutubeClient, DownloadableItem>(&client, &params).await
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SearchResults::error("google api key is not configured"))
        )
    }
}

async fn do_search<T, F>(client: &dyn SearchEngine<F>, params: &HashMap<String, String>) ->  (StatusCode, Json<SearchResults<F>>) {
    match params.get("q") {
        Some(query) => {
            match client.search(query).await {
                Ok(results) => (StatusCode::OK, Json(results)),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(SearchResults::error(&e.to_string())))
            }
        }
        _ => (StatusCode::OK, Json(SearchResults::error("missing q parameter")))
    }
}

#[debug_handler]
async fn handle_command(State(state): State<SharedState>, Json(payload): Json<LocalCommand>) -> (StatusCode, Json<Response>) {
    call_local_player(&state, |_, player| -> (StatusCode, Json<Response>) {
        match player.send_command(&payload.command, 0) {
            Ok(result) => (StatusCode::OK, Json(Response::success(result))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e)))
        }
    }).await
}

#[debug_handler]
async fn play(State(state): State<SharedState>, Json(payload): Json<PlayRequest>) -> (StatusCode, Json<Response>) {
    call_local_player(&state, |context, player| -> (StatusCode, Json<Response>) {
        let command = format!(
            "add file://{}", context.store.as_path(payload.collection, payload.video)
        );

        if let Err(err) = player.send_command("clear", 1) {
            println!("{:?}", err);
        }

        match player.send_command(&command, 0) {
            Ok(result) => (StatusCode::OK, Json(Response::success(result))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e)))
        }
    }).await
}


async fn call_local_player<F>(state: &SharedState, f: F) -> (StatusCode, Json<Response>)
    where F: FnOnce(&Context, &Arc<dyn Player>) -> (StatusCode, Json<Response>)
{
    if let Ok(context) = state.read() {
        return if let Some(player) = &context.player {
            f(&context, player)
        } else {
            (StatusCode::OK, Json(Response::success("no local player".to_string())))
        }
    }

    (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error("cannot lock state".to_string())))
}

#[debug_handler]
async fn list_collections(State(state): State<SharedState>) -> impl IntoResponse {

    match state.read().unwrap().store.list("".to_string()) {
        Ok(result) => (StatusCode::OK, Json(result)),
        Err(e) => (StatusCode::NOT_FOUND, Json(VideoEntry::error(e.to_string())))
    }
}

#[debug_handler]
async fn list_videos(State(state): State<SharedState>, Path(collection): Path<String>) -> (StatusCode, Json<VideoEntry>) {

    match state.read().unwrap().store.list(collection) {
        Ok(result) => (StatusCode::OK, Json(result)),
        Err(e) => (StatusCode::NOT_FOUND, Json(VideoEntry::error(e.to_string())))
    }
}

#[debug_handler]
async fn log_client_message(Json(payload): Json<ClientLogMessage>) -> impl IntoResponse {
    for message in payload.messages {
        println!("Client Log: {} - {}", payload.level, message);
    }

    StatusCode::OK
}

#[debug_handler]
pub async fn ws_handler(
    State(state): State<SharedState>,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>
    ) -> impl IntoResponse {

    let key =  addr.to_string();

    println!("opened websocket from: {}", key);

    let (client, response) = RemoteBrowserPlayer::from(ws, addr);
    let client_arc = Arc::new(client);
    if let Err(e) = client_arc.clone().execute(RemoteMessage::Stop).await {
        println!("failed to talk to new socket {}", e);
    }

    if let Ok(mut context) = state.write() {
        context.client_map.insert(key, client_arc.clone());
        context.remote_player = Some(client_arc);
    }

    response
}

#[debug_handler]
async fn video(State(state): State<SharedState>,  Path(video_path): Path<String>,  headers: HeaderMap) -> impl IntoResponse {

    let video_file = state.read().unwrap().store.as_path("".to_string(), video_path);

    stream_video(video_file, headers).await
}

#[debug_handler]
async fn remote_play(State(state): State<SharedState>, Json(payload): Json<PlayRequest>) -> (StatusCode, Json<Response>) {
    let url: String;

    if payload.collection == "" {
        url = format!("/stream/{}  ", payload.video);
    } else {
        url = format!("/stream/{}/{}", payload.collection, payload.video);
    }

    let command = Command {
        remote_address: payload.remote_address,
        message: RemoteMessage::Play{url: url.clone()}
    };

    execute_remote(&state, command).await
}


#[debug_handler]
async fn remote_command(State(state): State<SharedState>, Json(payload): Json<Command>) -> (StatusCode, Json<Response>) {
    return execute_remote(&state, payload).await;
}

async fn execute_remote(state: &SharedState, command: Command) -> (StatusCode, Json<Response>) {

    let remote_client: Arc<dyn RemotePlayer>;

    if let Ok(context) = state.read() {
        if command.remote_address.is_none() && context.remote_player.is_none() {
            return (StatusCode::BAD_REQUEST, Json(Response::error("missing remote_address".to_string())));
        }

        let key = command.remote_address.unwrap_or_else(|| String::from(""));

        match context.client_map.get(&key) {
            Some(client) => remote_client = client.clone(),
            None => match &context.remote_player {
                Some(client) => remote_client = client.clone(),
                None => return (StatusCode::BAD_REQUEST, Json(Response::error("no connection to remote_address".to_string())))
            }
        }
    } else {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error("could not lock state".to_string())));
    }

    match remote_client.execute(command.message).await {
        Ok(result) => (result, Json(Response::success("todo".to_string()))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e)))
    }
}