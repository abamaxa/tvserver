use std::sync::Arc;
use std::thread;
use tracing;

use crate::domain::{
    algorithm::get_videos_for_series_or_id,
    services::MediaSharing,
    traits::{Databaser, InstantMessegeService, MediaSharer, ProcessSpawner},
};

pub struct SharingService {
    messenger: Arc<dyn InstantMessegeService>,
    repo: Arc<dyn Databaser>,
    spawner: Arc<dyn ProcessSpawner>,
}

impl SharingService {
    pub fn new(
        messenger: Arc<dyn InstantMessegeService>,
        repo: Arc<dyn Databaser>,
        spawner: Arc<dyn ProcessSpawner>,
    ) -> Self {
        Self {
            messenger,
            repo,
            spawner,
        }
    }
}

#[async_trait::async_trait]
impl MediaSharer for SharingService {
    async fn share(&self, series_or_id: &str) -> anyhow::Result<()> {
        let videos = get_videos_for_series_or_id(self.repo.clone(), series_or_id).await?;

        let messenger_clone = self.messenger.clone();
        let repo_clone = self.repo.clone();
        let spawner_clone = self.spawner.clone();

        // MediaSharing::share() returns Box<dyn Error> which is not Send,
        // so we use thread::spawn with a dedicated runtime.
        thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("failed to create runtime for sharing: {}", e);
                    return;
                }
            };

            rt.block_on(async {
                for video in videos {
                    let media_sharer = MediaSharing::new(
                        messenger_clone.clone(),
                        video.clone(),
                        repo_clone.clone(),
                        spawner_clone.clone(),
                    );

                    if let Err(err) = media_sharer.share().await {
                        tracing::error!(
                            "Failed to share video: {} error: {}",
                            video.get_full_path().display(),
                            err
                        );
                    }
                }
            });
        });

        Ok(())
    }
}
