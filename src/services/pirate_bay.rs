use crate::domain::config::get_pirate_bay_url;
use crate::domain::models::{DownloadableItem, SearchResults};
use crate::domain::traits::{MediaSearcher, TextFetcher};
use crate::domain::SearchEngineType::Torrent;
use anyhow;
use async_trait::async_trait;
use chrono::DateTime;
use html_escape::decode_html_entities;
use mockall::lazy_static;
use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use std::sync::Arc;
use tracing::debug;
use urlencoding::decode;

// Debug logging for this module can be enabled at runtime without recompilation:
//
// Option 1: Enable debug for just this module:
//   RUST_LOG=app_lib::services::pirate_bay=debug cargo run
//
// Option 3: Mixed log levels with this module at debug:
//   RUST_LOG=info,app_lib::services::pirate_bay=debug cargo run
//
// You can also target specific functions:
//   RUST_LOG=info,app_lib::services::pirate_bay::parse_item=debug cargo run
//
// Or enable debug for all services:
//   RUST_LOG=app_lib::services=debug cargo run

pub type PirateFetcher = Arc<dyn TextFetcher>;

lazy_static! {
    static ref SELECTOR: Selector = Selector::parse(r#"#searchResult"#).unwrap();
    static ref TR_SELECTOR: Selector = Selector::parse("tr").unwrap();
    static ref TD_SELECTOR: Selector = Selector::parse("td").unwrap();
    static ref LINK_SELECTOR: Selector = Selector::parse("a").unwrap();
    static ref DESC_SELECTOR: Selector = Selector::parse(".detDesc").unwrap();
}

pub struct PirateClient {
    host: Url,
    client: PirateFetcher,
}

#[async_trait]
impl MediaSearcher<DownloadableItem> for PirateClient {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<DownloadableItem>> {
        let url = self.search_url(query);

        let body = self.client.get_text(url.as_str()).await?;

        match self.parse_search(body) {
            Some(results) => Ok(SearchResults::success(results)),
            None => Ok(SearchResults::error("could not parse results")),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ApiBayItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    info_hash: String,
    #[serde(default)]
    leechers: String,
    #[serde(default)]
    seeders: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    added: String,
}

impl PirateClient {
    pub fn new(client: PirateFetcher, host: Option<Url>) -> Self {
        Self {
            host: host.unwrap_or(get_pirate_bay_url()),
            client,
        }
    }

    fn search_url(&self, query: &str) -> Url {
        if self.uses_api() {
            return self.api_search_url(query);
        }

        self.legacy_html_search_url(query)
    }

    fn uses_api(&self) -> bool {
        self.host.domain() == Some("apibay.org")
    }

    fn api_search_url(&self, query: &str) -> Url {
        let mut url = self.host.clone();
        url.set_query(None);

        match query {
            "top-100" => url.set_path("precompiled/data_top100_all.json"),
            "top-videos" => url.set_path("precompiled/data_top100_200.json"),
            "top-books" => url.set_path("precompiled/data_top100_601.json"),
            "top-music" => url.set_path("precompiled/data_top100_100.json"),
            _ => {
                url.set_path("q.php");
                url.query_pairs_mut()
                    .append_pair("q", query)
                    .append_pair("cat", "0");
            }
        }

        url
    }

    fn legacy_html_search_url(&self, query: &str) -> Url {
        let mut url = self.host.clone();
        url.set_query(None);

        let segments = match query {
            "top-100" => vec!["top", "all"],
            "top-videos" => vec!["top", "200"],
            "top-books" => vec!["top", "601"],
            "top-music" => vec!["top", "100"],
            _ => vec!["search", query, "1", "99", "0"],
        };

        url.path_segments_mut()
            .expect("pirate bay host must be a base URL")
            .clear()
            .extend(segments);

        url
    }

    fn parse_search(&self, body: String) -> Option<Vec<DownloadableItem>> {
        Self::parse_api_search(&body).or_else(|| self.parse_html_search(body))
    }

    fn parse_api_search(body: &str) -> Option<Vec<DownloadableItem>> {
        let items: Vec<ApiBayItem> = serde_json::from_str(body).ok()?;
        Some(items.into_iter().filter_map(Self::parse_api_item).collect())
    }

    fn parse_api_item(item: ApiBayItem) -> Option<DownloadableItem> {
        if item.id == "0" || item.name.is_empty() || item.info_hash.is_empty() {
            return None;
        }

        let seeders = item.seeders.parse::<i32>().unwrap_or_default();
        if seeders == 0 {
            return None;
        }

        let leechers = item.leechers.parse::<i32>().unwrap_or_default();
        let title = decode_html_entities(&item.name).to_string();

        Some(DownloadableItem {
            title: title.replace('.', " "),
            description: Self::api_description(&item, seeders, leechers),
            link: format!(
                "magnet:?xt=urn:btih:{}&dn={}",
                item.info_hash,
                urlencoding::encode(&item.name)
            ),
            engine: Torrent,
        })
    }

    fn api_description(item: &ApiBayItem, seeders: i32, leechers: i32) -> String {
        let uploaded = item
            .added
            .parse::<i64>()
            .ok()
            .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
            .map(|datetime| datetime.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let size = item
            .size
            .parse::<u64>()
            .map(Self::format_size)
            .unwrap_or_else(|_| "unknown".to_string());

        format!(
            "Uploaded {}, Size {}, Seeders {}, Leechers {}, ULed by {}",
            uploaded, size, seeders, leechers, item.username
        )
    }

    fn format_size(size: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

        let mut value = size as f64;
        let mut unit = 0;

        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }

        if unit == 0 {
            format!("{} {}", size, UNITS[unit])
        } else {
            format!("{:.2} {}", value, UNITS[unit])
        }
    }

    fn parse_html_search(&self, html: String) -> Option<Vec<DownloadableItem>> {
        let document = Html::parse_document(&html);

        let table = document.select(&SELECTOR).next()?;

        Some(
            table
                .select(&TR_SELECTOR)
                .filter_map(Self::parse_item)
                .collect(),
        )
    }

    fn parse_item(row: ElementRef) -> Option<DownloadableItem> {
        let mut record = DownloadableItem {
            engine: Torrent,
            ..Default::default()
        };
        let mut seeders: i32 = 0;

        // Debug: print the entire row HTML
        debug!("Parsing row HTML: {}", row.html());

        for (idx, cell) in row.select(&TD_SELECTOR).enumerate() {
            // Debug: print cell content
            debug!("Cell {} content: {}", idx, cell.html());

            match idx {
                0 => {
                    // Category - skip
                }
                1 => {
                    // Cell 1 now contains title, magnet link, and description (date, size, uploader)
                    // Get title from <div class="detName"> -> <a class="detLink">
                    if let Some(title_link) = cell.select(&LINK_SELECTOR).find(|elem| {
                        elem.value()
                            .attr("class")
                            .map(|c| c.contains("detLink"))
                            .unwrap_or(false)
                    }) {
                        let title = title_link.text().collect::<Vec<_>>();
                        record.title = (*title.first()?).replace('.', " ");
                        debug!("Found title: {}", record.title);
                    }

                    // Get magnet link
                    if let Some(magnet_link) = cell.select(&LINK_SELECTOR).find(|elem| {
                        elem.value()
                            .attr("href")
                            .map(|href| href.starts_with("magnet:"))
                            .unwrap_or(false)
                    }) {
                        let link = decode(magnet_link.value().attr("href").unwrap())
                            .unwrap_or_else(|_| String::new().into())
                            .to_string();
                        record.link = link;
                        debug!("Found magnet link");
                    } else {
                        debug!("No magnet link found in cell 1");
                    }

                    // Get description from <font class="detDesc">
                    if let Some(desc_elem) = cell.select(&DESC_SELECTOR).next() {
                        let desc_text = PirateClient::get_element_text(&desc_elem);
                        record.description = desc_text.replace('\u{a0}', " ");
                        debug!("Found description: {}", record.description);
                    }
                }
                2 => {
                    // Seeders
                    seeders = PirateClient::get_element_i32(&cell).unwrap_or(0);
                    debug!("Seeders: {}", seeders);
                }
                3 => {
                    // Leechers
                    // let leechers = PirateClient::get_element_i32(&cell).unwrap_or(0);
                }
                _ => continue,
            }
        }

        // Skip results with no seeders or no magnet link
        match (seeders, record.link.is_empty()) {
            (0, _) => {
                debug!("Skipping item with 0 seeders");
                None
            }
            (_, true) => {
                debug!("Skipping item '{}' with no magnet link", record.title);
                None
            }
            _ => {
                debug!(
                    "Successfully parsed item: {} (seeders: {})",
                    record.title, seeders
                );
                Some(record)
            }
        }
    }

    fn get_element_i32(cell: &ElementRef) -> Option<i32> {
        match PirateClient::get_element_text(cell).parse::<i32>() {
            Ok(value) => Some(value),
            Err(_) => None,
        }
    }

    fn get_element_text(cell: &ElementRef) -> String {
        cell.text().collect::<Vec<_>>().join("").trim().to_string()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    // use crate::adaptors::HTTPClient;
    use crate::domain::traits::MockTextFetcher;
    use anyhow::Result;

    #[tokio::test]
    async fn test_search() -> Result<()> {
        let mut fetcher = MockTextFetcher::new();
        let html = String::from_utf8(tokio::fs::read("tests/fixtures/pb_search.html").await?)?;

        fetcher
            .expect_get_text()
            .returning(move |_| Ok(html.clone()));

        let pc = PirateClient::new(Arc::new(fetcher), None);

        let response = pc.search("Dragons Den").await?;

        assert!(response.error.is_none());
        assert!(response.results.is_some());

        let results = response.results.unwrap();

        assert_eq!(results.len(), 30);

        let first = results.first().unwrap();

        assert_eq!(first.engine, Torrent);
        assert_eq!(first.title, "Dragons Den UK S20E09 1080p HEVC x265-MeGusta");
        assert_eq!(first.link, "magnet:?first-link");
        assert_eq!(
            first.description,
            "Uploaded 03-03 00:50, Size 520.6 MiB, ULed by  jajaja"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_search_uses_apibay_query_endpoint() -> Result<()> {
        let mut fetcher = MockTextFetcher::new();

        fetcher
            .expect_get_text()
            .withf(|url| url == "https://apibay.org/q.php?q=ubuntu&cat=0")
            .returning(|_| Ok("[]".to_string()));

        let pc = PirateClient::new(Arc::new(fetcher), None);

        let response = pc.search("ubuntu").await?;

        assert!(response.error.is_none());
        assert_eq!(response.results.unwrap().len(), 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_search_uses_legacy_route_for_custom_html_proxy() -> Result<()> {
        let mut fetcher = MockTextFetcher::new();

        fetcher
            .expect_get_text()
            .withf(|url| url == "https://proxy.example/search/ubuntu/1/99/0")
            .returning(|_| Ok(String::from("<html></html>")));

        let pc = PirateClient::new(
            Arc::new(fetcher),
            Some(Url::parse("https://proxy.example").unwrap()),
        );

        let response = pc.search("ubuntu").await?;

        assert_eq!(response.error, Some("could not parse results".to_string()));

        Ok(())
    }

    #[tokio::test]
    async fn test_search_parses_apibay_response() -> Result<()> {
        let mut fetcher = MockTextFetcher::new();
        let json = r#"
        [
            {
                "id": "123",
                "name": "Ubuntu 24.04 LTS",
                "info_hash": "ABCDEF1234567890",
                "leechers": "2",
                "seeders": "44",
                "size": "6203355136",
                "num_files": "1",
                "username": "trusted",
                "added": "1725819528",
                "status": "trusted",
                "category": "303",
                "imdb": ""
            },
            {
                "id": "124",
                "name": "No Seeds",
                "info_hash": "FFFFFFFFFFFFFFFF",
                "leechers": "1",
                "seeders": "0",
                "size": "1",
                "num_files": "1",
                "username": "nobody",
                "added": "1725819528",
                "status": "",
                "category": "303",
                "imdb": ""
            }
        ]
        "#;

        fetcher
            .expect_get_text()
            .returning(move |_| Ok(json.to_string()));

        let pc = PirateClient::new(Arc::new(fetcher), None);

        let response = pc.search("ubuntu").await?;

        assert!(response.error.is_none());
        let results = response.results.unwrap();
        assert_eq!(results.len(), 1);

        let first = results.first().unwrap();
        assert_eq!(first.engine, Torrent);
        assert_eq!(first.title, "Ubuntu 24 04 LTS");
        assert_eq!(
            first.link,
            "magnet:?xt=urn:btih:ABCDEF1234567890&dn=Ubuntu%2024.04%20LTS"
        );
        assert_eq!(
            first.description,
            "Uploaded 2024-09-08 18:18, Size 5.78 GiB, Seeders 44, Leechers 2, ULed by trusted"
        );

        Ok(())
    }
}
