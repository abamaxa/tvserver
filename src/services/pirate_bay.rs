use crate::domain::models::{DownloadableItem, SearchResults};
use crate::domain::traits::{SearchEngine, TextFetcher};
use crate::domain::SearchEngineType::Torrent;
use anyhow;
use async_trait::async_trait;
use mockall::lazy_static;
use scraper::{ElementRef, Html, Selector};
use urlencoding::decode;

const BASE_URL: &str = "https://thehiddenbay.com";

type Fetcher = dyn TextFetcher;

lazy_static! {
    static ref SELECTOR: Selector = Selector::parse(r#"#searchResult"#).unwrap();
    static ref TR_SELECTOR: Selector = Selector::parse("tr").unwrap();
    static ref TD_SELECTOR: Selector = Selector::parse("td").unwrap();
    static ref LINK_SELECTOR: Selector = Selector::parse("a").unwrap();
    static ref DESC_SELECTOR: Selector = Selector::parse(".detDesc").unwrap();
}

pub struct PirateClient<'a> {
    host: String,
    client: &'a Fetcher,
}

#[async_trait]
impl<'a> SearchEngine<DownloadableItem> for PirateClient<'a> {
    async fn search(&self, query: &str) -> anyhow::Result<SearchResults<DownloadableItem>> {
        let url = match query {
            "top-100" => format!("{}/top/all", self.host),
            "top-videos" => format!("{}/top/200", self.host),
            "top-books" => format!("{}/top/601", self.host),
            _ => format!("{}/search/{}/1/99/0", self.host, query),
        };

        let html = self.client.get_text(&url).await?;

        match self.parse_search(html) {
            Some(results) => Ok(SearchResults::success(results)),
            None => Ok(SearchResults::error("could not parse results")),
        }
    }
}

impl<'a> PirateClient<'a> {
    pub fn new(host: Option<&str>, client: &'a Fetcher) -> Self {
        Self {
            host: String::from(host.unwrap_or(BASE_URL)),
            client,
        }
    }

    fn parse_search(&self, html: String) -> Option<Vec<DownloadableItem>> {
        let document = Html::parse_document(&html);

        let table = document.select(&SELECTOR).next()?;

        Some(
            table
                .select(&TR_SELECTOR)
                .into_iter()
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

                    record.title = (*title.first()?).to_string();
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
        // let fetcher = HTTPClient::new();
        let mut fetcher = MockTextFetcher::new();
        let html = String::from_utf8(tokio::fs::read("tests/fixtures/pb_search.html").await?)?;

        fetcher
            .expect_get_text()
            .returning(move |_| Ok(html.clone()));

        let pc = PirateClient::new(Some(BASE_URL), &fetcher);

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
