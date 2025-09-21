use crate::domain::messages::{LocalMessage, LocalMessageSender};
use crate::domain::traits::{Checker, ProcessSpawner, Storer};
use crate::services::TaskManager;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};
use tokio::time::{sleep, Duration};

pub struct Monitor {
    checker: Checker,
    task_manager: Arc<TaskManager>,
    store: Storer,
    sender: LocalMessageSender,
}

impl Monitor {
    pub fn start(
        checker: Checker,
        task_manager: Arc<TaskManager>,
        store: Storer,
        sender: LocalMessageSender,
    ) -> JoinHandle<()> {
        task::spawn(async move {
            tracing::info!("updating yt-dlp");
            task_manager.execute("Update yt-dlp", "pip", vec!["install", "--upgrade", "yt-dlp"]).await;

            tracing::info!("starting download monitor");
            let monitor = Self {
                checker,
                task_manager,
                store,
                sender,
            };

            let mut counter: i64 = 0;
            loop {
                if counter % 10 == 0 {
                    monitor.task_manager.cleanup(&monitor.store).await;

                    if let Err(err) = &monitor.checker.check_video_information().await {
                        tracing::error!("error checking video info: {}", err);
                    }
                }

                monitor.send_task_state().await;

                sleep(Duration::from_secs(3)).await;
                counter += 1;
            }
        })
    }

    async fn send_task_state(&self) {
        let current_state = self.task_manager.get_current_state().await;
        if current_state.len() > 0 {
            tracing::info!("Sending task state: {:?}", current_state);
        }
        if let Err(e) = self.sender.send(LocalMessage::Task(current_state)).await {
            tracing::error!("could not send task state: {}", e.to_string());
        }
    }
}

