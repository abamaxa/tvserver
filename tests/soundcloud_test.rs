#![cfg(feature = "webserver")]

mod common;

use std::sync::Arc;

use app_lib::domain::models::{DownloadableItem, SearchResults};
use app_lib::domain::traits::{MediaSearcher, MockDownload};
use app_lib::domain::SearchEngineType;
use app_lib::entrypoints::api;
use app_lib::services::SearchEngine;
use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

struct FixtureSoundcloud;

#[async_trait]
impl MediaSearcher<DownloadableItem> for FixtureSoundcloud {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<DownloadableItem>> {
        if query == "unavailable" {
            anyhow::bail!("SoundCloud unavailable");
        }
        Ok(SearchResults::success(vec![DownloadableItem {
            title: query.into(),
            description: "Artist".into(),
            link: "https://soundcloud.com/artist/track".into(),
            engine: SearchEngineType::SoundCloud,
        }]))
    }
}

#[tokio::test]
async fn soundcloud_http_search_dispatches_and_reports_errors() -> anyhow::Result<()> {
    let searcher = common::get_search_service(SearchEngine::new(
        SearchEngineType::SoundCloud,
        Arc::new(FixtureSoundcloud),
        Arc::new(MockDownload::new()),
    ))
    .await;
    let context = common::get_context(
        common::get_media_store(),
        searcher,
        common::get_task_manager(),
        common::get_repository().await,
        common::get_checker(),
    )
    .await?;
    let router = api::register(Arc::new(context));

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/search/soundcloud?q=jazz%20%26%20caf%C3%A9%20%2B%20100%25")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    let results: SearchResults<DownloadableItem> = serde_json::from_slice(&body)?;
    assert!(results.error.is_none());
    let track = &results.results.unwrap()[0];
    assert_eq!(track.title, "jazz & café + 100%");
    assert_eq!(track.link, "https://soundcloud.com/artist/track");
    assert_eq!(track.engine, SearchEngineType::SoundCloud);

    for (path, status, error) in [
        (
            "/api/search/soundcloud",
            StatusCode::BAD_REQUEST,
            "missing q parameter",
        ),
        (
            "/api/search/soundcloud?q=unavailable",
            StatusCode::INTERNAL_SERVER_ERROR,
            "SoundCloud unavailable",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), status);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let result: SearchResults<DownloadableItem> = serde_json::from_slice(&body)?;
        assert!(result.results.is_none());
        assert_eq!(result.error.as_deref(), Some(error));
    }
    Ok(())
}
