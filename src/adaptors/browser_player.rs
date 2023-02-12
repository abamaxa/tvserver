use std::{net::SocketAddr};
use std::option::Option;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use axum::{
    http::StatusCode,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
};
use axum::response::Response;
use futures::{sink::SinkExt, stream::StreamExt};
use async_trait::async_trait;
use crate::domain::events::RemoteMessage;

#[async_trait]
pub trait RemotePlayer: Send + Sync {
    async fn execute(&self, message: RemoteMessage) -> Result<StatusCode, String>;
}

#[derive(Debug)]
pub struct RemoteBrowserPlayer {
    in_tx: Sender<Vec<u8>>,
    // out_rx: Receiver<String>,
}

#[async_trait]
impl RemotePlayer for RemoteBrowserPlayer {

    async fn execute(&self, message: RemoteMessage) -> Result<StatusCode, String> {
        let as_bytes: Vec<u8>;

        match serde_json::to_vec(&message) {
            Ok(result) => as_bytes = result,
            Err(e) => return Err(e.to_string()),
        }

        match self.in_tx.send(as_bytes).await {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(err.to_string()),
        }
    }
}

impl RemoteBrowserPlayer {

    pub fn from(ws: WebSocketUpgrade, who: SocketAddr) -> (RemoteBrowserPlayer, Response) {
        let (in_tx,in_rx) = channel(100);
        let (out_tx,mut out_rx) = channel(100);

        let runner = RemoteBrowserPlayer {in_tx};

        let response = ws.on_upgrade(
            move | socket | RemoteBrowserPlayer::handle_socket(socket, who, in_rx, out_tx)
        );

        tokio::spawn(async move {
            loop {
                if let Some(msg) = out_rx.recv().await {
                    println!("out_rx received: {:?}", msg);
                }
            }
        });

        (runner, response)
    }

    async fn handle_socket(socket: WebSocket, who: SocketAddr, mut input: Receiver<Vec<u8>>, output: Sender<String>) {

        // By splitting socket we can send and receive at the same time. In this example we will send
        // unsolicited messages to client based on some sort of server's internal event (i.e .timer).
        let (mut sender, mut receiver) = socket.split();

        // Spawn a task that will push several messages to the client (does not matter what client does)
        let mut send_task = tokio::spawn(async move {
            loop {
                let mut buffer: Option<Vec<u8>> = None;
                if let Some(msg) = input.recv().await {
                    buffer = Some(msg);
                }

                if let Some(msg) = buffer {
                    if sender.send(msg.into()).await.is_err() {
                        // log lost connection
                        println!("lost connection");
                        break;
                    }
                }
            }
        });

        // This second task will receive messages from client and print them on server console
        let mut recv_task = tokio::spawn(async move {
            while let Some(Ok(msg)) = receiver.next().await {
                if let Message::Text(t) = msg {
                    println!("{} sent {}", who, t);
                    if let Err(e) = output.send(t).await {
                        println!("output.send failed: {}", e);
                    }
                }
            }
        });

        // If any one of the tasks exit, abort the other.
        tokio::select! {
            rv_a = (&mut send_task) => {
                match rv_a {
                    Ok(a) => println!("{:?} messages sent to {}", a, who),
                    Err(a) => println!("Error sending messages {:?}", a)
                }
                recv_task.abort();
            },
            rv_b = (&mut recv_task) => {
                match rv_b {
                    Ok(b) => println!("Received {:?} messages", b),
                    Err(b) => println!("Error receiving messages {:?}", b)
                }
                send_task.abort();
            }
        }

        // returning from the handler closes the websocket connection
        println!("Websocket context {} destroyed", who);
    }
}