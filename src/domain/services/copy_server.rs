use std::path::{Path, PathBuf};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::domain::config;
use crate::domain::messages::{DownloadInfo, DownloadRequest, DownloadSource};
use crate::domain::models::VideoDetails;
use crate::domain::traits::{DownloadProgress, DownloadProgressMonitor, Download, Repository};
use crate::services::download_source::RemoteLibrary;

pub struct CopyDownload {
    videos: Vec<VideoDetails>,
    remote: RemoteLibrary,
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
    pub fn new(source: DownloadSource, videos: Vec<VideoDetails>, databaser: Repository) -> Result<Self> {
        let remote = RemoteLibrary::new(source)?;
        if videos.is_empty() { anyhow::bail!("Select at least one video to download"); }
        for video in &videos { remote.stream_url(video)?; }
        let progress = Arc::new(Mutex::new(DownloadInfo {
            total_size: Some(videos.len() as i64),
            downloaded_size: 0,
            uploaded_size: None,
            finished: false,
            error_message: String::new(),
            progress_message: "Preparing to download...".to_string(),
            files: Vec::new(),
        }));

        Ok(Self {
            videos,
            remote,
            progress,
            running: Arc::new(RwLock::new(true)),
            databaser,
        })
    }

    pub async fn download(&self) -> Result<()> {
        let host_url = self.remote.host_url();

        let total_files = self.videos.len();
        let movies_dir = config::get_movie_dir();
        let thumbnails_dir = PathBuf::from(&movies_dir).join(".thumbnails");

        // Create thumbnails directory if it doesn't exist
        if !thumbnails_dir.exists() {
            fs::create_dir_all(&thumbnails_dir).await
                .context("Failed to create thumbnails directory")?;
        }

        tracing::info!("Starting download of {} videos from {}", total_files, host_url);

        let mut downloaded_files = Vec::new();
        let mut completed_count = 0;
        let mut failed_count = 0;

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
                    failed_count += 1;
                    continue;
                }
            }

            // Download video
            let video_url = self.remote.stream_url(video)?;
            let video_path = collection_dir.join(&video.video);

            tracing::info!("Downloading video from {} to {}", video_url, video_path.display());

            // Download and save the video
            if let Err(e) = self.download_file(video_url.clone(), &video_path).await {
                tracing::error!("Failed to download video {}: {}", video_url, e);
                failed_count += 1;
                continue;
            }
            downloaded_files.push(video_path.to_string_lossy().to_string());

            match self.databaser.save_video(&video).await {
                Ok(_) => tracing::info!("Saved video {} to database", video.video),
                Err(e) => tracing::error!("Failed to save video to database: {}", e),
            }

            // Download thumbnail if available
            if let Some(thumbnail) = self.get_thumbnail_for_video(video).await {
                let thumbnail_filename = PathBuf::from(&thumbnail).file_name()
                    .unwrap_or_default().to_string_lossy().to_string();
                let thumbnail_url = self.remote.thumbnail_url(&thumbnail)?;
                let thumbnail_path = thumbnails_dir.join(&thumbnail_filename);

                tracing::info!("Downloading thumbnail from {} to {}", thumbnail_url, thumbnail_path.display());

                if let Err(e) = self.download_file(thumbnail_url.clone(), &thumbnail_path).await {
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
            if failed_count > 0 {
                progress_info.progress_message.push_str(&format!("; {failed_count} failed"));
            }
            if failed_count > 0 && completed_count == 0 {
                progress_info.error_message = format!("Failed to download {failed_count} of {total_files} files from the host");
            }
        }

        tracing::info!("Download completed: {}/{} files", completed_count, total_files);
        Ok(())
    }

    async fn download_file(&self, url: reqwest::Url, path: &Path) -> Result<()> {
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
        let mut response = self.remote.get(url.clone())
            .send()
            .await
            .context(format!("Failed to send request to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Failed to download {}: HTTP status {}", url, status);
        }

        // Create a temporary file first
        let temp_path = path.with_extension("downloading");
        let mut file = fs::File::create(&temp_path)
            .await
            .context(format!("Failed to create file {}", temp_path.display()))?;

        while let Some(chunk) = response.chunk().await.context("Failed to read download")? {
            if !*self.running.read().await { anyhow::bail!("Download cancelled"); }
            file.write_all(&chunk).await
                .context(format!("Failed to write data to {}", temp_path.display()))?;
        }

        file.flush()
            .await
            .context(format!("Failed to flush data to {}", temp_path.display()))?;

        // Rename the temporary file to the final path
        fs::rename(&temp_path, path)
            .await
            .context(format!("Failed to rename {} to {}", temp_path.display(), path.display()))?;

        Ok(())
    }

    async fn get_thumbnail_for_video(&self, video: &VideoDetails) -> Option<String> {
        // Send a request to the server to get video details
        let url = self.remote.api_url("media", &format!("{}/{}", video.collection, video.checksum)).ok()?;

        match self.remote.get(url).timeout(std::time::Duration::from_secs(30)).send().await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Path as AxumPath, http::{HeaderMap, StatusCode}, response::IntoResponse, routing::get, Router};

    #[tokio::test]
    async fn download_source_stream_uses_basic_auth_and_preserves_file_bytes() {
        async fn stream(headers: HeaderMap, AxumPath(path): AxumPath<String>) -> axum::response::Response {
            assert_eq!(path, "Show/Season 1/Clip #1 & café.mp4");
            if headers.get("authorization").and_then(|value| value.to_str().ok()) != Some("Basic dmlld2VyOnNlY3JldA==") {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            vec![0_u8, 1, 2, 255, 100].into_response()
        }
        async fn metadata(headers: HeaderMap) -> axum::Json<serde_json::Value> {
            assert_eq!(headers["authorization"], "Basic dmlld2VyOnNlY3JldA==");
            axum::Json(serde_json::json!({ "Video": { "thumbnail": ["/api/thumbnails/image 1.jpg"] } }))
        }
        async fn thumbnail(headers: HeaderMap) -> Vec<u8> {
            assert_eq!(headers["authorization"], "Basic dmlld2VyOnNlY3JldA==");
            vec![0xff, 0xd8, 0xff]
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/api/stream/{*path}", get(stream))
                .route("/api/media/{*path}", get(metadata)).route("/api/thumbnails/{*path}", get(thumbnail))).await.unwrap();
        });
        let source = DownloadSource { host_url: format!("http://{address}"), username: Some("viewer".into()), password: Some("secret".into()) };
        let video = VideoDetails { collection: "Show/Season 1".into(), video: "Clip #1 & café.mp4".into(), ..Default::default() };
        let repository = Arc::new(crate::adaptors::SqlRepository::new(":memory:", None).await.unwrap());
        let downloader = CopyDownload::new(source.clone(), vec![video.clone()], repository.clone()).unwrap();
        let root = std::env::temp_dir().join(format!("tvserver-source-download-{}", rand::random::<u64>()));
        let target = root.join(&video.video);
        downloader.download_file(downloader.remote.stream_url(&video).unwrap(), &target).await.unwrap();
        assert_eq!(fs::read(&target).await.unwrap(), vec![0_u8, 1, 2, 255, 100]);
        let image = downloader.get_thumbnail_for_video(&video).await.unwrap();
        assert_eq!(image, "/api/thumbnails/image 1.jpg");
        let image_target = root.join("image 1.jpg");
        downloader.download_file(downloader.remote.thumbnail_url(&image).unwrap(), &image_target).await.unwrap();
        assert_eq!(fs::read(image_target).await.unwrap(), vec![0xff, 0xd8, 0xff]);
        let anonymous = CopyDownload::new(DownloadSource { username: None, password: None, ..source }, vec![video.clone()], repository).unwrap();
        let rejected = root.join("rejected.mp4");
        let error = anonymous.download_file(anonymous.remote.stream_url(&video).unwrap(), &rejected).await.unwrap_err();
        assert!(error.to_string().contains("401"));
        assert!(!rejected.exists());
        fs::remove_dir_all(root).await.unwrap();
        server.abort();
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
        source: DownloadSource,
        videos: Vec<VideoDetails>,
    ) -> Result<DownloadProgressMonitor, anyhow::Error> {
        let downloader = Arc::new(CopyDownload::new(source, videos, self.databaser.clone())?);

        tokio::spawn({
            let downloader = downloader.clone();
            async move {
                if let Err(err) = downloader.download().await {
                    tracing::error!("Error downloading videos: {:?}", err);
                    let mut progress = downloader.progress.lock().await;
                    progress.finished = true;
                    progress.error_message = err.to_string();
                }
            }
        });

        Ok(downloader)
    }
}
