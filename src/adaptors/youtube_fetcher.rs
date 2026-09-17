use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::{sync::RwLock, fs};

use crate::domain::config::get_downloads_dir;
use crate::domain::traits::{DownloadProgress, DownloadProgressMonitor, Download, Spawner, Task};
use crate::domain::messages::{DownloadInfo, DownloadRequest, TaskState};

pub struct YoutubeTask {
    spawner: Spawner,
    request: DownloadRequest,
    monitor: RwLock<Option<Task>>,
    destination: PathBuf,
}

#[async_trait]
impl DownloadProgress for YoutubeTask {
    fn terminate(&self) {
        tracing::info!("terminating youtube download: {}", self.request.name);
        // Note: the spawner's Task doesn't expose a synchronous abort.
        // Logging the termination request; the monitor task will be cleaned up when dropped.
    }

    async fn observe(&self) -> DownloadInfo {
        self.get_download_info().await
    }
}

impl YoutubeTask {
    pub fn new(spawner: Spawner, request: DownloadRequest) -> Self {
        let destination = PathBuf::from(get_downloads_dir())
            .join(format!("{}.mp4", request.name));

        Self { spawner, request, monitor: RwLock::new(None), destination: destination }
    }

    async fn start(&self) -> Result<(), anyhow::Error> {

        let output_path = self.destination
            .to_string_lossy()
            .to_string();

        let monitor =self.spawner
            .execute(
                &self.request.name,
                "yt-dlp",
                vec![
                    "--no-update",
                    // Deno is enabled by default; also allow the Node runtime
                    // supplied by our container and local development setup.
                    "--js-runtimes",
                    "node",
                    "-f", 
                    "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
                    "-o",
                    &output_path,
                    "--",
                    &self.request.link,
                ],
            )
            .await;

        self.monitor.write().await.replace(monitor);

        Ok(())  
    }

    /// Get current download info
    async fn get_download_info(&self) -> DownloadInfo { 
        // Get downloaded size
        let downloaded_size = self.get_file_size().await;
        let state = match self.monitor.read().await.as_ref() {
            Some(monitor) => monitor.get_state().await,
            None => TaskState {..Default::default()},
        };
        
        DownloadInfo {
            total_size: None,
            downloaded_size,
            uploaded_size: None,
            finished: state.finished,
            error_message: state.error_string,
            progress_message: state.process_details,
            files: vec![self.destination.to_string_lossy().to_string()],
        }
    }

    async fn get_file_size(&self) -> i64 {
        let base_file = self.destination.with_extension("");
        let dir = self.destination.parent().unwrap_or_else(|| Path::new(""));

        let Ok(mut entries) = fs::read_dir(dir).await else {
            tracing::error!("Failed to read directory: {:?}", dir);
            return 0;
        };

        let base_str = base_file.to_string_lossy().to_string();
        let mut total_size = 0;

        // Iterate through directory entries one by one
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() && path.to_string_lossy().to_string().starts_with(&base_str) {
                if let Ok(metadata) = fs::metadata(&path).await {
                    total_size += metadata.len() as i64;
                }
            }
        }

        total_size
    }

}

pub struct YoutubeFetcher {
    spawner: Spawner,
}

#[async_trait]
impl Download for YoutubeFetcher {
    async fn download(&self, request: DownloadRequest) -> Result<DownloadProgressMonitor, anyhow::Error> {
        let task = YoutubeTask::new(self.spawner.clone(), request);
        task.start().await?;
        Ok(Arc::new(task))
    }
}

impl YoutubeFetcher {
    pub fn new(spawner: Spawner) -> Self {
        Self { spawner }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::TokioProcessSpawner;
    use crate::domain::SearchEngineType;
    use axum::{http::header, routing::get, Router};
    use std::time::Duration;

    struct DownloadFixture {
        directory: PathBuf,
        server: tokio::task::JoinHandle<()>,
    }

    impl Drop for DownloadFixture {
        fn drop(&mut self) {
            self.server.abort();
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    // Exercise the real downloader against a local file, so invalid yt-dlp
    // arguments fail without relying on YouTube availability or credentials.
    #[tokio::test]
    #[ignore = "requires yt-dlp on PATH and a loopback HTTP listener"]
    async fn downloads_one_file_with_a_fixed_output_name() -> anyhow::Result<()> {
        const VIDEO: &[u8] = include_bytes!("../../tests/fixtures/media_dir/test.mp4");
        let app = Router::new().route(
            "/video.mp4",
            get(|| async { ([(header::CONTENT_TYPE, "video/mp4")], VIDEO) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let directory = std::env::temp_dir().join(format!(
            "tvserver-youtube-{}-{}",
            std::process::id(),
            address.port()
        ));
        fs::create_dir_all(&directory).await?;
        let fixture = DownloadFixture {
            directory,
            server: tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
        };
        let destination = fixture.directory.join("downloaded video.mp4");
        let task = YoutubeTask {
            spawner: Arc::new(TokioProcessSpawner::new()),
            request: DownloadRequest {
                name: "downloaded video".into(),
                link: format!("http://{address}/video.mp4"),
                engine: SearchEngineType::YouTube,
                series: None,
            },
            monitor: RwLock::new(None),
            destination: destination.clone(),
        };

        task.start().await?;
        let info = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let info = task.observe().await;
                if info.finished {
                    break info;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?;
        assert!(
            info.error_message.is_empty(),
            "download failed: {}; {}",
            info.error_message,
            info.progress_message
        );
        assert_eq!(fs::read(&destination).await?, VIDEO);
        assert_eq!(info.downloaded_size, VIDEO.len() as i64);
        assert_eq!(info.files, vec![destination.to_string_lossy().to_string()]);
        Ok(())
    }
}
