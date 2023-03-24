use anyhow::anyhow;
use async_trait::async_trait;
use std::process::ExitStatus;
use std::time::{Duration, SystemTime};
use std::{io::Result, marker::Unpin, process::Stdio, sync::Arc};

use crate::domain::config::delay_reaping_tasks;
use crate::domain::messages::TaskState;
use crate::domain::traits::{ProcessSpawner, Storer, Task, TaskMonitor};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

pub struct AsyncCommand {
    command: String,
    args: Vec<String>,
}

impl AsyncCommand {
    pub fn execute(cmd: &str, args: Vec<&str>) {
        tracing::info!("executing: {} {:?}", cmd, args);

        let string_args = args.iter().map(|s| String::from(*s)).collect();

        let async_command = Self {
            command: String::from(cmd),
            args: string_args,
        };

        let ptr = Arc::new(async_command);

        tokio::spawn(async move {
            let str_args = ptr.args.iter().map(|s| s.as_str()).collect();
            match Self::command(ptr.command.as_str(), str_args).await {
                Ok(_) => tracing::info!("succeeded - {} {:?}", ptr.command, ptr.args),
                Err(e) => tracing::error!("failed {} - {} {:?}", e, ptr.command, ptr.args),
            };
        });
    }

    pub async fn command(cmd: &str, args: Vec<&str>) -> anyhow::Result<bool> {
        let child = Command::new(cmd).args(&args).output();

        let output = child.await?;

        tracing::debug!(
            "execute: {} {:?}\nsuccess: {}\nstdout:\n{}stderr:\n{}",
            cmd,
            args,
            output.status.success(),
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        );

        Ok(output.status.success())
    }
}

#[derive(Debug)]
pub struct AsyncSubProcess {
    /*
    Represents an OS process.

    JoinHandle is not Clone, which means this struct can't be clone.
     */
    name: String,
    command: String,
    args: Vec<String>,
    handle: Option<JoinHandle<()>>,
    error_string: Arc<Mutex<String>>,
    output: Arc<RwLock<Vec<String>>>,
    created: SystemTime,
}

#[derive(Default)]
pub struct TokioProcessSpawner {}

impl TokioProcessSpawner {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ProcessSpawner for TokioProcessSpawner {
    async fn execute(&self, name: &str, cmd: &str, args: Vec<&str>) -> Task {
        AsyncSubProcess::execute(name, cmd, args).await
    }
}

impl AsyncSubProcess {
    pub async fn execute(name: &str, cmd: &str, args: Vec<&str>) -> Task {
        tracing::info!("executing: {} {:?}", cmd, args);

        let (stdout_tx, stdout_rx) = channel(10);

        let string_args: Vec<String> = args.iter().map(|s| String::from(*s)).collect();

        let command = String::from(cmd);

        let mut process = Self {
            name: name.to_string(),
            command,
            args: string_args,
            created: SystemTime::now(),
            error_string: Arc::new(Mutex::new(String::new())),
            handle: None,
            output: Arc::new(RwLock::new(Vec::new())),
        };

        process.handle = Some(process.start(stdout_tx));

        process.store_stdio(stdout_rx);

        Arc::new(process)
    }

    pub fn start(&self, stdout_tx: Sender<String>) -> JoinHandle<()> {
        let cmd = self.command.clone();
        let args = self.args.clone();
        let name = self.name.clone();
        let error_string = self.error_string.clone();

        tokio::spawn(async move {
            match Self::run(&cmd, args.clone(), stdout_tx).await {
                Ok(_) => tracing::info!("succeeded - {} {} {:?}", name, cmd, args),
                Err(e) => {
                    *error_string.lock().await = e.to_string();
                    tracing::error!("failed {} - {} {} {:?}", e, name, cmd, args)
                }
            };
        })
    }

    pub async fn run(cmd: &str, args: Vec<String>, output: Sender<String>) -> Result<ExitStatus> {
        let mut child = Command::new(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .args(args)
            .spawn()
            .expect("spawn child");

        let stdout = child.stdout.take().unwrap();
        let output2 = output.clone();

        let out_task = tokio::spawn(async move { Self::copy_stdio(stdout, output2).await });

        let stderr = child.stderr.take().unwrap();
        let err_task = tokio::spawn(async move { Self::copy_stdio(stderr, output).await });

        let result = child.wait().await;

        out_task.abort();
        err_task.abort();

        result
    }

    fn store_stdio(&self, mut stdout_rx: Receiver<String>) {
        let output = self.output.clone();
        tokio::spawn(async move {
            while let Some(msg) = stdout_rx.recv().await {
                output.write().await.push(msg);
            }
            tracing::info!("exited store_stdio");
        });
    }

    async fn copy_stdio<T>(mut child_stdout: T, output: Sender<String>)
    where
        T: AsyncRead + Unpin,
    {
        let mut buffer = [0; 0x1000];
        while let Ok(bytes_read) = child_stdout.read(&mut buffer).await {
            let data = String::from_utf8(buffer[0..bytes_read].to_vec()).unwrap_or_default();
            if let Err(e) = output.send(data).await {
                tracing::error!("could not copy to channel: {}", e);
                break;
            }
        }
    }
}

impl PartialEq<Self> for AsyncSubProcess {
    fn eq(&self, other: &Self) -> bool {
        self.command == other.command && self.args == other.args
    }
}

impl Eq for AsyncSubProcess {}

#[async_trait]
impl TaskMonitor for AsyncSubProcess {
    async fn get_state(&self) -> TaskState {
        let last_message = match self.output.read().await.last() {
            Some(msg) => msg.clone(),
            _ => String::new(),
        };

        TaskState {
            key: self.get_key(),
            name: self.name.clone(),
            display_name: self.name.clone(),
            finished: self.has_finished(),
            eta: 0,
            percent_done: 0.0,
            size_details: "".to_string(),
            error_string: self.error_string.lock().await.clone(),
            rate_details: "".to_string(),
            process_details: last_message,
        }
    }

    fn get_key(&self) -> String {
        format!("{} {:?}", self.command, self.args)
    }

    fn get_seconds_since_finished(&self) -> i64 {
        if self.has_finished() {
            self.created
                .elapsed()
                .unwrap_or_else(|_| Duration::from_secs(0))
                .as_secs()
                .try_into()
                .unwrap_or(1_000_000_000_000)
        } else {
            0
        }
    }

    fn terminate(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }

    fn has_finished(&self) -> bool {
        match &self.handle {
            Some(handle) => handle.is_finished(),
            _ => true,
        }
    }

    async fn cleanup(&self, _store: &Storer) -> anyhow::Result<()> {
        if self.get_seconds_since_finished() < delay_reaping_tasks() {
            Err(anyhow!("not ready to reap"))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Result;
    use std::time::Duration;

    #[tokio::test]
    async fn test_execute_async_command() {
        assert!(AsyncCommand::command("ls", vec!["-l"]).await.unwrap());
    }

    #[tokio::test]
    async fn test_execute_task() -> Result<()> {
        let args =
            vec!["-u", "-c", "import time;print(1);time.sleep(1);print(2);time.sleep(1);print(3)"];

        let task = AsyncSubProcess::execute("Python", "python3", args).await;

        let mut markers: [bool; 3] = [false, false, false];

        loop {
            let status = task.get_state().await;

            let idx = status.process_details.parse::<usize>().unwrap_or(0);

            if idx > 0 {
                markers[idx - 1] = true;
            }

            if status.finished {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        assert!(markers
            .into_iter()
            .filter(|i| !*i)
            .collect::<Vec<bool>>()
            .is_empty());

        Ok(())
    }
}
