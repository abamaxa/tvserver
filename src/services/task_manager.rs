use crate::domain::messages::TaskState;
use crate::domain::traits::{ProcessSpawner, Spawner, Storer, Task};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinSet;

#[derive(Clone)]
pub struct TaskManager {
    current_tasks: Arc<RwLock<BTreeMap<String, Task>>>,
    spawner: Spawner,
}

#[async_trait]
impl ProcessSpawner for TaskManager {
    async fn execute(&self, name: &str, cmd: &str, args: Vec<&str>) -> Task {
        let task = self.spawner.execute(name, cmd, args).await;
        self.add(task.clone()).await;
        task
    }
}

impl TaskManager {
    pub fn new(spawner: Spawner) -> Self {
        TaskManager {
            spawner,
            current_tasks: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub async fn add(&self, task: Task) -> Option<Task> {
        let key = task.get_key();
        self.current_tasks.write().await.insert(key, task)
    }

    pub async fn remove(&self, key: &str, store: Storer) -> Result<()> {
        let key = String::from(key);
        if let Some(task) = self.current_tasks.write().await.remove(&key) {
            if !task.has_finished() {
                task.terminate();
                return task.cleanup(&store, true).await;
            }
        }
        Ok(())
    }

    pub async fn get_current_state(&self) -> Vec<TaskState> {
        // get a copy of the tasks and then release the lock so we can't
        // deadlock when waiting to lock a Task when that task is locked on
        // another async thread that is waiting to lock the current_tasks map.
        let mut result_set = JoinSet::new();
        for item in self.cloned_task_list().await {
            result_set.spawn(async move { item.get_state().await });
        }

        let mut results: Vec<TaskState> = Vec::with_capacity(result_set.len());
        while let Some(result) = result_set.join_next().await {
            if let Ok(state) = result {
                results.push(state);
            }
        }

        results
    }

    pub async fn cloned_task_list(&self) -> Vec<Task> {
        self.current_tasks.read().await.values().cloned().collect()
    }

    pub async fn cleanup(&self, store: &Storer) {
        let mut task_set = JoinSet::new();
        for task in self.cloned_task_list().await {
            #[allow(clippy::redundant_closure_call)]
            task_set.spawn((|store: Storer| async move {
                let mut result: Option<String> = None;
                if task.has_finished() && task.cleanup(&store, false).await.is_ok() {
                    result = Some(task.get_key());
                }
                result
            })(store.clone()));
        }

        let mut keys_to_delete = vec![];
        while let Some(res) = task_set.join_next().await {
            if let Ok(Some(key)) = res {
                keys_to_delete.push(key);
            }
        }

        let mut current_tasks = self.current_tasks.write().await;
        for key in keys_to_delete {
            current_tasks.remove(&key);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::domain::{
        traits::{MockMediaStorer, MockTaskMonitor},
        NoSpawner, TaskType,
    };
    use mockall::TimesRange;
    use tokio::task::JoinSet;

    #[tokio::test]
    async fn test_current_tasks() {
        const STILL_RUNNING: &str = "still running";

        let spawner = Arc::new(NoSpawner::new());
        let storer = make_storer(0.into());

        let task_manager = TaskManager::new(spawner);
        let task_finished = make_task("1", true);
        let task_running = make_task(STILL_RUNNING, false);

        assert!(task_manager.add(task_finished.clone()).await.is_none());
        assert!(task_manager.add(task_finished.clone()).await.is_some());
        assert!(task_manager.add(task_running.clone()).await.is_none());

        let states = task_manager.get_current_state().await;

        assert_eq!(states.len(), 2);

        task_manager.cleanup(&storer).await;

        let states = task_manager.get_current_state().await;

        assert_eq!(states.len(), 1);
        assert_eq!(states.first().unwrap().name, STILL_RUNNING);

        task_manager.remove(STILL_RUNNING, storer).await.unwrap();

        let states = task_manager.get_current_state().await;

        assert_eq!(states.len(), 0);
    }

    #[tokio::test]
    async fn test_join_set() {
        let mut js = JoinSet::new();

        for i in 1..10 {
            js.spawn(async move { i * 2 });
        }

        while let Some(res) = js.join_next().await {
            println!("{}", res.unwrap());
        }
    }

    fn make_task(key: &str, finished: bool) -> Task {
        let mut mock_task = MockTaskMonitor::new();
        let key = key.to_string();
        let name = key.to_string();

        mock_task.expect_has_finished().return_const(finished);
        mock_task.expect_terminate().return_const(());
        mock_task.expect_cleanup().returning(|_, _| Ok(()));
        mock_task
            .expect_get_key()
            .returning(move || key.to_string());
        mock_task.expect_get_state().returning(move || TaskState {
            key: String::from("key"),
            name: name.to_string(),
            display_name: name.to_string(),
            finished,
            eta: 0,
            percent_done: 0.0,
            size_details: "".to_string(),
            error_string: "".to_string(),
            rate_details: "".to_string(),
            process_details: "".to_string(),
            task_type: TaskType::AsyncProcess,
        });

        Arc::new(mock_task)
    }

    fn make_storer(count_move_file: TimesRange) -> Storer {
        let mut mock_store = MockMediaStorer::new();

        mock_store
            .expect_add_file()
            .times(count_move_file)
            .returning(|_| Ok(()));

        Arc::new(mock_store)
    }
}
