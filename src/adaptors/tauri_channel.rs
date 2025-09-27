#[cfg(not(feature = "webserver"))]
use crate::domain::messages::{ReceivedRemoteMessage, RemoteMessage};
#[cfg(not(feature = "webserver"))]
use crate::domain::traits::{RemotePlayer, SendError};
#[cfg(not(feature = "webserver"))]
use async_trait::async_trait;
#[cfg(not(feature = "webserver"))]
use axum::http::StatusCode;
#[cfg(not(feature = "webserver"))]
use tauri::{AppHandle, Runtime};
#[cfg(not(feature = "webserver"))]
use tauri::{Emitter, Listener};
#[cfg(not(feature = "webserver"))]
use tokio::sync::mpsc::{channel, Receiver, Sender};
#[cfg(not(feature = "webserver"))]
use tokio::task::JoinHandle;

#[cfg(not(feature = "webserver"))]
/// A struct that implements RemotePlayer for Tauri channels
#[derive(Debug)]
pub struct TauriChannelPlayer {
    app: AppHandle,
    in_tx: Sender<RemoteMessage>,
    channel_name: String,
}

#[cfg(not(feature = "webserver"))]
#[async_trait]
impl RemotePlayer for TauriChannelPlayer {
    async fn send(&self, message: RemoteMessage) -> Result<StatusCode, SendError> {
        // First try to send via the internal channel
        match self.in_tx.send(message.clone()).await {
            Ok(_) => Ok(StatusCode::OK),
            Err(_) => {
                // If the internal channel fails, try to emit directly to the frontend
                match self.app.emit(&self.channel_name, message) {
                    Ok(_) => Ok(StatusCode::OK),
                    Err(err) => Err(SendError::Other(err.to_string())),
                }
            }
        }
    }
}

#[cfg(not(feature = "webserver"))]
impl TauriChannelPlayer {
    pub fn create(app: AppHandle, channel_name: String) -> TauriChannelPlayer {
        let (in_tx, in_rx) = channel(100);

        // Spawn a task to forward messages from the channel to the frontend
        tokio::spawn(handle_sending(app.clone(), in_rx, channel_name.clone()));

        TauriChannelPlayer {
            app,
            in_tx,
            channel_name,
        }
    }
}

#[cfg(not(feature = "webserver"))]
/// Handle sending messages to the frontend
async fn handle_sending(app: AppHandle, mut input: Receiver<RemoteMessage>, channel_name: String) {
    loop {
        match input.recv().await {
            Some(message) => {
                if let Err(e) = app.emit(&channel_name, message) {
                    tracing::error!("Error sending message to frontend: {}", e);
                }
            }
            _ => {}
        }
    }
}

#[cfg(not(feature = "webserver"))]
/// Setup a channel listener for incoming messages from the frontend
pub fn setup_frontend_listener<R: Runtime>(
    app: AppHandle<R>,
    channel_name: &str,
    output: Sender<ReceivedRemoteMessage>,
) -> JoinHandle<()> {
    let event_name = channel_name.to_string();
    tokio::spawn(async move {
        // Use app.listen which requires Listener trait
        let _unlisten = app.listen(&event_name.clone(), move |event| {
            // event.payload() returns &str directly, no need for if let Some
            let payload = event.payload();
            match serde_json::from_str::<RemoteMessage>(payload) {
                Ok(message) => {
                    let received = ReceivedRemoteMessage {
                        message,
                        from_address: event_name.clone(),
                    };

                    // Spawn a task to send the message to the message bus
                    let output_clone = output.clone();
                    tokio::spawn(async move {
                        if let Err(e) = output_clone.send(received).await {
                            tracing::error!("Error forwarding message from frontend: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!(
                        "Error deserializing message from frontend: {} payload: {}",
                        e,
                        payload
                    );
                }
            }
        });

        // Keep the task alive as long as the application is running
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        }
    })
}