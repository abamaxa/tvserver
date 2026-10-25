#![cfg(feature = "webserver")]

mod common;

use app_lib::{entrypoints::register, services::SearchService};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn download_routes_accept_source_payloads_and_report_validation_errors_as_json() {
    let repository = common::get_repository().await;
    let tasks = common::get_task_manager();
    let search = SearchService::new(tasks.clone(), Vec::new(), repository.clone());
    let context = common::get_context(
        common::get_media_store(),
        search,
        tasks,
        repository,
        common::get_checker(),
    )
    .await
    .unwrap();
    let app = register(Arc::new(context));

    for (method, path, payload, message) in [
        (
            "POST",
            "/api/download-source",
            json!({ "hostUrl": "ftp://example.com", "collection": "" }),
            "HTTP or HTTPS",
        ),
        (
            "POST",
            "/api/download-source/thumbnail",
            json!({ "hostUrl": "https://beta.qcc.cc", "path": "https://other.example/image.jpg", "username": "viewer", "password": "secret" }),
            "selected host",
        ),
        (
            "PATCH",
            "/api/tasks",
            json!({ "hostUrl": "https://beta.qcc.cc", "videos": [] }),
            "at least one video",
        ),
        (
            "PATCH",
            "/api/tasks",
            json!({ "hostUrl": "https://beta.qcc.cc", "videos": [{ "checksum": "123", "collection": "../escape", "video": "clip.mp4" }] }),
            "Invalid library path",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method} {path}");
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["errors"][0].as_str().unwrap().contains(message), "{body}");
        assert!(!body.to_string().contains("secret"));
    }
}
