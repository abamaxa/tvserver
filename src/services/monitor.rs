use crate::domain::traits::{Checker, Storer};
use crate::services::TaskManager;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};
use tokio::time::{sleep, Duration};

pub struct Monitor {
    checker: Checker,
    task_manager: Arc<TaskManager>,
    store: Storer,
}

impl Monitor {
    pub fn start(
        checker: Checker,
        task_manager: Arc<TaskManager>,
        store: Storer,
    ) -> JoinHandle<()> {
        task::spawn(async move {
            tracing::info!("starting download monitor");
            let monitor = Self {
                checker,
                task_manager,
                store,
            };

            loop {
                monitor.task_manager.cleanup(&monitor.store).await;

                if let Err(err) = &monitor.checker.check_video_information().await {
                    tracing::error!("error checking video info: {}", err);
                }

                sleep(Duration::from_secs(60)).await;
            }
        })
    }

    /*async fn move_completed_downloads(&self, items: &[Task]) {
        for item in items.iter().filter(|item| item.has_finished()) {
            if let Err(e) = item.cleanup(&self.store, false).await {
                // TODO: distinguish between genuine problems and policy delays in
                // reaping completed tasks
                tracing::info!("could not move videos: {}", e);
            } else {
                println!("key: {}", item.get_key());
                if let Err(e) = self.downloads.remove(&item.get_key(), true).await {
                    tracing::error!("could not remove video: {}: {}", item.get_key(), e);
                }
            }
        }
    }*/
}

