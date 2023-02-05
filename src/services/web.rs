use crate::adaptors::{filestore::{VideoEntry, VideoStore}, player::Player};
use crate::domain::events::{Command, PlayRequest, Response};

use std::sync::Arc;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    http::StatusCode,
    response::IntoResponse,
    Json, Router};
use axum_macros::debug_handler;
use crate::services::video_serving::video;

#[derive(Clone)]
pub struct Context {
    player: Arc<dyn Player>,
    store: Arc<dyn VideoStore>,
}

pub fn register(player: Arc<dyn Player>, store: Arc<dyn VideoStore>) -> Router {
    Router::new()
        .route("/collections", get(list_collections))
        .route("/videos/*collection", get(list_videos))
        .route("/play", post(play))
        .route("/remote", post(handle_command))
        .route("/stream/*path", get(video))
        .with_state(Context{player, store})
}

#[debug_handler]
async fn handle_command(State(context): State<Context>, Json(payload): Json<Command>) -> (StatusCode, Json<Response>) {
    // curl -v http://localhost:4000/remote/volume-up
    match context.player.send_command(&payload.command, 0) {
        Ok(result) => (StatusCode::OK, Json(Response::success(result))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e)))
    }
}

async fn play(State(context): State<Context>, Json(payload): Json<PlayRequest>) -> (StatusCode, Json<Response>) {

    let command = format!(
        "add file://{}", context.store.as_path(payload.collection, payload.video)
    );

    if let Err(err) = context.player.send_command("clear", 1) {
        println!("{:?}", err);
    }

    match context.player.send_command(&command, 0) {
        Ok(result) => (StatusCode::OK, Json(Response::success(result))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(Response::error(e)))
    }
}

async fn list_collections(State(context): State<Context>) -> impl IntoResponse {

    match context.store.list("".to_string()) {
        Ok(result) => (StatusCode::OK, Json(result)),
        Err(e) => (StatusCode::NOT_FOUND, Json(VideoEntry::error(e.to_string())))
    }
}

async fn list_videos(State(context): State<Context>, Path(collection): Path<String>) -> (StatusCode, Json<VideoEntry>) {

    match context.store.list(collection) {
        Ok(result) => (StatusCode::OK, Json(result)),
        Err(e) => (StatusCode::NOT_FOUND, Json(VideoEntry::error(e.to_string())))
    }
}
