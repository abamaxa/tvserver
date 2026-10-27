use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::{sync::RwLock, fs};

use crate::domain::config::get_downloads_dir;
use crate::domain::traits::{DownloadProgress, DownloadProgressMonitor, Download, Spawner, Task};
use crate::domain::messages::{DownloadInfo, DownloadRequest, TaskState};
use crate::adaptors::youtube_runtime;
use crate::domain::SearchEngineType;

pub struct YoutubeTask {
    spawner: Spawner,
    request: DownloadRequest,
    monitor: RwLock<Option<Task>>,
    destination: PathBuf,
}

#[async_trait]
impl DownloadProgress for YoutubeTask {
    fn terminate(&self) {
        if let Ok(monitor) = self.monitor.try_read() {
            if let Some(task) = monitor.as_ref() { task.terminate(); }
        }
    }

    async fn observe(&self) -> DownloadInfo {
        self.get_download_info().await
    }
}

impl YoutubeTask {
    pub fn new(spawner: Spawner, request: DownloadRequest) -> Self {
        let extension = if request.engine == SearchEngineType::SoundCloud { "mp3" } else { "mp4" };
        let destination = PathBuf::from(get_downloads_dir())
            .join(format!("{}.{extension}", download_filename(&request.name)));

        Self { spawner, request, monitor: RwLock::new(None), destination: destination }
    }

    async fn start(&self) -> Result<(), anyhow::Error> {

        let mut command = youtube_runtime::command().await?;

        let audio_only = self.request.engine == SearchEngineType::SoundCloud;
        let output_path = if audio_only {
            // Let yt-dlp use the source extension before FFmpeg produces the final MP3.
            format!("{}.%(ext)s", self.destination.with_extension("").to_string_lossy().replace('%', "%%"))
        } else {
            self.destination.to_string_lossy().replace('%', "%%")
        };

        let link = if self.request.link.len() == 11 && self.request.link.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
            format!("https://www.youtube.com/watch?v={}", self.request.link)
        } else { self.request.link.clone() };
        command.args.extend([
            "--no-update", "--no-playlist", "--newline", "--socket-timeout", "20",
        ].into_iter().map(String::from));
        let format_args = if audio_only {
            vec!["-f", "bestaudio/best", "--extract-audio", "--audio-format", "mp3", "--audio-quality", "0"]
        } else {
            // Prefer synchronized, efficient hardware playback on Android.
            // Existing AV1 downloads use VLC's software decoder there.
            let format = if cfg!(target_os = "android") {
                "bestvideo[ext=mp4][vcodec^=avc1]+bestaudio[ext=m4a]/best[ext=mp4][vcodec^=avc1]/bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best"
            } else {
                "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best"
            };
            vec!["-f", format, "--merge-output-format", "mp4"]
        };
        command.args.extend(format_args.into_iter().map(String::from));
        command.args.extend(["-o", &output_path, "--", &link].into_iter().map(String::from));

        let monitor =self.spawner
            .execute(
                &self.request.name,
                &command.program,
                command.args.iter().map(String::as_str).collect(),
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

fn download_filename(title: &str) -> String {
    let mut name = String::new();
    for character in title.chars() {
        let character = if character.is_control() || matches!(character, '/' | '\\') { '_' } else { character };
        if name.len() + character.len_utf8() > 180 { break; }
        name.push(character);
    }
    let name = name.trim_matches(['.', ' ']);
    if name.is_empty() { "YouTube video".into() } else { name.into() }
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

    #[test]
    fn download_names_stay_in_one_storage_component() {
        assert_eq!(download_filename("../bad/name\\clip"), "_bad_name_clip");
        assert_eq!(download_filename("..."), "YouTube video");
        assert_eq!(download_filename("A café & 100% video"), "A café & 100% video");
        let name = download_filename(&"é".repeat(200));
        assert!(name.len() <= 180);
        assert!(!name.contains('/'));
    }

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
    #[ignore = "requires yt-dlp installed in DOWNLOAD_DIR and a loopback HTTP listener"]
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

    #[tokio::test]
    #[ignore = "requires yt-dlp, FFmpeg, ffprobe and a loopback HTTP listener"]
    async fn downloads_soundcloud_audio_as_one_reported_mp3() -> anyhow::Result<()> {
        // A short PCM WAV requires real audio conversion rather than an extension change.
        let app = Router::new().route("/audio.wav", get(|| async {
            let mut wav = b"RIFF".to_vec();
            wav.extend(1636_u32.to_le_bytes());
            wav.extend(b"WAVEfmt ");
            wav.extend(16_u32.to_le_bytes());
            wav.extend(1_u16.to_le_bytes());
            wav.extend(1_u16.to_le_bytes());
            wav.extend(8000_u32.to_le_bytes());
            wav.extend(16000_u32.to_le_bytes());
            wav.extend(2_u16.to_le_bytes());
            wav.extend(16_u16.to_le_bytes());
            wav.extend(b"data");
            wav.extend(1600_u32.to_le_bytes());
            wav.resize(1644, 0);
            ([(header::CONTENT_TYPE, "audio/wav")], wav)
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let directory = std::env::temp_dir().join(format!("tvserver-soundcloud-{}-{}", std::process::id(), address.port()));
        fs::create_dir_all(&directory).await?;
        let fixture = DownloadFixture {
            directory,
            server: tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
        };
        let destination = fixture.directory.join("100% café track.mp3");
        let task = YoutubeTask {
            spawner: Arc::new(TokioProcessSpawner::new()),
            request: DownloadRequest {
                name: "100% café track".into(),
                link: format!("http://{address}/audio.wav"),
                engine: SearchEngineType::SoundCloud,
                series: None,
            },
            monitor: RwLock::new(None),
            destination: destination.clone(),
        };
        task.start().await?;
        let info = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let info = task.observe().await;
                if info.finished { break info; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await?;
        assert!(info.error_message.is_empty(), "download failed: {}; {}", info.error_message, info.progress_message);
        assert_eq!(info.files, vec![destination.to_string_lossy().to_string()]);
        assert_eq!(info.downloaded_size, fs::metadata(&destination).await?.len() as i64);
        let output = tokio::process::Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_type,codec_name", "-of", "json"])
            .arg(&destination).output().await?;
        assert!(output.status.success());
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(metadata["streams"].as_array().unwrap().len(), 1);
        assert_eq!(metadata["streams"][0]["codec_type"], "audio");
        assert_eq!(metadata["streams"][0]["codec_name"], "mp3");
        assert!(matches!(crate::domain::algorithm::classify_media_kind(&destination), crate::domain::algorithm::MediaKind::Video));
        Ok(())
    }
}
