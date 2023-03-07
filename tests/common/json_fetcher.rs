use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use tokio::fs;
use tvserver::domain::models::YoutubeResponse;
use tvserver::domain::traits::JsonFetcher;

#[derive(Default, Debug, Clone)]
pub struct MockFetcher {
    response: Option<YoutubeResponse>,
    error: Option<String>,
}

#[async_trait]
impl JsonFetcher<YoutubeResponse> for MockFetcher {
    async fn get_json(&self, _: &str, _: &[(&str, &str)]) -> anyhow::Result<YoutubeResponse> {
        match &self.response {
            Some(response) => Ok(response.clone()),
            None => match &self.error {
                Some(error) => Err(anyhow!(error.clone())),
                None => panic!("must set either response or error"),
            },
        }
    }
}

pub async fn get_json_fetcher(fixture: &str) -> Arc<dyn JsonFetcher<YoutubeResponse>> {
    let response = Some(results_from_fixture(fixture).await);
    Arc::new(MockFetcher {
        response,
        error: None,
    })
}

async fn results_from_fixture(name: &str) -> YoutubeResponse {
    let path = Path::new(name);

    let data = fs::read(&path).await.unwrap();

    let result: YoutubeResponse = serde_json::from_slice(&data).unwrap();

    result
}
