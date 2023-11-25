use crate::domain::traits::{Downloader, Repository, Storer, Task};
use crate::services::TaskManager;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};
use tokio::time::{sleep, Duration};

pub struct Monitor {
    store: Storer,
    downloads: Downloader,
    task_manager: Arc<TaskManager>,
}

impl Monitor {
    pub fn start(
        store: Storer,
        downloads: Downloader,
        task_manager: Arc<TaskManager>,
        repo: Repository,
    ) -> JoinHandle<()> {
        task::spawn(async move {
            tracing::info!("starting download monitor");
            let monitor = Self {
                store,
                downloads,
                task_manager,
            };

            loop {
                match monitor.downloads.list_in_progress().await {
                    Ok(results) => monitor.move_completed_downloads(&results).await,
                    Err(e) => {
                        tracing::error!("download monitor could not read torrents list: {:?}", e)
                    }
                }

                monitor.task_manager.cleanup(&monitor.store).await;

                if let Err(err) = &monitor.store.check_video_information(repo.clone()).await {
                    tracing::error!("error checking video info: {}", err);
                }

                sleep(Duration::from_secs(10)).await;
            }
        })
    }

    async fn move_completed_downloads(&self, items: &[Task]) {
        for item in items.iter().filter(|item| item.has_finished()) {
            if let Err(e) = item.cleanup(&self.store, false).await {
                // TODO: distinguish between genuine problems and policy delays in
                // reaping completed tasks
                tracing::debug!("could not move videos: {}", e);
            } else {
                println!("key: {}", item.get_key());
                if let Err(e) = self.downloads.remove(&item.get_key(), true).await {
                    tracing::error!("could not remove video: {}: {}", item.get_key(), e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptors::SqlRepository;
    use anyhow::Result;
    use mockall::predicate;
    use tokio::time;

    use crate::domain::models::test::torrents_from_fixture;
    use crate::domain::models::TorrentTask;
    use crate::domain::traits::{MockMediaDownloader, MockMediaStorer, Task};
    use crate::domain::NoSpawner;

    #[tokio::test]
    async fn test_download_monitor() -> Result<()> {
        let mut mock_downloader = MockMediaDownloader::new();
        let mut counter = 0;
        let list_results = results_from_fixture("torrents_get.json").await?;

        mock_downloader
            .expect_list_in_progress()
            .returning(move || match counter {
                0 => {
                    counter += 1;
                    Ok(list_results.clone())
                }
                _ => Ok(vec![]),
            });

        mock_downloader
            .expect_remove()
            .times(1)
            .with(predicate::eq("2"), predicate::eq(true))
            .returning(|_, _| Ok(()));

        let downloader: Downloader = Arc::new(mock_downloader);

        let mut mock_store = MockMediaStorer::new();

        mock_store.expect_add_file().times(1).returning(|_| Ok(()));

        let store: Storer = Arc::new(mock_store);

        let spawner = Arc::new(NoSpawner::new());

        let task_manager = Arc::new(TaskManager::new(spawner));

        let repo: Repository = Arc::new(SqlRepository::new(":memory:").await.unwrap());

        let monitor_handle = Monitor::start(store, downloader, task_manager, repo);

        // wait for monitor to finish a cycle, if it hasn't finished by then it ought
        // to be a test fail
        time::sleep(time::Duration::from_millis(100)).await;

        monitor_handle.abort();

        Ok(())
    }

    async fn results_from_fixture(name: &str) -> Result<Vec<Task>> {
        let fixture = torrents_from_fixture(name).await?;

        let items = fixture
            .iter()
            .map(|t| Arc::new(TorrentTask::from(t)) as Task)
            .collect();

        Ok(items)
    }
}
