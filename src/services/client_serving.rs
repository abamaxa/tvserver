use std::env;
use axum::{body::{boxed, Body, BoxBody}, http::{Request, Response, StatusCode, Uri}};
use tower::util::ServiceExt;
use tower_http::services::ServeDir;
use crate::domain::CLIENT_DIR;

pub async fn app_handler(uri: Uri) -> Result<Response<BoxBody>, (StatusCode, String)> {
    let mut client_dir = env::var(CLIENT_DIR).unwrap_or(String::from("client"));
    client_dir.push_str("/app");
    file_handler(client_dir, uri).await
}

pub async fn player_handler(uri: Uri) -> Result<Response<BoxBody>, (StatusCode, String)> {
    let mut client_dir = env::var(CLIENT_DIR).unwrap_or(String::from("client"));
    client_dir.push_str("/player");
    file_handler(client_dir, uri).await
}

async fn file_handler(client_dir: String, uri: Uri) -> Result<Response<BoxBody>, (StatusCode, String)> {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();

    let serve_dir = ServeDir::new(client_dir);
    // `ServeDir` implements `tower::Service` so we can call it with `tower::ServiceExt::oneshot`
    // When run normally, the root is the workspace root
    match serve_dir.oneshot(req).await {
        Ok(res) => Ok(res.map(boxed)),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", err),
        )),
    }
}
