use crate::domain::config::get_pirate_bay_url;
use crate::domain::models::{DownloadableItem, SearchResults};
use crate::domain::traits::{MediaSearcher, TextFetcher};
use crate::domain::SearchEngineType::Torrent;
use anyhow;
use async_trait::async_trait;
use mockall::lazy_static;
use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use std::sync::Arc;
use urlencoding::decode;

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
        let url = match query {
            "top-100" => format!("{}/top/all", self.host),
            "top-videos" => format!("{}/top/200", self.host),
            "top-books" => format!("{}/top/601", self.host),
            "top-music" => format!("{}/top/100", self.host),
            _ => format!("{}/search/{}/1/99/0", self.host, query),
        };

        let html = self.client.get_text(&url).await?;

        match self.parse_search(html) {
            Some(results) => Ok(SearchResults::success(results)),
            None => Ok(SearchResults::error("could not parse results")),
        }
    }
}

impl PirateClient {
    pub fn new(client: PirateFetcher, host: Option<Url>) -> Self {
        Self {
            host: host.unwrap_or(get_pirate_bay_url()),
            client,
        }
    }

    fn parse_search(&self, html: String) -> Option<Vec<DownloadableItem>> {
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

        for (idx, cell) in row.select(&TD_SELECTOR).enumerate() {
            match idx {
                1 => {
                    let mut itr = cell.select(&LINK_SELECTOR);
                    let title = itr.next()?.text().collect::<Vec<_>>();
                    let link = decode(itr.next().unwrap().value().attr("href")?);
                    let desc = PirateClient::get_element_text(&cell.select(&DESC_SELECTOR).next()?);

                    record.title = (*title.first()?).replace('.', " ");
                    record.description = desc.to_owned();
                    record.link = link.unwrap_or_else(|_| String::new().into()).to_string();
                }
                2 => seeders = PirateClient::get_element_i32(&cell)?,
                //3 => record.leechers = PirateClient::get_element_i32(&cell)?,
                _ => continue,
            }
        }

        match seeders {
            0 => None,
            _ => Some(record),
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
            "Uploaded 03-03 00:50, Size 520.6 MiB, ULed by  jajaja"
        );

        Ok(())
    }
}
