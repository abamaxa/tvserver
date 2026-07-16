use crate::domain::messages::{LocalMessage, LocalMessageSender};
use crate::domain::traits::{BookCheckerHandle, Checker, ProcessSpawner, Storer};
use crate::services::TaskManager;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};
use tokio::time::{sleep, Duration};

pub struct Monitor {
    checker: Checker,
    book_checker: BookCheckerHandle,
    task_manager: Arc<TaskManager>,
    store: Storer,
    sender: LocalMessageSender,
}

impl Monitor {
    pub fn start(
        checker: Checker,
        book_checker: BookCheckerHandle,
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
                book_checker,
                task_manager,
                store,
                sender,
            };

            let mut counter: i64 = 0;
            let mut had_tasks = false;
            loop {
                let has_tasks = monitor.run_cycle(counter, had_tasks).await;
                had_tasks = has_tasks;

                // Back off when idle: 5s with no tasks, 3s with active tasks
                let sleep_secs = if has_tasks { 3 } else { 5 };
                sleep(Duration::from_secs(sleep_secs)).await;
                counter += 1;
            }
        })
    }

    async fn run_cycle(&self, counter: i64, had_tasks: bool) -> bool {
        // Check media information every ~5 minutes when idle (30s when tasks active)
        if counter % 10 == 0 {
            self.task_manager.cleanup(&self.store).await;

            if let Err(error) = self.checker.check_video_information().await {
                tracing::error!("error checking video info: {error}");
            }
            if let Err(error) = self.book_checker.check_book_information().await {
                tracing::error!("error checking book info: {error}");
            }
        }

        let current_state = self.task_manager.get_current_state().await;
        let has_tasks = !current_state.is_empty();

        // Broadcast when tasks exist, or once when transitioning to empty
        // so clients clear their task UI.
        if has_tasks || had_tasks {
            if let Err(error) = self.sender.send(LocalMessage::Task(current_state)).await {
                tracing::error!("could not send task state: {error}");
            }
        }

        has_tasks
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::domain::{
        messages::LocalMessage,
        traits::{MockBookChecker, MockMediaChecker, MockMediaStorer, Storer},
        NoSpawner,
    };
    use tokio::sync::mpsc;

    use super::{Monitor, TaskManager};

    #[tokio::test]
    async fn book_scan_failure_does_not_stop_broadcast_or_video_scan() {
        let mut video_checker = MockMediaChecker::new();
        video_checker
            .expect_check_video_information()
            .times(1)
            .returning(|| Ok(()));
        let mut book_checker = MockBookChecker::new();
        book_checker
            .expect_check_book_information()
            .times(1)
            .returning(|| Err(anyhow::anyhow!("book scan failed")));
        let store: Storer = Arc::new(MockMediaStorer::new());
        let task_manager = Arc::new(TaskManager::new(Arc::new(NoSpawner::new())));
        let (sender, mut receiver) = mpsc::channel(4);
        let monitor = Monitor {
            checker: Arc::new(video_checker),
            book_checker: Arc::new(book_checker),
            task_manager,
            store,
            sender,
        };

        let has_tasks = monitor.run_cycle(0, true).await;

        assert!(!has_tasks);
        let message = receiver.recv().await.expect("task state should be broadcast");
        let LocalMessage::Task(state) = message else {
            panic!("expected task state message");
        };
        assert!(state.is_empty());
    }
}
