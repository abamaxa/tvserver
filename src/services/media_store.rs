//! # MediaStore
//!
//! `MediaStore` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaStorer.
use std::path::{Path, PathBuf};
use anyhow::Result;

use crate::domain::algorithm::{get_collection_from_path, get_videos_for_series_or_id, skip_file, title_case};
use crate::domain::config::get_movie_dir;
use crate::domain::messages::MediaItem;
use async_trait::async_trait;

use crate::domain::models::{CollectionDetails, CollectionItem, VideoDetails};
use crate::domain::traits::{FileStorer, MediaStorer, Repository};


#[derive(Clone)]
pub struct MediaStore {
    store: FileStorer,
    repo: Repository,
}

impl MediaStore {
    pub fn new(store: FileStorer, repo: Repository) -> MediaStore {
        MediaStore { store, repo }
    }

    async fn get_new_video_path(&self, path: &Path, collection: &str) -> anyhow::Result<PathBuf> {
        let dest_dir = Path::new(&get_movie_dir()).join("New");

        self.store.create_folder(collection).await?;

        Ok(dest_dir.join(path.file_name().unwrap_or_default()))
    }

    async fn list_series(&self, series_or_id: &str) -> Result<CollectionDetails> {
        if !series_or_id.is_empty() {
            // When series_or_id is provided, retrieve the video details.
            let items: Vec<VideoDetails> = get_videos_for_series_or_id(self.repo.clone(), series_or_id).await?;
            // Collections is empty in this branch.
            Ok(CollectionDetails::new(
                series_or_id.to_string(),
                Vec::<CollectionItem>::new(),
                items,
            ))
        } else {
            // When series_or_id is empty, list all series.
            let collections: Vec<CollectionItem> = self.repo.list_all_series().await?;
            // Videos is empty in this branch.
            Ok(CollectionDetails::new(
                series_or_id.to_string(),
                collections,
                Vec::<VideoDetails>::new(),
            ))
        }
    }
}

#[async_trait]
impl MediaStorer for MediaStore {

    /// List media items for a given collection.
    async fn list(&self, series: &str) -> Result<MediaItem> {
        let details = self.list_series(series).await?;
        Ok(MediaItem::Collection(details))
    }

    /// Add a file to the media store.
    async fn add_file(&self, full_path: &Path) -> anyhow::Result<()> {
        let full_path_str = full_path.to_str().unwrap_or_default();
        if skip_file(full_path_str) {
            // Skip file if it meets the skip criteria.
            return Ok(());
        }

        // Determine the collection and file name from the path.
        let collection = title_case(&get_collection_from_path(full_path));

        let dest_path = self.get_new_video_path(full_path, &collection).await?;

        // If the source path is the same as the destination, nothing needs to be done.
        if full_path == dest_path {
            return Ok(());
        }

        tracing::info!("Moving file from {:?} to {:?}", full_path, dest_path);
        self.store.rename(full_path_str, dest_path.to_str().unwrap_or_default()).await?;
        Ok(())
    }

    /// Rename (move) a media file.
    async fn rename(&self, current: &str, new_name: &str) -> anyhow::Result<()> {
        let current_path = Path::new(current);
        let parent_dir = current_path.parent()
            .ok_or_else(|| anyhow::anyhow!("No parent directory found for {:?}", current_path))?;
        let new_path = parent_dir.join(new_name);
        self.store.rename(current, new_path.to_str().unwrap_or_default()).await?;
        Ok(())
    }

    /// Delete a media file, ensuring that the file path is within the movie directory.
    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        let mut full_path = PathBuf::from(path);
        let movie_dir = get_movie_dir();
        if !full_path.starts_with(&movie_dir) {
            full_path = PathBuf::from(movie_dir).join(full_path);
        }
        self.store.delete(full_path.to_str().unwrap_or_default()).await?;
        Ok(())
    }

    /// Construct a local path for a video within a collection.
    fn as_local_path(&self, collection: &str, video: &str) -> String {
        let movie_dir = PathBuf::from(get_movie_dir());
        movie_dir.join(collection).join(video)
            .to_string_lossy()
            .to_string()
    }
}

/*use crate::domain::algorithm::{get_collection_and_video_from_path, get_collection_from_path};
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

        if details.videos.len() == 1 && name != "Poirot" {
            match details.videos.get(0) {
                Some(MediaItem::Video(video)) => {
                    if video.video == name {
                        return Ok(MediaItem::Video(video.to_owned()))
                    }
                },
                _ => (),
            }
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
*/