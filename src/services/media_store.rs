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
