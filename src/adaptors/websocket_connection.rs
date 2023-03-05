use crate::domain::events::RemoteMessage;
use async_trait::async_trait;
use axum::response::Response;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
};
use futures::stream::{SplitSink, SplitStream};
use futures::{sink::SinkExt, stream::StreamExt};
use std::net::SocketAddr;
use std::option::Option;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;

#[async_trait]
pub trait RemotePlayer: Send + Sync {
    async fn send(&self, message: RemoteMessage) -> Result<StatusCode, String>;
}

#[derive(Debug)]
pub struct RemoteBrowserPlayer {
    in_tx: Sender<Vec<u8>>,
}

#[async_trait]
impl RemotePlayer for RemoteBrowserPlayer {
    async fn send(&self, message: RemoteMessage) -> Result<StatusCode, String> {
        let as_bytes: Vec<u8> = match serde_json::to_vec(&message) {
            Ok(result) => result,
            Err(e) => return Err(e.to_string()),
        };

        match self.in_tx.send(as_bytes).await {
            Ok(_) => Ok(StatusCode::OK),
            Err(err) => Err(err.to_string()),
        }
    }
}

impl RemoteBrowserPlayer {
    pub fn from(ws: WebSocketUpgrade, who: SocketAddr) -> (RemoteBrowserPlayer, Response) {
        let (in_tx, in_rx) = channel(100);
        let (out_tx, mut out_rx) = channel(100);

        let runner = RemoteBrowserPlayer { in_tx };

        let response = ws.on_upgrade(move |socket| handle_socket(socket, who, in_rx, out_tx));

        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                println!("websocket {}, received: {:?}", who, msg);
            }
        });

        (runner, response)
    }
}

async fn handle_socket(
    socket: WebSocket,
    who: SocketAddr,
    input: Receiver<Vec<u8>>,
    output: Sender<String>,
) {
    let (sender, receiver) = socket.split();

    let send_task = tokio::spawn(async move { handle_sending(input, sender).await });

    let recv_task = tokio::spawn(async move { handle_receiving(output, receiver).await });

    wait_for_socket_to_close(who, send_task, recv_task).await
}

async fn handle_sending(mut input: Receiver<Vec<u8>>, mut sender: SplitSink<WebSocket, Message>) {
    loop {
        let buffer: Option<Vec<u8>>;
        if let Some(msg) = input.recv().await {
            buffer = Some(msg);
        } else {
            println!("broken pipe");
            break;
        }

        if let Some(msg) = buffer {
            if sender.send(msg.into()).await.is_err() {
                println!("lost connection");
                break;
            }
        }
    }
}

async fn handle_receiving(output: Sender<String>, mut receiver: SplitStream<WebSocket>) {
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(t) = msg {
            if let Err(e) = output.send(t).await {
                println!("output.send failed: {}", e);
            }
        }
    }
}

async fn wait_for_socket_to_close(
    who: SocketAddr,
    mut send_task: JoinHandle<()>,
    mut recv_task: JoinHandle<()>,
) {
    // If any one of the tasks exit, abort the other.
    tokio::select! {
        rv_a = (&mut send_task) => {
            match rv_a {
                Ok(a)  => println!("{:?} messages sent to {}", a, who),
                Err(a) => println!("Error sending messages {:?}", a)
            }
            recv_task.abort();
        },
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(b)  => println!("Received {:?} messages", b),
                Err(b) => println!("Error receiving messages {:?}", b)
            }
            send_task.abort();
        }
    }

    println!("Websocket context {} destroyed", who);
}
