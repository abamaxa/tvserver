use std::sync::Arc;

use tvserver::domain::traits::{MockTextFetcher, TextFetcher};

pub async fn get_text_fetcher(fixture: &str) -> Arc<dyn TextFetcher> {
    let mut fetcher = MockTextFetcher::new();
    let html = String::from_utf8(tokio::fs::read(fixture).await.unwrap()).unwrap();

    fetcher
        .expect_get_text()
        .returning(move |_| Ok(html.clone()));

    Arc::new(fetcher)
}
