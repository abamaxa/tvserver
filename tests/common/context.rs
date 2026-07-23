use anyhow::Result;
use app_lib::domain::messagebus::{LocalMessageExchange, MessageExchange, MessageFilter};
use app_lib::domain::traits::{Checker, Repository, Storer};
use app_lib::entrypoints::{BookRuntime, Context};
use app_lib::services::{SearchService, TaskManager};
use std::{path::Path, sync::Arc};

pub async fn get_context(
    store: Storer,
    searcher: SearchService,
    task_manager: Arc<TaskManager>,
    repository: Repository,
    checker: Checker,
) -> Result<Context> {
    let book_runtime = get_book_services(repository.clone()).await;

    get_context_with_book_services(store, searcher, task_manager, repository, checker, book_runtime)
        .await
}

pub async fn get_context_with_book_services(
    store: Storer,
    searcher: SearchService,
    task_manager: Arc<TaskManager>,
    repository: Repository,
    checker: Checker,
    book_runtime: BookRuntime,
) -> Result<Context> {
    let local_message_exchange = LocalMessageExchange::new();

    let messenger = MessageExchange::new(
        local_message_exchange.new_sender(),
        local_message_exchange
            .listen_for_messages(MessageFilter::All)
            .await?,
    );

    Ok(Context::new(
        store,
        searcher,
        messenger,
        task_manager,
        repository,
        checker,
        local_message_exchange,
        book_runtime,
        None,
    ))
}

pub async fn get_book_services(repository: Repository) -> BookRuntime {
    let test_root =
        std::env::temp_dir().join(format!("tvserver-context-tests-{}", std::process::id()));
    let book_root = test_root.join("books");
    let thumbnail_root = test_root.join("book-thumbnails");

    get_book_services_at(repository, &book_root, &thumbnail_root).await
}

pub async fn get_book_services_at(
    repository: Repository,
    book_root: &Path,
    thumbnail_root: &Path,
) -> BookRuntime {
    let exchange = LocalMessageExchange::new();
    BookRuntime::initialize(repository, exchange.new_sender(), book_root, thumbnail_root).await
}
