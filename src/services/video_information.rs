use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender, MediaEvent};
use crate::domain::services::generate_video_metadatas;
use crate::domain::traits::Repository;
use tokio::task::JoinHandle;
use tokio::sync::Semaphore;
use std::sync::Arc;
use once_cell::sync::Lazy;

// Create a static semaphore with a capacity of half the number of CPUs
static WORKER_SEMAPHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| {
    let num_cpus = num_cpus::get();
    let concurrent_limit = std::cmp::max(1, num_cpus / 2);
    tracing::info!("Using {} CPU cores, limiting to {} concurrent metadata tasks", num_cpus, concurrent_limit);
    Arc::new(Semaphore::new(concurrent_limit))
});

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
        match event {
            MediaEvent::MediaAvailable(event) => {
                // Clone the values needed for the task
                let full_path = event.full_path;
                let search = event.search;
                let repo = self.repo.clone();
                let semaphore = WORKER_SEMAPHORE.clone();
                
                // Spawn a new task to process the media event
                tokio::spawn(async move {
                    // Acquire a permit from the semaphore, which will limit concurrent tasks
                    let permit = semaphore.acquire().await.unwrap();
                    
                    // Process the media event
                    if let Err(err) = generate_video_metadatas(full_path, repo, search).await {
                        tracing::error!("processing MediaAvailable: {}", err);
                    }
                    
                    // Permit is automatically dropped when it goes out of scope
                    drop(permit);
                });
            },
            _ => return,
        };
    }
}
