//! # MediaStore
//!
//! `MediaStore` is responsible to storing and retrieving media from disk, as opposed
//! to some sort of cloud storage like AWS S3.
//!
//! provides an implementation of MediaStorer.
use std::path::{Path, PathBuf};
use anyhow::Result;

use crate::domain::algorithm::{get_collection_from_path, get_videos_for_series_or_id, skip_file, title_case};
use crate::domain::config::{get_movie_dir, get_thumbnail_dir};
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
        let dest_dir = Path::new(&get_movie_dir()).join(collection);

        self.store.create_folder(&dest_dir).await?;

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

    /// Deletes the parent directory of the given path if empty, then recursively
    /// deletes empty parent directories up to (but not including) movie_dir.
    async fn cleanup_empty_directories(&self, file_path: &Path, movie_dir: &str) {
        let movie_dir_path = Path::new(movie_dir);
        let mut current_dir = file_path.parent();

        while let Some(dir) = current_dir {
            // Stop if we've reached or gone above movie_dir
            if dir == movie_dir_path || !dir.starts_with(movie_dir_path) {
                break;
            }

            // Try to remove the directory (only succeeds if empty)
            match self.store.remove_empty_dir(dir).await {
                Ok(_) => {
                    tracing::info!("Deleted empty directory: {}", dir.display());
                    current_dir = dir.parent();
                }
                Err(_) => {
                    // Directory not empty or other error, stop climbing
                    break;
                }
            }
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
    async fn add_file(&self, full_path: &Path, suggested_series: Option<String>) -> anyhow::Result<PathBuf> {
        let full_path_str = full_path.to_str().unwrap_or_default();
        if skip_file(full_path_str) {
            // Skip file if it meets the skip criteria.
            return Err(anyhow::anyhow!("Skipping file: {}", full_path_str));
        }

        let collection = match suggested_series {
            Some(series) => title_case(&series),
            None => get_collection_from_path(full_path)
        };

        let dest_path = self.get_new_video_path(full_path, &collection).await?;

        // If the source path is the same as the destination, nothing needs to be done.
        if full_path == dest_path {
            return Ok(dest_path);
        }

        tracing::info!("Moving file from {:?} to {:?}", full_path, dest_path);
        self.store.rename(full_path_str, dest_path.to_str().unwrap_or_default()).await?;
        Ok(dest_path)
    }

    /// Delete a media file, ensuring that the file path is within the movie directory.
    /// Also deletes associated thumbnails and cleans up empty directories.
    async fn delete(&self, video_id: String) -> anyhow::Result<()> {
        let videos = get_videos_for_series_or_id(self.repo.clone(), &video_id).await?;
        if videos.is_empty() {
            return Err(anyhow::anyhow!("Video not found: {}", video_id));
        }

        let movie_dir = get_movie_dir();
        let thumbnail_dir = get_thumbnail_dir(&movie_dir);

        for video in videos {
            // Delete thumbnails first
            for thumbnail_filename in &video.thumbnail {
                let thumbnail_path = thumbnail_dir.join(thumbnail_filename);
                if let Err(e) = self.store.delete(thumbnail_path.to_str().unwrap_or_default()).await {
                    tracing::warn!("Failed to delete thumbnail {:?}: {}", thumbnail_path, e);
                }
            }

            // Delete video file
            let mut full_path = video.get_full_path();
            if !full_path.starts_with(&movie_dir) {
                full_path = PathBuf::from(&movie_dir).join(full_path);
            }
            // there's a race here, where the video is deleted from the filesystem but
            // could be picked up by the media checker and queued for insertion into
            // the database.
            tracing::info!("Deleting video from filesystem: {}", full_path.display());
            self.store.delete(full_path.to_str().unwrap_or_default()).await?;

            // Delete from database
            tracing::info!("Deleting video from database: {}", video.checksum);
            self.repo.delete_video(video.checksum).await?;

            // Clean up empty directories
            self.cleanup_empty_directories(&full_path, &movie_dir).await;
        }
        Ok(())
    }
}
