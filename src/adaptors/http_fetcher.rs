use crate::domain::config::get_openai_api_key;
use crate::domain::messages::{ChatGPTRequest, ChatGPTResponse};
use crate::domain::models::YoutubeResponse;
use crate::domain::traits::{JsonFetcher, TextFetcher};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::StatusCode;
use serde::Serialize;

const ACCESS_DENIED_MSG: &str = "Access denied, ensure access tokens have been set";

#[derive(Default)]
pub struct HTTPClient {
    client: reqwest::Client,
}

impl HTTPClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl<'a, Q: Serialize + Sync + Send + 'a> JsonFetcher<'a, YoutubeResponse, Q> for HTTPClient {
    async fn fetch(&self, url: &str, query: &'a Q) -> anyhow::Result<YoutubeResponse> {
        let response = self
            .client
            .get(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .query(query)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => Ok(response.json::<YoutubeResponse>().await?),
            StatusCode::UNAUTHORIZED => Err(anyhow!(ACCESS_DENIED_MSG)),
            _ => Err(anyhow!("Error code {}", response.status())),
        }
    }
}

#[async_trait]
impl<'a> JsonFetcher<'a, ChatGPTResponse, ChatGPTRequest> for HTTPClient {
    async fn fetch(&self, url: &str, query: &'a ChatGPTRequest) -> anyhow::Result<ChatGPTResponse> {
        let api_key = get_openai_api_key();

        let response = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", api_key))
            .json(query)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => Ok(response.json::<ChatGPTResponse>().await?),
            StatusCode::UNAUTHORIZED => Err(anyhow!(ACCESS_DENIED_MSG)),
            _ => Err(anyhow!(
                "Error code {}: {:?}",
                response.status(),
                response.text().await
            )),
        }
    }
}

#[async_trait]
impl TextFetcher for HTTPClient {
    async fn get_text(&self, url: &str) -> Result<String> {
        Ok(self.client.get(url).send().await?.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::messages::ChatGPTMessage;
    use crate::domain::models::YoutubeResponse;
    use axum::extract::Query;
    use axum::routing::get;
    use axum::{Json, Router};
    use std::collections::HashMap;
    use tokio::task::JoinHandle;
    use tokio::time;

    #[tokio::test]
    async fn test_get_text() -> Result<()> {
        const HOST_ADDR: &str = "127.0.0.1:35093";
        const TEST_TEXT: &str = "Test Text";
        let app = Router::new().route("/", get(|| async { TEST_TEXT }));

        let http_server = setup_http_server(app, HOST_ADDR).await;

        let client: &dyn TextFetcher = &HTTPClient::new();

        let text = client.get_text(&format!("http://{}/", HOST_ADDR)).await?;

        http_server.abort();

        assert_eq!(text, TEST_TEXT);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_json() -> Result<()> {
        const HOST_ADDR: &str = "127.0.0.1:35094";
        const KIND_PARAM: &str = "this is the kind param";
        const ETAG_PARAM: &str = "this is the etag param";
        const REGION_PARAM: &str = "this is the region param";

        let app = Router::new().route(
            "/",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                Json(YoutubeResponse {
                    kind: params.get("KIND_PARAM").unwrap().to_owned(),
                    etag: params.get("ETAG_PARAM").unwrap().to_owned(),
                    next_page_token: "".to_string(),
                    region_code: params.get("REGION_PARAM").unwrap().to_owned(),
                    page_info: Default::default(),
                    items: vec![],
                })
            }),
        );

        let http_server = setup_http_server(app, HOST_ADDR).await;

        let client: &dyn JsonFetcher<'_, YoutubeResponse, &[(&str, &str)]> = &HTTPClient::new();

        let query = [
            ("ETAG_PARAM", ETAG_PARAM),
            ("KIND_PARAM", KIND_PARAM),
            ("REGION_PARAM", REGION_PARAM),
        ];

        let q: &[(&str, &str)] = &query;

        let result = client.fetch(&format!("http://{}/", HOST_ADDR), &q).await?;

        http_server.abort();

        assert_eq!(result.etag, ETAG_PARAM);
        assert_eq!(result.kind, KIND_PARAM);
        assert_eq!(result.region_code, REGION_PARAM);

        Ok(())
    }

    #[tokio::test]
    async fn test_access_denied() -> Result<()> {
        const HOST_ADDR: &str = "127.0.0.1:35095";
        let app = Router::new().route("/", get(|| async { StatusCode::UNAUTHORIZED }));

        let http_server = setup_http_server(app, HOST_ADDR).await;

        let client: &dyn JsonFetcher<'_, YoutubeResponse, &[(&str, &str)]> = &HTTPClient::new();

        let result = client
            .fetch(&format!("http://{}/", HOST_ADDR), &(&[] as &[(&str, &str)]))
            .await;

        http_server.abort();

        assert!(result.is_err());
        assert_eq!(result.err().unwrap().to_string(), ACCESS_DENIED_MSG);

        Ok(())
    }

    #[tokio::test]
    async fn test_general_error() -> Result<()> {
        const HOST_ADDR: &str = "127.0.0.1:35096";
        const ERROR_CODE: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;

        let app = Router::new().route("/", get(|| async { ERROR_CODE }));

        let http_server = setup_http_server(app, HOST_ADDR).await;

        let client: &dyn JsonFetcher<'_, YoutubeResponse, &[(&str, &str)]> = &HTTPClient::new();

        let result = client
            .fetch(&format!("http://{}/", HOST_ADDR), &(&[] as &[(&str, &str)]))
            .await;

        http_server.abort();

        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            format!("Error code {}", ERROR_CODE)
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_server_down() -> Result<()> {
        let client: &dyn TextFetcher = &HTTPClient::new();

        let result = client.get_text("http://localhost:60232/").await;

        assert!(result.is_err());

        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_chatgpt() -> Result<()> {
        let client: &dyn JsonFetcher<'_, ChatGPTResponse, ChatGPTRequest> = &HTTPClient::new();

        let request = ChatGPTRequest {
            model: String::from("gpt-4"),
            messages: vec![ChatGPTMessage {
                role: "user".to_string(),
                content: "Hello!".to_string(),
            }],
            ..Default::default()
        };

        let results = client
            .fetch("https://api.openai.com/v1/chat/completions", &request)
            .await?;

        println!("{:?}", results);

        Ok(())
    }

    async fn setup_http_server(app: Router, host: &'static str) -> JoinHandle<Result<()>> {
        let task = tokio::spawn(async move {
            axum::Server::bind(&host.parse().unwrap())
                .serve(app.into_make_service())
                .await?;
            Ok(())
        });

        // wait for the server to come up
        time::sleep(time::Duration::from_millis(100)).await;

        task
    }
}
