use async_trait::async_trait;
use std::env;
use transmission_rpc::types::{BasicAuth, Id, TorrentAddArgs, TorrentGetField};
use transmission_rpc::TransClient;

use crate::domain::models::{DownloadListResults, DownloadProgress, SearchResults};
use crate::domain::traits::DownloadClient;
use crate::domain::{TRANSMISSION_PWD, TRANSMISSION_URL, TRANSMISSION_USER};

pub struct TransmissionDaemon {
    url: String,
}

const DEFAULT_URL: &str = "http://higo.abamaxa.com:9091/transmission/rpc";

const FIELDS: [TorrentGetField; 23] = [
    TorrentGetField::ActivityDate,
    TorrentGetField::AddedDate,
    TorrentGetField::DoneDate,
    TorrentGetField::DownloadDir,
    TorrentGetField::EditDate,
    TorrentGetField::Eta,
    TorrentGetField::Error,
    TorrentGetField::ErrorString,
    TorrentGetField::Files,
    TorrentGetField::HashString,
    TorrentGetField::Id,
    TorrentGetField::IsFinished,
    TorrentGetField::LeftUntilDone,
    TorrentGetField::Name,
    TorrentGetField::PeersConnected,
    TorrentGetField::PeersGettingFromUs,
    TorrentGetField::PeersSendingToUs,
    TorrentGetField::PercentDone,
    TorrentGetField::RateDownload,
    TorrentGetField::RateUpload,
    TorrentGetField::SizeWhenDone,
    TorrentGetField::Status,
    TorrentGetField::TotalSize,
];

#[async_trait]
impl DownloadClient for TransmissionDaemon {
    async fn fetch(&self, link: &str) -> Result<String, String> {
        let mut client = self.get_client();
        let add: TorrentAddArgs = TorrentAddArgs {
            filename: Some(link.to_string()),
            paused: Some(false),
            ..TorrentAddArgs::default()
        };

        return match client.torrent_add(add).await {
            Ok(res) => Ok(format!("response: {:?}", &res)),
            Err(e) => Err(e.to_string()),
        };
    }

    async fn list_in_progress(&self) -> DownloadListResults {
        match self
            .get_client()
            .torrent_get(Some(FIELDS.to_vec()), None)
            .await
        {
            Err(e) => {
                tracing::error!("{}", e);
                SearchResults::error(e.to_string().as_str())
            }
            Ok(res) => {
                let results = res
                    .arguments
                    .torrents
                    .iter()
                    .map(DownloadProgress::from)
                    .collect();

                SearchResults::success(results)
            }
        }
    }

    async fn remove(&self, id: i64, delete_local_data: bool) -> Result<(), String> {
        match self
            .get_client()
            .torrent_remove(vec![Id::Id(id)], delete_local_data)
            .await
        {
            Err(e) => Err(e.to_string()),
            Ok(_) => Ok(()),
        }
    }
}

impl TransmissionDaemon {
    pub fn new() -> Self {
        let url = env::var(TRANSMISSION_URL).unwrap_or(String::from(DEFAULT_URL));
        TransmissionDaemon { url }
    }

    fn get_client(&self) -> TransClient {
        let url = self.url.parse().unwrap();
        if let (Ok(user), Ok(password)) = (env::var(TRANSMISSION_USER), env::var(TRANSMISSION_PWD))
        {
            TransClient::with_auth(url, BasicAuth { user, password })
        } else {
            TransClient::new(url)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::domain::models::DownloadableItem;
    use crate::domain::traits::SearchEngine;
    use crate::services::pirate_bay::PirateClient;

    #[tokio::test]
    #[ignore]
    async fn test_torrents_list() {
        let client = TransmissionDaemon::new();

        let results = client.list_in_progress().await;

        for item in &results.results.unwrap() {
            println!("{:?}, {:?}", item.name, item.download_finished);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_torrents_add_and_delete() {
        let mut link: Option<String> = None;
        let pc: &dyn SearchEngine<DownloadableItem> = &PirateClient::new(None);

        match pc.search("top-books").await {
            Err(err) => panic!("{}", err.to_string()),
            Ok(results) => {
                for item in results.results.unwrap() {
                    println!("{}: {}, {}", item.link, item.title, item.description);
                    link = Some(item.link);
                    break;
                }
            }
        }

        if link.is_none() {
            panic!("no test torrent found");
        }

        let client = TransmissionDaemon::new();

        match client.fetch(&link.unwrap()).await {
            Ok(result) => println!("{}", result),
            Err(err) => panic!("{}", err),
        }

        let results = client.list_in_progress().await;
        for item in &results.results.unwrap() {
            println!("{}, {}", item.name, item.download_finished);
        }
    }
}
