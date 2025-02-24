use async_trait::async_trait;
use mainline::{Client, Magnet, TorrentMeta};
use tokio::{fs::File, io::AsyncWriteExt};
use std::{env, path::Path, sync::Arc};

use crate::domain::models::TorrentTask;
use crate::domain::traits::{MediaDownloader, Task};

pub struct TransmissionDaemon {
    client: Client,
    // Active download tasks. We wrap them in a Mutex to allow safe concurrent access.
    tasks: Mutex<Vec<Task>>,
}


#[async_trait]
impl MediaDownloader for TransmissionDaemon {
    // TODO: implement a timeout
    async fn fetch(&self, name: &str, link: &str) -> Result<String, String> {
        tracing::info!("Processing magnet link: {}", link);
        // Parse the magnet URI.
        let magnet = Magnet::from_uri(link).expect("Invalid magnet link");

        tokio::spawn(async move {
            // Fetch metadata from peers using the magnet link.
            let metadata = self.client
                .fetch_metadata(magnet)
                .await?;

            // Begin downloading the torrent.
            let torrent = self.client.download(metadata.clone())
                .await?;
        });

        Ok(link.to_string())
    }

    async fn list_in_progress(&self) -> Result<Vec<Task>, String> {
        match self
            .get_client()
            .torrent_get(Some(FIELDS.to_vec()), None)
            .await
        {
            Err(e) => Err(e.to_string()),
            Ok(res) => Ok(res
                .arguments
                .torrents
                .iter()
                .map(|t| Arc::new(TorrentTask::from(t)) as Task)
                .collect::<Vec<Task>>()),
        }
    }

    async fn remove(&self, key: &str, delete_local_data: bool) -> Result<(), String> {
        match key.parse::<i64>() {
            Ok(id) => match self
                .get_client()
                .torrent_remove(vec![Id::Id(id)], delete_local_data)
                .await
            {
                Err(e) => Err(e.to_string()),
                Ok(_) => Ok(()),
            },
            Err(e) => Err(format!("invalid key '{}': {}", key, e)),
        }
    }
}

impl TransmissionDaemon {
    #[allow(clippy::new_without_default)]
    pub async fn new() -> Self {
        let client = Client::new().await.unwrap();
        TransmissionDaemon { client, tasks: Mutex::new(vec![]) }
    }

    fn get_client(&self) -> TransClient {
        if let (Ok(user), Ok(password)) = get_transmission_credentials() {
            TransClient::with_auth(self.url.clone(), BasicAuth { user, password })
        } else {
            TransClient::new(self.url.clone())
        }
    }

    async fn download(&self, link: &str) -> Result<(), String> {
        let magnet = Magnet::from_uri(input).expect("Invalid magnet link");
        // Fetch metadata from peers using the magnet link.
        let metadata = self.client
            .fetch_metadata(magnet)
            .await?;
        
        // Display the list of files contained in the torrent.
        tracing::info!("Torrent contains {} file(s):", metadata.files.len());
        for file in &metadata.files {
            tracing::info!("- {} ({} bytes)", file.path.display(), file.length);
        }

        // Begin downloading the torrent.
        let torrent = self.client.download(metadata.clone())
            .await?;

        tracing::info!("Downloading...");
        while !torrent.is_complete().await {
            let progress = torrent.progress().await;
            tracing::info!("Progress: {:.2}%", progress * 100.0);
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
        tracing::info!("Download complete!");

        // Create a root directory to store the downloaded files.
        let root_dir = Path::new("downloads");
        tokio::fs::create_dir_all(root_dir)
            .await
            .expect("Failed to create download directory");

        // Save each file in the torrent to disk, preserving the directory structure.
        for file in &metadata.files {
            let file_path = root_dir.join(&file.path);
            if let Some(parent) = file_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .expect("Failed to create parent directory");
            }
            tracing::info!("Saving: {}", file_path.display());
            let mut file_handle = File::create(&file_path)
                .await
                .expect("Failed to create file");
            let data = torrent.file_data(&file)
                .await
                .expect("Failed to retrieve file data");
            file_handle.write_all(&data)
                .await
                .expect("Failed to write file data");
        }
        tracing::info!("All files saved to '{}'", root_dir.display());
    }

}

/*use async_trait::async_trait;
use reqwest::Url;
use std::sync::Arc;
use transmission_rpc::types::{BasicAuth, Id, TorrentAddArgs, TorrentGetField};
use transmission_rpc::TransClient;

use crate::domain::config::{get_transmission_credentials, get_transmission_url};
use crate::domain::models::TorrentTask;
use crate::domain::traits::{MediaDownloader, Task};

pub struct TransmissionDaemon {
    url: Url,
}

// Daemon wraps generic client? but problem is that client isn't Send
// also need rate limiting

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
impl MediaDownloader for TransmissionDaemon {
    // TODO: implement a timeout
    async fn fetch(&self, name: &str, link: &str) -> Result<String, String> {
        let mut client = self.get_client();
        let add: TorrentAddArgs = TorrentAddArgs {
            filename: Some(link.to_string()),
            paused: Some(false),
            ..TorrentAddArgs::default()
        };

        return match client.torrent_add(add).await {
            Ok(res) => Ok(format!("{} response: {:?}", name, &res)),
            Err(e) => Err(e.to_string()),
        };
    }

    async fn list_in_progress(&self) -> Result<Vec<Task>, String> {
        match self
            .get_client()
            .torrent_get(Some(FIELDS.to_vec()), None)
            .await
        {
            Err(e) => Err(e.to_string()),
            Ok(res) => Ok(res
                .arguments
                .torrents
                .iter()
                .map(|t| Arc::new(TorrentTask::from(t)) as Task)
                .collect::<Vec<Task>>()),
        }
    }

    async fn remove(&self, key: &str, delete_local_data: bool) -> Result<(), String> {
        match key.parse::<i64>() {
            Ok(id) => match self
                .get_client()
                .torrent_remove(vec![Id::Id(id)], delete_local_data)
                .await
            {
                Err(e) => Err(e.to_string()),
                Ok(_) => Ok(()),
            },
            Err(e) => Err(format!("invalid key '{}': {}", key, e)),
        }
    }
}

impl TransmissionDaemon {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let url = get_transmission_url();
        TransmissionDaemon { url }
    }

    fn get_client(&self) -> TransClient {
        if let (Ok(user), Ok(password)) = get_transmission_credentials() {
            TransClient::with_auth(self.url.clone(), BasicAuth { user, password })
        } else {
            TransClient::new(self.url.clone())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::adaptors::HTTPClient;
    use crate::domain::models::DownloadableItem;
    use crate::domain::traits::{MediaSearcher, TextFetcher};
    use crate::services::pirate_bay::PirateClient;
    use std::sync::Arc;

    #[tokio::test]
    #[ignore]
    async fn test_torrents_list() {
        let client = TransmissionDaemon::new();

        let results = client.list_in_progress().await;

        for item in &results.unwrap() {
            let state = item.get_state().await;
            println!("{:?}, {:?}", state.name, state.finished);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_torrents_add_and_delete() {
        let mut link: Option<String> = None;
        let fetcher: Arc<dyn TextFetcher> = Arc::new(HTTPClient::new());
        let pc: &dyn MediaSearcher<DownloadableItem> = &PirateClient::new(fetcher, None);

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

        match client.fetch("test name", &link.unwrap()).await {
            Ok(result) => println!("{}", result),
            Err(err) => panic!("{}", err),
        }

        let results = client.list_in_progress().await;
        for item in &results.unwrap() {
            let state = item.get_state().await;
            println!("{}, {}", state.name, state.finished);
        }
    }
}
*/