use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub struct StdSubprocess {
    //stdin_tx: Sender<String>,
    //stdout_rx: Receiver<String>
}

pub struct AsyncCommand {
    command: String,
    args: Vec<String>,
}

impl AsyncCommand {
    pub fn execute(cmd: &str, args: Vec<&str>) {
        println!("executing: {} {:?}", cmd, args);

        let string_args = args.iter().map(|s| String::from(*s)).collect();

        let async_command = Self {
            command: String::from(cmd),
            args: string_args,
        };

        let ptr = Arc::new(async_command);

        tokio::spawn(async move {
            let str_args = ptr.args.iter().map(|s| s.as_str()).collect();
            match Self::command(ptr.command.as_str(), str_args).await {
                Ok(_) => println!("download succeeded - {} {:?}", ptr.command, ptr.args),
                Err(e) => println!("download failed {} - {} {:?}", e, ptr.command, ptr.args),
            };
        });
    }

    pub async fn command(cmd: &str, args: Vec<&str>) -> anyhow::Result<bool> {
        let child = Command::new(cmd).args(&args).output();

        let output = child.await?;

        print!(
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

impl StdSubprocess {
    pub async fn _run(cmd: &str, args: Vec<String>, output: Sender<String>) {
        let mut child = Command::new(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .args(args)
            .spawn()
            .expect("spawn child");

        let child_stdout = child.stdout.take().unwrap();
        //let child_stdin = child.stdin.take().unwrap();

        let mut stdout_task = tokio::spawn(async move {
            let mut buffer = String::new();
            let mut child_out = BufReader::new(child_stdout);
            loop {
                match child_out.read_line(&mut buffer).await {
                    Ok(read) => {
                        if read == 0 {
                            println!("read stdout returned nothing");
                            break;
                        }
                    }
                    Err(e) => {
                        println!("read stdout returned failed: {}", e);
                        break;
                    }
                }

                match output.send(buffer.clone()).await {
                    Ok(_) => buffer.clear(),
                    Err(e) => {
                        println!("could not copy stdout to channel: {}", e);
                        break;
                    }
                }
            }
        });

        let mut stdin_task = tokio::spawn(async move {});

        tokio::select! {
            _ = (&mut stdin_task) => {
                stdout_task.abort();
            },
            _ = (&mut stdout_task) => {
                stdin_task.abort();
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_execute_command() {
        assert!(AsyncCommand::command("ls", vec!["-l"]).await.unwrap());
    }
}
