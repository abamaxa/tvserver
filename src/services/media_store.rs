//! # MediaStore
//!
//! `MediaStore` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaStorer.
use crate::domain::algorithm::{get_collection_and_video_from_path, get_collection_from_path};
use crate::domain::config::get_movie_dir;
use crate::domain::messages::{LocalMessage, LocalMessageSender, MediaEvent, MediaItem};
use crate::domain::services::calculate_checksum;
use async_recursion::async_recursion;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::domain::models::{CollectionDetails, VideoDetails};
use crate::domain::traits::{FileStorer, MediaStorer, Repository};

#[derive(Clone)]
pub struct MediaStore {
    store: FileStorer,
    repo: Repository,
    sender: LocalMessageSender,
}

impl MediaStore {
    pub fn new(store: FileStorer, repo: Repository, sender: LocalMessageSender) -> MediaStore {
        MediaStore { store, repo, sender }
    }

    async fn get_new_video_path(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let dest_dir = Path::new(&get_movie_dir()).join("New");

        self.store.create_folder("New").await?;

        Ok(dest_dir.join(path.file_name().unwrap_or_default()))
    }

    fn skip_file(name: &str) -> bool {
        name.starts_with('.')
            || name == "TV"
            || name.ends_with(".py")
            || name.ends_with(".json")
            || name.ends_with(".png")
            || name.ends_with(".jpg")
    }

    async fn rename_or_copy_and_delete(
        &self,
        src: &Path,
        destination: &Path,
    ) -> anyhow::Result<()> {
        self.store
            .rename(
                src.as_os_str().to_str().unwrap_or_default(),
                destination.as_os_str().to_str().unwrap_or_default(),
            )
            .await?;

        self.store_video_info(destination);

        Ok(())
    }

    fn store_video_info(&self, path: &Path) {
        let queue_len = self.sender.len();
        if queue_len >= 10 {
            tracing::info!("local queue has more than 100 entries, will process {:?} later, {} receivers", path, self.sender.receiver_count());
            return;
        }

        let event = MediaEvent::new_media(path, None);

        if let Err(e) = self.sender.send(LocalMessage::Media(event)) {
            tracing::error!("could not queue Media event: {}", e.to_string())
        }
    }

    #[async_recursion]
    async fn process_directory(&self, path: PathBuf) -> anyhow::Result<()> {
        let collection = get_collection_from_path(&path);

        let mut current_videos = self.repo.list_videos(&collection).await?;

        let mut read_dir = fs::read_dir(path).await?;

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();

            let filename = path.file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default();

            if Self::skip_file(filename) {
                continue;
            }

            if path.is_dir() {
                self.process_directory(path.clone()).await?;
                continue;
            } 

            let mut existing = current_videos
                .iter()
                .filter_map(|item| if item.video == filename { Some((item.checksum, item)) } else {None})
                .collect::<HashMap<_, _>>();

            if existing.len() == 0 {
                self.store_video_info(&path);
                continue;
            }

            if existing.len() > 1 {
                let checksum = match calculate_checksum(&path).await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("could not calculate checksum for {:?}: {}", &path, e.to_string());
                        continue;
                    }
                };
                
                match existing.get(&checksum) {
                    Some(item) => existing = HashMap::from([(item.checksum, *item)]),
                    None => {
                        self.store_video_info(&path);
                        continue;
                    }
                }
            }

            let (_, current) = existing.iter().next().unwrap();
            if current.should_retry_metadata() {
                self.store_video_info(&path);
            }

            let checksum = current.checksum;
            current_videos = current_videos.into_iter().filter(
                |item| item.checksum != checksum || item.video != filename
            ).collect();
        }

        self.delete_orphaned_records(current_videos).await;

        Ok(())
    }

    async fn delete_orphaned_records(&self, videos: Vec<VideoDetails>) {
        for video in videos {
            if let Err(err) = self.repo.delete_video(video.checksum).await {
                tracing::error!("error deleting record {}: {} - {}", video.video, video.checksum, err.to_string());
            }
        }
    }

    async fn list_from_repo(&self, collection: &str) -> anyhow::Result<CollectionDetails> {

        let items = self.repo.list_videos(collection).await?;

        let collections = self.repo.list_collection(collection).await?;

        let videos = items
            .into_iter()
            .map(|i| MediaItem::Video(i))
            .collect();

        Ok(CollectionDetails::from(collection, collections, videos))
    }

}

#[async_trait]
impl MediaStorer for MediaStore {
    async fn list(&self, collection: &str) -> anyhow::Result<MediaItem> {
        fn split_at_last_slash(s: &str) -> (String, String) {
            match s.rfind('/') {
                Some(index) => {
                    let (first, last) = s.split_at(index);
                    (first.to_string(), last[1..].to_string())
                },
                None => (String::new(), s.to_string()), // Handle the case where there is no slash
            }
        }

        let (parent, name) = split_at_last_slash(collection);

        if let Ok(video) = self.repo.retrieve_video_by_name_and_collection(&name, &parent).await {
            return Ok(MediaItem::Video(video));
        }

        let details = self.list_from_repo(collection).await?;

        if details.videos.len() == 1 {
            return Ok(details.videos.get(0).unwrap().to_owned());
        } 
        
        Ok(MediaItem::Collection(details))
    }

    async fn add_file(&self, path: &Path) -> anyhow::Result<()> {
        let new_path = self.get_new_video_path(path).await?;

        tracing::debug!(
            "move file {} to {}",
            path.to_str().unwrap_or_default(),
            new_path.to_str().unwrap_or_default()
        );

        self.rename_or_copy_and_delete(path, &new_path).await?;

        Ok(())
    }

    async fn rename(&self, current: &str, new_path: &str) -> anyhow::Result<()> {
        tracing::debug!("rename file {} to {}", current, new_path);
        let item = self.store.get(current).await?;

        if !item.is_dir() {
            if let Ok(mut details) = item.get_metadata().await {
                (details.collection, details.video) =
                    get_collection_and_video_from_path(&Path::new(new_path));
                item.save_metadata(details).await?;
            }
        }

        self.store.rename(current, new_path).await
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        self.store.delete(path).await
    }

    async fn check_video_information(&self) -> anyhow::Result<()> {
        self.process_directory(PathBuf::from(&get_movie_dir())).await?;

        Ok(())
    }

    fn as_local_path(&self, collection: &str, video: &str) -> String {
        let root = get_movie_dir();
        // generates the path component of a URI to a video
        if collection.is_empty() {
            format!("{}/{}", root, video)
        } else {
            format!("{}/{}/{}", root, collection, video)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::{FileSystemStore, SqlRepository};
    use anyhow::Result;
    use std::sync::Arc;
    use tokio::sync::broadcast;

    #[test]
    fn test_skip_file() {
        let test_cases = [
            ("TV", true),
            ("TV2", false),
            ("file.py", true),
            ("file.mp4", false),
            ("file.jpg", true),
            ("file.png", true),
        ];

        for (name, expected) in test_cases {
            assert_eq!(MediaStore::skip_file(name), expected);
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_check_video_info() -> Result<()> {
        let (tx, _rx1) = broadcast::channel(16);
        let filer: FileStorer = Arc::new(FileSystemStore::new("/Users/chris2/Movies"));
        let repo: Repository = Arc::new(SqlRepository::new(":memory:").await.unwrap());
        let store = MediaStore::new(filer, repo, tx);

        store.check_video_information().await?;

        Ok(())
    }
}
