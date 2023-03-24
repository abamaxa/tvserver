use crate::domain::traits::{ProcessSpawner, Task};
use async_trait::async_trait;

pub struct NoSpawner {}

impl NoSpawner {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ProcessSpawner for NoSpawner {
    async fn execute(&self, name: &str, cmd: &str, args: Vec<&str>) -> Task {
        panic!("no spawn: {} {} {:?}", name, cmd, args)
    }
}
