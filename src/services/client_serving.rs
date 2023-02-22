use std::env;
use axum::{body::{boxed, Body, BoxBody}, http::{Request, Response, StatusCode, Uri}};
use tower::util::ServiceExt;
use tower_http::services::ServeDir;
use crate::domain::CLIENT_DIR;


pub async fn file_handler(uri: Uri) -> Result<Response<BoxBody>, (StatusCode, String)> {

    let res = get_static_file(uri.clone()).await?;

    if res.status() == StatusCode::NOT_FOUND {
        match format!("{}.html", uri).parse() {
            Ok(uri_html) => get_static_file(uri_html).await,
            Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "Invalid URI".to_string())),
        }
    } else {
        Ok(res)
    }
}

async fn get_static_file(uri: Uri) -> Result<Response<BoxBody>, (StatusCode, String)> {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();

    let client_dir = env::var(CLIENT_DIR).unwrap_or(String::from("client"));

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
