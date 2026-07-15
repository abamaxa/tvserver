use anyhow::Result;
use app_lib::adaptors::FileSystemStore;
use app_lib::domain::messagebus::{LocalMessageExchange, MessageExchange, MessageFilter};
use app_lib::domain::traits::{Checker, FileStorer, Repository, Storer};
use app_lib::entrypoints::Context;
use app_lib::services::{BookStore, SearchService, TaskManager};
use std::sync::Arc;

pub async fn get_context(
    store: Storer,
    searcher: SearchService,
    task_manager: Arc<TaskManager>,
    repository: Repository,
    checker: Checker,
) -> Result<Context> {
    let local_message_exchange = LocalMessageExchange::new();

    let messenger = MessageExchange::new(
        local_message_exchange.new_sender(),
        local_message_exchange
            .listen_for_messages(MessageFilter::All)
            .await?,
    );

    let (book_store, book_file_storer) = get_book_services(repository.clone());

    Ok(Context::new(
        store,
        searcher,
        messenger,
        task_manager,
        repository,
        checker,
        local_message_exchange,
        book_store,
        book_file_storer,
        None,
    ))
}

pub fn get_book_services(repository: Repository) -> (Arc<BookStore>, FileStorer) {
    let test_root =
        std::env::temp_dir().join(format!("tvserver-context-tests-{}", std::process::id()));
    let book_root = test_root.join("books");
    let thumbnail_root = test_root.join("book-thumbnails");
    let book_file_storer: FileStorer = Arc::new(FileSystemStore::new(
        book_root.to_str().expect("book test root should be UTF-8"),
    ));
    let thumbnail_file_storer: FileStorer = Arc::new(FileSystemStore::new(
        thumbnail_root
            .to_str()
            .expect("book thumbnail test root should be UTF-8"),
    ));
    let book_store = Arc::new(BookStore::new_with_roots(
        book_file_storer.clone(),
        thumbnail_file_storer,
        repository,
        &book_root,
        &thumbnail_root,
    ));

    (book_store, book_file_storer)
}
