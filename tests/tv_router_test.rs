#![cfg(feature = "webserver")]

use std::{
    net::SocketAddr,
    sync::{Arc, Once},
};

use app_lib::{
    adaptors::{SqlRepository, TokioProcessSpawner},
    domain::{
        messagebus::{LocalMessageExchange, MessageExchange, MessageFilter},
        models::{SeriesDetails, VideoDetails, VideoMetadata},
        traits::{MockMediaChecker, MockMediaStorer, Repository},
    },
    entrypoints::{webserver::build_http_router, BookRuntime, Context, BOOK_LIBRARY_UNAVAILABLE},
    services::{SearchService, TaskManager},
};
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
    Router,
};
use scraper::{Html, Selector};
use tower::ServiceExt;

async fn test_router(repository: Repository) -> Router {
    static CONFIG: Once = Once::new();
    CONFIG.call_once(|| std::env::set_var("MOVIE_DIR", "tests/fixtures/media_dir"));
    let tasks = Arc::new(TaskManager::new(Arc::new(TokioProcessSpawner::new())));
    let search = SearchService::new(tasks.clone(), Vec::new(), repository.clone());
    let exchange = LocalMessageExchange::new();
    let messenger = MessageExchange::new(
        exchange.new_sender(),
        exchange
            .listen_for_messages(MessageFilter::All)
            .await
            .unwrap(),
    );
    build_http_router(Context::new(
        Arc::new(MockMediaStorer::new()),
        search,
        messenger,
        tasks,
        repository,
        Arc::new(MockMediaChecker::new()),
        exchange,
        BookRuntime::Unavailable {
            message: Arc::from(BOOK_LIBRARY_UNAVAILABLE),
        },
        None,
    ))
    .unwrap()
}

fn video(checksum: i64, title: &str, season: &str, episode: &str) -> VideoDetails {
    VideoDetails {
        checksum,
        collection: "Folder & more/Nested #1".to_string(),
        video: format!("Episode {episode} #1 & \"two\" % + ?-{checksum}.mp4"),
        series: SeriesDetails {
            series_title: title.to_string(),
            season: season.to_string(),
            episode: episode.to_string(),
            episode_title: format!("The <adventure> & \"story\" {episode}"),
        },
        description: "A <script>alert('bad')</script> & a story".to_string(),
        thumbnail: vec!["Cover & #1?.jpg".to_string()],
        metadata: VideoMetadata {
            duration: 1501.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn get(app: &Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .extension(ConnectInfo("127.0.0.1:12345".parse::<SocketAddr>().unwrap()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(
        Html::parse_document(&body)
            .select(&Selector::parse("script").unwrap())
            .count(),
        0
    );
    (status, body)
}

fn items(body: &str) -> Vec<(String, String)> {
    Html::parse_document(body)
        .select(&Selector::parse("a.item").unwrap())
        .map(|item| {
            let title = item
                .select(&Selector::parse(".item-title").unwrap())
                .next()
                .unwrap()
                .text()
                .collect::<String>();
            (title, item.value().attr("href").unwrap().to_string())
        })
        .collect()
}

fn link_url(body: &str, label: &str) -> String {
    Html::parse_document(body)
        .select(&Selector::parse("a").unwrap())
        .find(|link| link.text().collect::<String>() == label)
        .unwrap_or_else(|| panic!("Missing link: {label}"))
        .value()
        .attr("href")
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn browse_shows_seasons_episodes_and_native_playback_using_only_links() {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    let title = "Kids & <fun> / \"TV\" + 100% ?";
    repository
        .save_video(&video(1, title, "2", "10"))
        .await
        .unwrap();
    let first = video(2, title, "2", "2");
    repository.save_video(&first).await.unwrap();
    repository
        .save_video(&video(3, title, "10", "1"))
        .await
        .unwrap();
    let app = test_router(repository).await;

    let (status, root) = get(&app, "/tv").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items(&root)[0].0, title);
    assert!(!root.contains("<fun>"));
    let thumbnail = Html::parse_document(&root)
        .select(&Selector::parse("img").unwrap())
        .next()
        .unwrap()
        .value()
        .attr("src")
        .unwrap()
        .to_string();
    assert_eq!(thumbnail, "/api/thumbnails/Cover%20%26%20%231%3F.jpg");

    let (status, show) = get(&app, &items(&root)[0].1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        items(&show)
            .iter()
            .map(|item| item.0.as_str())
            .collect::<Vec<_>>(),
        ["Season 2", "Season 10"]
    );
    let (status, season) = get(&app, &items(&show)[0].1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        items(&season)
            .iter()
            .map(|item| item.1.as_str())
            .collect::<Vec<_>>(),
        ["/tv/watch/2", "/tv/watch/1"]
    );
    assert!(season.contains("26 min"));

    let (status, watch) = get(&app, &items(&season)[0].1).await;
    assert_eq!(status, StatusCode::OK);
    let doc = Html::parse_document(&watch);
    let player = doc
        .select(&Selector::parse("video").unwrap())
        .next()
        .unwrap();
    assert!(player.value().attr("controls").is_some());
    assert!(player.value().attr("muted").is_none());
    let src = player.value().attr("src").unwrap();
    assert_eq!(src, "/api/stream/Folder%20%26%20more/Nested%20%231/Episode%202%20%231%20%26%20%22two%22%20%25%20%2B%20%3F-2.mp4");
    assert_eq!(link_url(&watch, "Open video directly"), src);
    assert_eq!(link_url(&watch, "Next episode"), "/tv/watch/1");
    assert!(!watch.contains("<script>"));

    let (_, back) = get(&app, &link_url(&watch, "Back to episodes")).await;
    assert_eq!(items(&back), items(&season));
    let (_, next) = get(&app, &link_url(&watch, "Next episode")).await;
    assert_eq!(link_url(&next, "Previous episode"), "/tv/watch/2");
    assert!(!next.contains(">Next episode</a>"));
}

#[tokio::test]
async fn numeric_and_empty_show_titles_and_mixed_seasons_remain_browsable() {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    // A numeric show title must not be interpreted as another video's checksum.
    repository
        .save_video(&video(123, "Different show", "1", "1"))
        .await
        .unwrap();
    repository
        .save_video(&video(4, "123", "", ""))
        .await
        .unwrap();
    repository.save_video(&video(5, "", "", "")).await.unwrap();
    for (id, season) in [(6, ""), (7, "Specials"), (8, "2"), (9, "10")] {
        repository
            .save_video(&video(id, "Mixed", season, "1"))
            .await
            .unwrap();
    }
    let app = test_router(repository).await;
    let (_, numeric) = get(&app, "/tv/show?title=123").await;
    assert_eq!(items(&numeric)[0].1, "/tv/watch/4");
    assert!(!numeric.contains("<h2>Seasons</h2>"));
    let (_, empty) = get(&app, "/tv/show?title=").await;
    assert!(empty.contains("Other videos"));
    assert_eq!(items(&empty)[0].1, "/tv/watch/5");
    let (_, mixed) = get(&app, "/tv/show?title=Mixed").await;
    let seasons = items(&mixed);
    assert_eq!(
        seasons
            .iter()
            .map(|item| item.0.as_str())
            .collect::<Vec<_>>(),
        ["Season 2", "Season 10", "Other episodes", "Specials"]
    );
    for (index, checksum) in [(2, 6), (3, 7)] {
        let (_, episodes) = get(&app, &seasons[index].1).await;
        assert_eq!(items(&episodes)[0].1, format!("/tv/watch/{checksum}"));
    }
}

#[tokio::test]
async fn search_and_pagination_preserve_filters_and_episode_back_links() {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    for index in 1..=22 {
        repository
            .save_video(&video(index, &format!("Test & show {index:02}"), "1", "1"))
            .await
            .unwrap();
        repository
            .save_video(&video(100 + index, "Long show", "1", &index.to_string()))
            .await
            .unwrap();
    }
    let app = test_router(repository).await;
    let (_, first) = get(&app, "/tv?q=TEST%20%26").await;
    assert_eq!(items(&first).len(), 20);
    let next_url = link_url(&first, "Next page");
    assert!(next_url.contains("q=TEST%20%26"));
    let (_, second) = get(&app, &next_url).await;
    assert_eq!(items(&second).len(), 2);
    assert_eq!(items(&second)[0].0, "Test & show 21");
    let (_, previous) = get(&app, &link_url(&second, "Previous page")).await;
    assert_eq!(items(&first), items(&previous));
    let (_, no_match) = get(&app, "/tv?q=missing").await;
    assert!(items(&no_match).is_empty());
    assert!(no_match.contains("No shows match"));

    let (_, first) = get(&app, "/tv/show?title=Long%20show&season=1&page=0").await;
    assert_eq!(items(&first).len(), 20);
    let (_, second) = get(&app, &link_url(&first, "Next page")).await;
    assert_eq!(items(&second).len(), 2);
    assert_eq!(items(&second)[0].1, "/tv/watch/121");
    let (_, watch) = get(&app, &items(&second)[0].1).await;
    let back_url = link_url(&watch, "Back to episodes");
    assert!(back_url.ends_with("&page=2"));
    let (_, back) = get(&app, &back_url).await;
    assert_eq!(items(&back), items(&second));
    let (_, clamped) = get(
        &app,
        "/tv/show?title=Long%20show&season=1&page=18446744073709551615",
    )
    .await;
    assert_eq!(items(&clamped), items(&second));
}

#[tokio::test]
async fn empty_library_and_missing_items_return_readable_html() {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    repository
        .save_video(&video(1, "Show", "1", "1"))
        .await
        .unwrap();
    let app = test_router(repository.clone()).await;
    for uri in ["/tv/show?title=Missing", "/tv/show?title=Show&season=missing", "/tv/watch/999"] {
        let (status, body) = get(&app, uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(link_url(&body, "Back to all shows"), "/tv");
    }
    repository.delete_video(1).await.unwrap();
    let (status, body) = get(&app, "/tv/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("No videos yet"));
}

#[tokio::test]
async fn tv_pages_and_embedded_styles_use_existing_access_controls() {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    let app = test_router(repository).await;
    for uri in ["/tv", "/tv/", "/tv/show?title=Show", "/tv/watch/1", "/tv/styles.css"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .extension(ConnectInfo("203.0.113.1:12345".parse::<SocketAddr>().unwrap()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/tv/styles.css")
                .extension(ConnectInfo("127.0.0.1:12345".parse::<SocketAddr>().unwrap()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    assert!(response.headers().contains_key("content-security-policy"));
}

#[tokio::test]
async fn native_player_uses_existing_range_capable_stream() {
    let repository: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
    let mut fixture = video(1, "Fixture", "", "");
    fixture.video = "test.mp4".to_string();
    fixture.collection.clear();
    repository.save_video(&fixture).await.unwrap();
    let app = test_router(repository).await;
    let (_, watch) = get(&app, "/tv/watch/1").await;
    let stream = link_url(&watch, "Open video directly");
    let response = app
        .oneshot(
            Request::builder()
                .uri(stream)
                .header(header::RANGE, "bytes=0-31")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "32");
    assert!(response.headers()[header::CONTENT_RANGE]
        .to_str()
        .unwrap()
        .starts_with("bytes 0-31/"));
}
