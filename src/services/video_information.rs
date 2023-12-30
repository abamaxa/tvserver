use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender, MediaEvent};
use crate::domain::services::generate_video_metadatas;
use crate::domain::traits::Repository;
use tokio::task::JoinHandle;

pub struct MetaDataManager {
    repo: Repository,
    receiver: LocalMessageReceiver,
    _sender: LocalMessageSender,
}

impl MetaDataManager {
    fn new(repo: Repository, receiver: LocalMessageReceiver, sender: LocalMessageSender) -> Self {
        Self {
            repo,
            receiver,
            _sender: sender,
        }
    }

    pub fn consume(
        repo: Repository,
        receiver: LocalMessageReceiver,
        sender: LocalMessageSender,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut manager = Self::new(repo, receiver, sender);
            manager.event_loop().await;
            eprintln!("local event loop exiting");
        })
    }

    async fn event_loop(&mut self) {
        loop {
            match self.receiver.recv().await {
                Ok(msg) => match msg {
                        LocalMessage::Media(event) => self.handle_media_event(event).await,
                        _ => continue,
                 },
                 Err(e) => tracing::error!("event loop got an error: {}", e)
            }
        }
    }

    async fn handle_media_event(&self, event: MediaEvent) {
        let _ = match event {
            MediaEvent::MediaAvailable(event) => {
                if let Err(err) = generate_video_metadatas(event.full_path, self.repo.clone()).await {
                    match err.code {
                        // MetaDataErrorCode::ZeroFileSize => ,
                        _ => tracing::error!("processing MediaAvailable: {}", err)
                    };
                }
            },
            _ => return,
        };
    }
}
