use crate::domain::messages::{ReceivedRemoteMessage, RemoteMessage};
use crate::domain::traits::RemotePlayer;
use async_trait::async_trait;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
};
use futures::stream::{SplitSink, SplitStream};
use futures::{sink::SinkExt, stream::StreamExt};
use std::net::SocketAddr;
use std::option::Option;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;

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
    pub fn create(
        ws: WebSocketUpgrade,
        who: SocketAddr,
        on_message: Sender<ReceivedRemoteMessage>,
    ) -> (RemoteBrowserPlayer, Response) {
        let (in_tx, in_rx) = channel(100);
        let (out_tx, mut out_rx) = channel(100);

        let runner = RemoteBrowserPlayer { in_tx };

        let response = ws.on_upgrade(move |socket| handle_socket(socket, who, in_rx, out_tx));

        tokio::spawn(async move {
            while let Some(message) = out_rx.recv().await {
                if let Err(err) = on_message
                    .send(ReceivedRemoteMessage {
                        message,
                        from_address: who,
                    })
                    .await
                {
                    tracing::error!("error forwarding message: {}", err);
                }
            }
        });

        (runner, response)
    }
}

async fn handle_socket(
    socket: WebSocket,
    who: SocketAddr,
    input: Receiver<Vec<u8>>,
    output: Sender<RemoteMessage>,
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
            tracing::info!("broken pipe");
            break;
        }

        if let Some(msg) = buffer {
            if sender.send(msg.into()).await.is_err() {
                tracing::warn!("lost connection");
                break;
            }
        }
    }
}

async fn handle_receiving(output: Sender<RemoteMessage>, mut receiver: SplitStream<WebSocket>) {
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(txt) = msg {
            tracing::info!("websocket received: {}", txt);
        } else if let Message::Binary(msg) = msg {
            match serde_json::from_slice::<RemoteMessage>(&msg) {
                Ok(player) => {
                    // tracing::info!("Received state: {:?}", player);
                    if let Err(e) = output.send(player).await {
                        tracing::info!("output.send failed: {}", e);
                    }
                }
                Err(error) => {
                    tracing::error!("Failed to deserialize RemoteMessage: {}", error);
                }
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
                Ok(a)  => tracing::info!("{:?} messages sent to {}", a, who),
                Err(a) => tracing::info!("Error sending messages {:?}", a)
            }
            recv_task.abort();
        },
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(b)  => tracing::debug!("Received {:?} messages", b),
                Err(b) => tracing::debug!("Error receiving messages {:?}", b)
            }
            send_task.abort();
        }
    }

    tracing::info!("Websocket context {} destroyed", who);
}
