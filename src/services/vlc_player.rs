use std::io::{BufRead, BufReader, Write};
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time;

use crate::domain::traits::Player;

#[derive(Debug)]
pub struct VLCPlayer {
    stdin_tx: Mutex<Sender<String>>,
    stdout_rx: Mutex<Receiver<String>>,
}

impl Player for VLCPlayer {
    fn send_command(&self, command: &str, wait_secs: i32) -> Result<String, String> {
        match self
            .stdin_tx
            .lock()
            .unwrap()
            .send(command.to_string() + "\n")
        {
            Ok(_) => Ok(self.read_result(wait_secs)),
            Err(err) => Err(err.0),
        }
    }
}

impl VLCPlayer {
    pub fn new() -> VLCPlayer {
        let (stdin_tx, stdin_rx) = channel();
        let (stdout_tx, stdout_rx) = channel();

        let runner = VLCPlayer {
            stdin_tx: Mutex::new(stdin_tx),
            stdout_rx: Mutex::new(stdout_rx),
        };

        thread::spawn(move || {
            VLCPlayer::run_vlc(&stdin_rx, stdout_tx);
        });

        println!("{}", runner.read_result(1));

        runner
    }

    fn run_vlc(input: &Receiver<String>, output: Sender<String>) {
        const CMD: &str = "/Applications/VLC.app/Contents/MacOS/VLC";
        let mut child = Command::new(CMD)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .args(["--extraintf", "rc", "--fullscreen"])
            .spawn()
            .expect("spawn child");

        let child_stdout = child.stdout.take().unwrap();
        let mut child_stdin = child.stdin.take().unwrap();

        thread::spawn(move || {
            let mut buffer = String::new();
            let mut child_out = BufReader::new(child_stdout);

            while child_out.read_line(&mut buffer).unwrap() != 0 {
                let msg = buffer.clone(); // trim_end().to_string();
                output.send(msg).unwrap();
                buffer.clear();
            }
        });

        loop {
            if let Ok(msg) = input.recv() {
                child_stdin
                    .write_all(msg.as_bytes())
                    .expect("lost stdin to vlc");
            }
        }
    }

    fn read_result(&self, wait_secs: i32) -> String {
        let mut result = String::new();
        let mut counter = 0;

        while counter * 10 <= wait_secs {
            if let Ok(buffer) = self.stdout_rx.lock().unwrap().try_recv() {
                result += &buffer;
                counter = 0;
                if buffer.starts_with("+----") {
                    break;
                }
            } else {
                thread::sleep(time::Duration::from_millis(100));
                counter += 1;
            }
        }

        result
    }
}

#[cfg(test)]
mod test {

    use super::{Player, VLCPlayer};

    #[test]
    #[ignore]
    fn test_run_vlc() {
        let vlc = VLCPlayer::new();

        let mut result = vlc.send_command("help", 1);

        println!("Help from VLC: {}", result.unwrap());

        result = vlc.send_command("add file:///Users/chris2/Movies/Abigails Party.avi", 1);

        println!("Add file from VLC: {}", result.unwrap());

        result = vlc.send_command("info", 1);

        println!("Info from VLC: {}", result.unwrap());

        let _ = vlc.send_command("quit", 1);
    }
}
