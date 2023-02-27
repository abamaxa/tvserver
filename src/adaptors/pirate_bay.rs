use async_trait::async_trait;
use scraper::{Html, Selector};
use urlencoding::decode;
use crate::domain::SearchEngineType::TORRENT;
use crate::domain::models::{SearchResults, DownloadableItem};
use crate::domain::traits::SearchEngine;


const BASE_URL: &str = "https://thehiddenbay.com";


pub struct PirateClient {
    host: String,
}

#[async_trait]
impl SearchEngine<DownloadableItem> for PirateClient {
    async fn search(&self, query: &str) -> Result<SearchResults<DownloadableItem>, reqwest::Error> {
        let url = match query {
            "top-100" => format!("{}/top/all", self.host),
            "top-videos" => format!("{}/top/200", self.host),
            "top-books" => format!("{}/top/601", self.host),
            _ =>  format!("{}/search/{}/1/99/0", self.host, query),
        };

        let res = reqwest::get(url).await?;

        let html = res.text().await?;

        match self.parse_search(html) {
            Some(results) => Ok(SearchResults::success(results)),
            None => Ok(SearchResults::error("could not parse results")),
        }
    }
}

impl PirateClient {
    pub fn new(host: Option<&str>) -> Self {
        Self{ host: String::from(host.unwrap_or(BASE_URL))}
    }

    fn parse_search(&self, html: String) -> Option<Vec<DownloadableItem>> {
        let mut results = vec![];

        let document = Html::parse_document(&html);
        let selector = Selector::parse(r#"#searchResult"#).unwrap();
        let tr_selector = Selector::parse("tr").unwrap();
        let td_selector = Selector::parse("td").unwrap();
        let link_selector = Selector::parse("a").unwrap();
        let desc_selector = Selector::parse(".detDesc").unwrap();

        let table = document.select(&selector).next()?;

        for row in table.select(&tr_selector) {
            let mut record = DownloadableItem {engine: TORRENT, ..Default::default()};
            let mut seeders: i32 = 0;

            for (idx, cell) in row.select(&td_selector).enumerate() {
                match idx {
                    1 => {
                        let mut itr = cell.select(&link_selector);
                        let title = itr.next()?.text().collect::<Vec<_>>();
                        let link = decode(itr.next().unwrap().value().attr("href")?);
                        let desc = PirateClient::get_element_text(&cell.select(&desc_selector).next()?);

                        record.title = (*title.iter().nth(0)?).to_string();
                        record.description = desc.to_string();
                        record.link = link.unwrap_or_else(|_| String::new().into()).to_string();
                    }
                    2 => seeders = PirateClient::get_element_i32(&cell)?,
                    //3 => record.leechers = PirateClient::get_element_i32(&cell)?,
                    _ => continue,
                }
            }

            if seeders > 0 {
                results.push(record);
            }
        }

        Some(results)
    }

    fn get_element_text(cell: &scraper::ElementRef) -> String {
        // The DOM allows multiple text nodes of an element, so join them all together.
        cell.text().collect::<Vec<_>>().join("").trim().to_string()
    }

    fn get_element_i32(cell: &scraper::ElementRef) -> Option<i32> {
        // The DOM allows multiple text nodes of an element, so join them all together.
        match PirateClient::get_element_text(cell).parse::<i32>() {
            Ok(value) => Some(value),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_search() {
        let pc = PirateClient::new(Some(BASE_URL));

        match pc.search("Dragons Den").await {
            Err(err) => panic!("{:?}", err.to_string()),
            Ok(results) => {
                for item in results.results.unwrap() {
                    println!("{}, {}", item.title, item.description);
                }
            },
        }
    }
}