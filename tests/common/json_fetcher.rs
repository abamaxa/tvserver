use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use serde::Serialize;
use tokio::fs;
use tvserver::domain::models::YoutubeResponse;
use tvserver::domain::traits::JsonFetcher;

#[derive(Default, Debug, Clone)]
pub struct MockFetcher {
    response: Option<YoutubeResponse>,
    error: Option<String>,
}

#[async_trait]
impl<'a, Q: Serialize + Sync + Send + 'a> JsonFetcher<'a, YoutubeResponse, Q> for MockFetcher {
    async fn fetch(&self, _: &str, _: &'a Q) -> anyhow::Result<YoutubeResponse> {
        match &self.response {
            Some(response) => Ok(response.clone()),
            None => match &self.error {
                Some(error) => Err(anyhow!(error.clone())),
                None => panic!("must set either response or error"),
            },
        }
    }
}

type TestJsonFetcher = Arc<dyn for<'a> JsonFetcher<'a, YoutubeResponse, &'a [(&'a str, &'a str)]>>;

pub async fn get_json_fetcher(fixture: &str) -> TestJsonFetcher {
    let response = Some(results_from_fixture(fixture).await);
    Arc::new(MockFetcher {
        response,
        error: None,
    })
}

async fn results_from_fixture(name: &str) -> YoutubeResponse {
    let mut path = PathBuf::from("tests/fixtures");

    path.push(name);

    let data = fs::read(&path).await.unwrap();

    let result: YoutubeResponse = serde_json::from_slice(&data).unwrap();

    result
}
