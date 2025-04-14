use tvserver::domain::traits::{Checker, Repository, Storer};
use tvserver::domain::messagebus::{LocalMessageExchange, MessageExchange, MessageFilter};
use tvserver::entrypoints::Context;
use tvserver::services::{SearchService, TaskManager};
use std::sync::Arc;
use anyhow::Result;

pub async fn get_context(
    store: Storer, 
    searcher: SearchService, 
    task_manager: Arc<TaskManager>, 
    repository: Repository, 
    checker: Checker) -> Result<Context> 
{
    let local_message_exchange = LocalMessageExchange::new();
    
    let messenger = MessageExchange::new(
        local_message_exchange.new_sender(), 
        local_message_exchange.listen_for_messages("MessageExchange", MessageFilter::All).await?
    );

    Ok(Context::new(
        store,
        searcher,
        messenger,
        task_manager,
        repository,
        checker,
        local_message_exchange,
    ))
}

