use crate::common::get_no_spawner;
use std::sync::Arc;
use tvserver::services::TaskManager;

pub fn get_task_manager() -> Arc<TaskManager> {
    Arc::new(TaskManager::new(get_no_spawner()))
}
