use std::env;
use transmission_rpc::types::{
    BasicAuth,
    Id,
    TorrentAddArgs,
};
pub use transmission_rpc::types::Torrent;
use transmission_rpc::TransClient;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::models::SearchResults;
use crate::domain::{TRANSMISSION_PWD, TRANSMISSION_URL, TRANSMISSION_USER};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct DownloadProgress {
    pub name: String,
    pub percentage_downloaded: f64,
}

#[async_trait]
pub trait TorrentDaemon {
    async fn add(&self, link: &str) -> Result<String, String>;
    async fn list(&self) -> Result<SearchResults<Torrent>, SearchResults<Torrent>>;
    async fn delete(&self, id: i64, delete_local_data: bool) -> Result<(), String>;
}

pub struct TransmissionDaemon {
    url: String,
}

const DEFAULT_URL: &str = "http://higo.abamaxa.com:9091/transmission/rpc";


#[async_trait]
impl TorrentDaemon for TransmissionDaemon {

    async fn add(&self, link: &str) -> Result<String, String> {
        let mut client = self.get_client();
        let add: TorrentAddArgs = TorrentAddArgs {
            filename: Some(link.to_string()),
            ..TorrentAddArgs::default()
        };

        return match client.torrent_add(add).await {
            Ok(res) => Ok(format!("response: {:?}", &res)),
            Err(e) => Err(e.to_string()),
        }
    }

    async fn list(&self) -> Result<SearchResults<Torrent>, SearchResults<Torrent>> {
        match self.get_client().torrent_get(None, None).await {
            Err(e) => {
                println!("{}", e);
                Err(SearchResults::error(e.to_string().as_str()))
            },
            Ok(res) => Ok(SearchResults::success(res.arguments.torrents))
        }
    }

    async fn delete(&self, id: i64, delete_local_data: bool) -> Result<(), String> {
        match self.get_client().torrent_remove(vec![Id::Id(id)], delete_local_data).await {
            Err(e) => Err(e.to_string()),
            Ok(_) => Ok(())
        }
    }
}

impl TransmissionDaemon {

    pub fn new() -> Self {
        let url = env::var(TRANSMISSION_URL).unwrap_or(String::from(DEFAULT_URL));
        TransmissionDaemon{url: url }
    }

    fn get_client(&self) -> TransClient {
        let url = self.url.parse().unwrap();
        if let (Ok(user), Ok(password)) = (env::var(TRANSMISSION_USER), env::var(TRANSMISSION_PWD)) {
            TransClient::with_auth(url, BasicAuth { user, password })
        } else {
            TransClient::new(url)
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_list() {
        let client = TransmissionDaemon::new();

        match client.list().await {
            Err(err) => panic!("{}", err.error.unwrap()),
            Ok(results) => {
                for item in results.results.unwrap() {
                    println!("{:?}, {:?}", item.name, item.status);
                }
            },
        }
    }
}