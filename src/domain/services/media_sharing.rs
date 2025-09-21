use std::{
    fs,
    path::Path,
    sync::Arc,
};

use tracing;

use crate::domain::{
    algorithm::{generate_video_html, replace_extension},
    config,
    models::VideoDetails,
    traits::{Databaser, InstantMessegeService, ProcessSpawner},
};

use super::encoding::{re_encode, AlreadyEncodedError};

pub struct MediaSharing {
    messenger: Arc<dyn InstantMessegeService>,
    video: VideoDetails,
    repo: Arc<dyn Databaser>,
    spawner: Arc<dyn ProcessSpawner>,
}

impl MediaSharing {
    pub fn new(
        messenger: Arc<dyn InstantMessegeService>,
        video: VideoDetails,
        repo: Arc<dyn Databaser>,
        spawner: Arc<dyn ProcessSpawner>,
    ) -> Self {
        Self {
            messenger,
            video,
            repo,
            spawner,
        }
    }

    pub async fn share(&self) -> Result<(), Box<dyn std::error::Error>> {
        let full_title = self.video.series.full_title();
        match self.do_share().await {
            Err(e) => {
                self.messenger.send_error(full_title, &e.to_string()).await?;
                Err(e)
            }
            Ok(url) => {
                self.messenger.send_link(&full_title, &url).await?;
                tracing::info!(
                    "Shared video video={} url={}",
                    self.video.get_full_path().display(),
                    url
                );
                Ok(())
            }
        }
    }

    async fn do_share(&self) -> Result<String, Box<dyn std::error::Error>> {
        // TODO: make this a task
        let path = self.video.get_full_path();
        let html_path = replace_extension(path.to_str().unwrap_or_default(), ".html");
        let server = config::get_sharing_server();

        let mut video_copy = self.video.clone();
        let re_encode_result = re_encode(&mut video_copy, true, self.spawner.clone()).await;

        if re_encode_result.is_ok() {
            self.repo.save_video(&video_copy).await?;
        } else if let Err(e) = re_encode_result {
            if e.is::<AlreadyEncodedError>() {
                // Continue if the video is already encoded
            } else {
                return Err(e);
            }
        }

        let video_url = format!("https://{}/{}", server, video_copy.get_download_path());
        let html_url = format!("https://{}/{}", server, replace_extension(&video_copy.get_download_path(), ".html"));

        let html_content = generate_video_html(&video_url, &video_copy.series.full_title());

        fs::write(Path::new(&html_path), html_content)?;

        Ok(html_url)
    }
}
