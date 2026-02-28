//! # MediaCheck
//!
//! `MediaCheck` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaChecker.
use crate::domain::algorithm::{get_collection_from_path, skip_file};
use crate::domain::config::get_movie_dir;
use crate::domain::messages::{LocalMessage, LocalMessageSender, MediaEvent};
use crate::domain::services::calculate_checksum;
use async_recursion::async_recursion;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::domain::models::VideoDetails;
use crate::domain::traits::{FileStorer, MediaChecker, Repository};

#[derive(Clone)]
pub struct MediaCheck {
    store: FileStorer,
    repo: Repository,
    sender: LocalMessageSender,
}

impl MediaCheck {
    pub fn new(store: FileStorer, repo: Repository, sender: LocalMessageSender) -> MediaCheck {
        MediaCheck { store, repo, sender }
    }

    async fn store_video_info(&self, path: &Path) {
        /*let queue_len = self.sender.len();
        if queue_len >= 100 {
            tracing::info!("local queue has more than 100 entries, will process {:?} later, {} receivers", path, self.sender.receiver_count());
            return;
        }*/

        let event = MediaEvent::new_media(path, None);

        tracing::info!("Storing video info: {:?}", event);

        if let Err(e) = self.sender.send(LocalMessage::Media(event)).await {
            tracing::error!("could not queue Media event: {}", e.to_string())
        }
    }

    #[async_recursion]
    async fn process_directory(&self, dir_path: PathBuf) -> anyhow::Result<()> {
        let collection = get_collection_from_path(&dir_path);

        let mut current_videos = self.repo.list_videos(&collection).await?;

        //let mut read_dir = fs::read_dir(path).await?
        let (directories, files) = self.store.list_folder(&collection).await?;

        for directory in directories {
            if skip_file(&directory) {
                continue;
            }
            self.process_directory(Path::new(&dir_path).join(directory)).await?;
        }

        for file in files {
            let path = PathBuf::from(file);

            let filename = path.file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default();

            if skip_file(filename) {
                continue;
            }

            let full_path = dir_path.join(path.clone());

            let mut existing = current_videos
                .iter()
                .filter_map(|item| if item.video == filename { Some((item.checksum, item)) } else {None})
                .collect::<HashMap<_, _>>();

            if existing.len() == 0 {
                self.store_video_info(&full_path).await;
                continue;
            }

            if existing.len() > 1 {
                let checksum = match calculate_checksum(&full_path).await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("could not calculate checksum for {:?}: {}", &full_path, e.to_string());
                        continue;
                    }
                };
                
                match existing.get(&checksum) {
                    Some(item) => existing = HashMap::from([(item.checksum, *item)]),
                    None => {
                        self.store_video_info(&full_path).await;
                        continue;
                    }
                }
            }

            let (_, current) = existing.iter().next().unwrap();
            if current.should_retry_metadata() {
                self.store_video_info(&full_path).await;
            }

            let checksum = current.checksum;
            current_videos = current_videos.into_iter().filter(
                |item| item.checksum != checksum || item.video != filename
            ).collect();
        }

        self.delete_orphaned_records(current_videos).await;

        Ok(())
    }

    async fn find_orphaned_records(&self) -> anyhow::Result<()> {
        let collections = self.repo.list_collection("").await?;

        for collection in collections {
            let videos = self.repo.list_videos(&collection).await?;
            if videos.is_empty() {
                continue;
            }

            // If directory doesn't exist, all records for this collection are orphans
            let disk_files: HashSet<String> = match self.store.list_folder(&collection).await {
                Ok((_dirs, files)) => files.into_iter().collect(),
                Err(_) => HashSet::new(),
            };

            let orphans: Vec<VideoDetails> = videos
                .into_iter()
                .filter(|v| !disk_files.contains(&v.video))
                .collect();

            self.delete_orphaned_records(orphans).await;
        }

        Ok(())
    }

    async fn delete_orphaned_records(&self, videos: Vec<VideoDetails>) {
        for video in videos {
            if let Err(err) = self.repo.delete_video(video.checksum).await {
                tracing::error!("error deleting record {}: {} - {}", video.video, video.checksum, err.to_string());
            }
        }
    }
}

#[async_trait]
impl MediaChecker for MediaCheck {
    async fn check_video_information(&self) -> anyhow::Result<()> {
        self.process_directory(PathBuf::from(&get_movie_dir())).await?;

        self.find_orphaned_records().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::{FileSystemStore, SqlRepository};
    use anyhow::Result;
    use std::sync::Arc;
    use tokio::sync::mpsc;


    #[tokio::test]
    #[ignore]
    async fn test_check_video_info() -> Result<()> {
        let (tx, _rx1) = mpsc::channel(16);
        let filer: FileStorer = Arc::new(FileSystemStore::new("/Users/chris2/Movies"));
        let repo: Repository = Arc::new(SqlRepository::new(":memory:", None).await.unwrap());
        let store = MediaCheck::new(filer, repo, tx);

        store.check_video_information().await?;

        Ok(())
    }
}
