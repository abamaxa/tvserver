use anyhow::Error;
use crate::domain::messagebus::MessageFilter;
use crate::domain::services::HistoryService;
use crate::services::{
    MetaDataManager, Monitor,
};
use tokio::task::JoinHandle;

use super::context::{Context, create_context};

pub struct TVServer {
    context: Context,
    monitor_handle: JoinHandle<()>,
    metadata_manager: JoinHandle<()>,
    history_service: JoinHandle<()>,
}

impl TVServer {
    pub async fn new() -> Result<Self, Error> {
        let context = create_context().await?;
        
        let monitor_handle = Monitor::start(
            context.get_checker(),      
            context.get_task_manager(),
            context.get_store(),
            context.get_local_sender(),
        );
    
        let metadata_manager = MetaDataManager::consume(
            context.get_repository(),
            context.get_store(),
            context.listen_for_messages(MessageFilter::All).await.unwrap(),
            context.get_local_sender(),
            context.get_spawner(),
        );

        let hs = HistoryService::new(  
            context.get_repository(),
            context.get_local_sender(),
        );

        let history_service = hs.observe(
            context.listen_for_messages(MessageFilter::PlayerState).await.unwrap()
        );
        
        Ok(Self { context, monitor_handle, metadata_manager, history_service })
    }

    pub fn get_context(&self) -> &Context {
        &self.context
    }

    pub fn shutdown(&self) {
        self.monitor_handle.abort();
        self.metadata_manager.abort();
        self.history_service.abort();
    }
}