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
            let mut had_tasks = false;
            loop {
                // Check video information every ~5 minutes when idle (30s when tasks active)
                if counter % 10 == 0 {
                    monitor.task_manager.cleanup(&monitor.store).await;

                    if let Err(err) = &monitor.checker.check_video_information().await {
                        tracing::error!("error checking video info: {}", err);
                    }
                }

                let current_state = monitor.task_manager.get_current_state().await;
                let has_tasks = !current_state.is_empty();

                // Broadcast when tasks exist, or once when transitioning to empty
                // so clients clear their task UI
                if has_tasks || had_tasks {
                    if let Err(e) = monitor.sender.send(LocalMessage::Task(current_state)).await {
                        tracing::error!("could not send task state: {}", e.to_string());
                    }
                }
                had_tasks = has_tasks;

                // Back off when idle: 5s with no tasks, 3s with active tasks
                let sleep_secs = if has_tasks { 3 } else { 5 };
                sleep(Duration::from_secs(sleep_secs)).await;
                counter += 1;
            }
        })
    }
}

