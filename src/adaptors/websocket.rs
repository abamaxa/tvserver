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
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct RemoteBrowserPlayer {
    in_tx: Sender<RemoteMessage>,
}

#[async_trait]
impl RemotePlayer for RemoteBrowserPlayer {
    async fn send(&self, message: RemoteMessage) -> Result<StatusCode, String> {
        match self.in_tx.send(message).await {
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
    input: Receiver<RemoteMessage>,
    output: Sender<RemoteMessage>,
) {
    let (sender, receiver) = socket.split();

    let send_task = tokio::spawn(async move { handle_sending(input, sender).await });

    let recv_task = tokio::spawn(async move { handle_receiving(output, receiver, who).await });

    wait_for_socket_to_close(who, send_task, recv_task).await
}

async fn handle_sending(
    mut input: Receiver<RemoteMessage>,
    mut sender: SplitSink<WebSocket, Message>,
) {
    loop {
        let message = match input.recv().await {
            Some(msg) => msg,
            _ => {
                tracing::info!("broken pipe");
                break;
            }
        };

        let result = match message {
            RemoteMessage::Ping(n) => sender.send(Message::Ping(n.to_be_bytes().to_vec())).await,
            _ => {
                let as_bytes: Vec<u8> = match serde_json::to_vec(&message) {
                    Ok(result) => result,
                    Err(e) => {
                        tracing::error!("{}", e.to_string());
                        continue;
                    }
                };

                sender.send(as_bytes.into()).await
            }
        };

        if result.is_err() {
            tracing::warn!("lost connection");
            break;
        }
    }
}

async fn handle_receiving(
    output: Sender<RemoteMessage>,
    mut receiver: SplitStream<WebSocket>,
    who: SocketAddr,
) {
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(txt) => {
                tracing::info!("websocket {} sent text: {}", who, txt)
            }
            Message::Binary(msg) => {
                match serde_json::from_slice::<RemoteMessage>(&msg) {
                    Ok(player) => {
                        // tracing::info!("Received state: {:?}", player);
                        if let Err(e) = output.send(player).await {
                            tracing::error!("output.send failed: {}", e);
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            "failed to deserialize RemoteMessage: {} from {}",
                            error,
                            who
                        );
                    }
                }
            }
            Message::Pong(msg) => {
                tracing::info!("websocket {} pong: {:?}", who, msg);
                let pong_message = RemoteMessage::Pong(who);
                if let Err(e) = output.send(pong_message).await {
                    tracing::error!("output.send pong message failed: {}", e);
                }
            }
            Message::Close(_) => {
                tracing::info!("websocket {} close message", who);
                if let Err(e) = output.send(RemoteMessage::Close(who)).await {
                    tracing::error!("output.send close message failed: {}", e);
                }
            }
            _ => tracing::info!("websocket {} sent: {:?}", who, msg),
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
