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
        
        // Clone the dependencies to be moved into the thread
        let messenger_clone = self.messenger.clone();
        let repo_clone = self.repo.clone();
        let spawner_clone = self.spawner.clone();
        
        // Spawn a regular thread to process videos in the background
        thread::spawn(move || {
            // Create a new runtime for async operations inside this thread
            let rt = tokio::runtime::Runtime::new().unwrap();
            
            rt.block_on(async {
                for video in videos {
                    let media_sharer = MediaSharing::new(
                        messenger_clone.clone(),
                        video.clone(),
                        repo_clone.clone(),
                        spawner_clone.clone(),
                    );
                    
                    // Share the video and handle any errors
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
