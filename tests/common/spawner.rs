use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tvserver::domain::messages::TaskState;
use tvserver::domain::traits::{MockTaskMonitor, ProcessSpawner, Spawner, Task};
use tvserver::domain::TaskType;

struct FakeSpawner {
    fixture: PathBuf,
}

#[async_trait]
impl ProcessSpawner for FakeSpawner {
    async fn execute(&self, name: &str, cmd: &str, _args: Vec<&str>) -> Task {
        let mut task = MockTaskMonitor::new();

        let output = String::from_utf8(tokio::fs::read(&self.fixture).await.unwrap()).unwrap();
        let name = name.to_string();
        let command = cmd.to_string();

        task.expect_get_state().returning(move || TaskState {
            key: String::from("key 0"),
            name: name.to_string(),
            display_name: command.to_string(),
            finished: true,
            eta: 0,
            percent_done: 100.0,
            size_details: "".to_string(),
            error_string: "".to_string(),
            rate_details: "".to_string(),
            process_details: output.to_owned(),
            task_type: TaskType::AsyncProcess,
        });

        Arc::new(task)
    }
}

struct NoSpawner {}

#[async_trait]
impl ProcessSpawner for NoSpawner {
    async fn execute(&self, name: &str, cmd: &str, args: Vec<&str>) -> Task {
        panic!("no spawn: {} {} {:?}", name, cmd, args)
    }
}

pub async fn get_spawner(fixture: &Path) -> Spawner {
    let spawner = FakeSpawner {
        fixture: fixture.to_path_buf(),
    };

    Arc::new(spawner)
}

pub fn get_no_spawner() -> Spawner {
    Arc::new(NoSpawner {})
}
