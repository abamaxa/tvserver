use std::path::{Path, PathBuf};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::domain::config;
use crate::domain::messages::{DownloadInfo, DownloadRequest};
use crate::domain::models::VideoDetails;
use crate::domain::traits::{DownloadProgress, DownloadProgressMonitor, Download, Repository};

pub struct CopyDownload {
    videos: Vec<VideoDetails>,
    host_url: String,
    progress: Arc<Mutex<DownloadInfo>>,
    running: Arc<RwLock<bool>>,
    databaser: Repository,
}

#[async_trait]
impl DownloadProgress for CopyDownload {
    fn terminate(&self) {
        tokio::spawn({
            let running = self.running.clone();
            async move {
                let mut is_running = running.write().await;
                *is_running = false;
            }
        });
    }

    async fn observe(&self) -> DownloadInfo {
        self.progress.lock().await.clone()
    }
}

impl CopyDownload {
    pub fn new(host_url: String, videos: Vec<VideoDetails>, databaser: Repository) -> Self {
        let progress = Arc::new(Mutex::new(DownloadInfo {
            total_size: Some(videos.len() as i64),
            downloaded_size: 0,
            uploaded_size: None,
            finished: false,
            error_message: String::new(),
            progress_message: "Preparing to download...".to_string(),
            files: Vec::new(),
        }));

        Self {
            videos,
            host_url,
            progress,
            running: Arc::new(RwLock::new(true)),
            databaser,
        }
    }

    pub async fn download(&self) -> Result<()> {
        let host_url = if self.host_url.starts_with("http") {
            self.host_url.clone()
        } else {
            format!("http://{}", self.host_url)
        };

        let total_files = self.videos.len();
        let movies_dir = config::get_movie_dir();
        let thumbnails_dir = PathBuf::from(&movies_dir).join(".thumbnails");

        // Create thumbnails directory if it doesn't exist
        if !thumbnails_dir.exists() {
            fs::create_dir_all(&thumbnails_dir).await
                .context("Failed to create thumbnails directory")?;
        }

        tracing::info!("Starting download of {} videos from {}", total_files, host_url);

        let client = reqwest::Client::new();
        let mut downloaded_files = Vec::new();
        let mut completed_count = 0;

        {
            let mut progress_info = self.progress.lock().await;
            progress_info.progress_message = format!("Downloading 0/{} files", total_files);
        }

        for (i, video) in self.videos.iter().enumerate() {
            // Check if we should stop
            if !*self.running.read().await {
                tracing::info!("Download terminated by user");
                break;
            }

            // Update progress
            {
                let mut progress_info = self.progress.lock().await;
                progress_info.downloaded_size = i as i64;
                progress_info.progress_message = format!("Downloading {}/{} files", i, total_files);
            }

            // Create the collection directory if it doesn't exist
            let collection_dir = PathBuf::from(&movies_dir).join(&video.collection);
            if !collection_dir.exists() {
                if let Err(e) = fs::create_dir_all(&collection_dir).await {
                    tracing::error!("Failed to create directory {}: {}", collection_dir.display(), e);
                    continue;
                }
            }

            // Download video
            let video_url = format!("{}/api/stream/{}/{}", host_url, video.collection, video.video);
            let video_path = collection_dir.join(&video.video);
            downloaded_files.push(video_path.to_string_lossy().to_string());

            tracing::info!("Downloading video from {} to {}", video_url, video_path.display());

            // Download and save the video
            if let Err(e) = self.download_file(&client, &video_url, &video_path).await {
                tracing::error!("Failed to download video {}: {}", video_url, e);
                continue;
            }

            match self.databaser.save_video(&video).await {
                Ok(_) => tracing::info!("Saved video {} to database", video.video),
                Err(e) => tracing::error!("Failed to save video to database: {}", e),
            }

            // Download thumbnail if available
            if let Some(thumbnail) = self.get_thumbnail_for_video(&host_url, video).await {
                let thumbnail_filename = PathBuf::from(&thumbnail).file_name()
                    .unwrap_or_default().to_string_lossy().to_string();
                let thumbnail_url = format!("{}/api/thumbnails/{}", host_url, thumbnail);
                let thumbnail_path = thumbnails_dir.join(&thumbnail_filename);

                tracing::info!("Downloading thumbnail from {} to {}", thumbnail_url, thumbnail_path.display());

                if let Err(e) = self.download_file(&client, &thumbnail_url, &thumbnail_path).await {
                    tracing::error!("Failed to download thumbnail {}: {}", thumbnail_url, e);
                }
            }

            completed_count += 1;
        }

        // Update final progress
        {
            let mut progress_info = self.progress.lock().await;
            progress_info.downloaded_size = completed_count as i64;
            progress_info.finished = true;
            progress_info.progress_message = format!("Downloaded {}/{} files", completed_count, total_files);
            progress_info.files = downloaded_files;
        }

        tracing::info!("Download completed: {}/{} files", completed_count, total_files);
        Ok(())
    }

    async fn download_file(&self, client: &reqwest::Client, url: &str, path: &Path) -> Result<()> {
        // Skip if file already exists
        if path.exists() {
            tracing::info!("File already exists: {}", path.display());
            return Ok(());
        }

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).await
                    .context(format!("Failed to create directory {}", parent.display()))?;
            }
        }

        // Download the file
        let response = client.get(url)
            .send()
            .await
            .context(format!("Failed to send request to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Failed to download {}: HTTP status {}", url, status);
        }

        let content = response.bytes()
            .await
            .context(format!("Failed to read response from {}", url))?;

        // Create a temporary file first
        let temp_path = path.with_extension("downloading");
        let mut file = fs::File::create(&temp_path)
            .await
            .context(format!("Failed to create file {}", temp_path.display()))?;

        file.write_all(&content)
            .await
            .context(format!("Failed to write data to {}", temp_path.display()))?;

        file.flush()
            .await
            .context(format!("Failed to flush data to {}", temp_path.display()))?;

        // Rename the temporary file to the final path
        fs::rename(&temp_path, path)
            .await
            .context(format!("Failed to rename {} to {}", temp_path.display(), path.display()))?;

        Ok(())
    }

    async fn get_thumbnail_for_video(&self, host_url: &str, video: &VideoDetails) -> Option<String> {
        // Send a request to the server to get video details
        let client = reqwest::Client::new();
        let url = format!("{}/api/media/{}/{}", host_url, video.collection, video.checksum);

        match client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<serde_json::Value>().await {
                        Ok(value) => {
                            if let Some(video_obj) = value.get("Video") {
                                if let Some(thumbnail_array) = video_obj.get("thumbnail").and_then(|t| t.as_array()) {
                                    if let Some(thumbnail) = thumbnail_array.first().and_then(|t| t.as_str()) {
                                        return Some(thumbnail.to_string());
                                    }
                                }
                            }
                            None
                        },
                        Err(e) => {
                            tracing::error!("Failed to parse video details response: {}", e);
                            None
                        }
                    }
                } else {
                    tracing::error!("Failed to get video details: HTTP status {}", response.status());
                    None
                }
            },
            Err(e) => {
                tracing::error!("Failed to send request for video details: {}", e);
                None
            }
        }
    }
}

pub struct CopyServer {
    databaser: Repository,
}

impl CopyServer {
    pub fn new(databaser: Repository) -> Self {
        Self { databaser}
    }
}

#[async_trait]
impl Download for CopyServer {
    async fn download(
        &self,
        _: DownloadRequest,
    ) -> Result<DownloadProgressMonitor, anyhow::Error> {
        // This method won't be used directly, but we implement it to satisfy the trait
        anyhow::bail!("Use download_videos method for CopyServer")
    }
}

impl CopyServer {
    pub async fn download_videos(
        &self,
        host_url: String,
        videos: Vec<VideoDetails>,
    ) -> Result<DownloadProgressMonitor, anyhow::Error> {
        let downloader = Arc::new(CopyDownload::new(host_url, videos, self.databaser.clone()));

        tokio::spawn({
            let downloader = downloader.clone();
            async move {
                if let Err(err) = downloader.download().await {
                    tracing::error!("Error downloading videos: {:?}", err);
                }
            }
        });

        Ok(downloader)
    }
}
