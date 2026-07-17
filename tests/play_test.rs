#![cfg(feature = "webserver")]

mod common;

use crate::common::{get_media_store, get_pirate_search, get_repository, get_task_manager};
use anyhow::Result;
use app_lib::domain::config::MOVIE_DIR;
use app_lib::domain::messagebus::{LocalMessageExchange, MessageExchange, MessageFilter};
use app_lib::domain::messages::PlayRequest;
use app_lib::domain::traits::MockBookChecker;
use app_lib::{domain::messages::Response, entrypoints};
use common::get_checker;
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

#[tokio::test]
async fn test_remote_play() -> Result<()> {
    env::set_var(MOVIE_DIR, "tests/fixtures/media_dir");

    let local_exchange = LocalMessageExchange::new();
    let exchange = MessageExchange::new(
        local_exchange.new_sender(),
        local_exchange
            .listen_for_messages(MessageFilter::All)
            .await?,
    );

    let key = SocketAddr::from_str("0.0.0.0:456").unwrap();

    exchange
        .add_player(key.to_string(), common::get_remote_player())
        .await;

    let searcher = get_pirate_search("torrents_get.json", "pb_search.html").await;

    let repository = get_repository().await;
    let (book_store, book_file_storer) = common::get_book_services(repository.clone());
    let context = entrypoints::Context::new(
        get_media_store(),
        searcher,
        exchange,
        get_task_manager(),
        repository,
        get_checker(),
        Arc::new(MockBookChecker::new()),
        local_exchange,
        book_store,
        book_file_storer,
        None,
    );

    let server = common::create_server(context, 57182).await;

    let client = reqwest::Client::new();

    let request = PlayRequest {
        collection: "".to_string(),
        video: "test.mp4".to_string(),
        remote_address: None,
        width: 1920,
        height: 1080,
        aspect_width: 1920,
        aspect_height: 1080,
        video_id: (1234567890 as i64).to_string(),
        metadata: None,
    };

    let body = client
        .post("http://localhost:57182/api/remote/play")
        .json(&request)
        .send()
        .await?
        .text()
        .await?;

    let response: Response = serde_json::from_str(&body)?;

    assert!(response.errors.is_empty());
    assert_eq!(response.message, "success");

    Ok(server.abort())
}
