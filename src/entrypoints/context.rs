use sqlx::Error;
use std::sync::Arc;

use crate::adaptors::{FileSystemStore, SqlRepository, TokioProcessSpawner};
use crate::domain::config::{get_database_url, get_movie_dir};
use crate::domain::messagebus::MessageExchange;
use crate::domain::traits::{ FileStorer, Player};
use crate::domain::config::enable_vlc_player;
use crate::services::{
    MediaStore, SearchService, TaskManager, VLCPlayer,
};

use super::api::Context;

pub async fn create_context() -> Result<Context, Error> {
    let messenger = MessageExchange::new();

    let repository = Arc::new(SqlRepository::new(&get_database_url()).await?);

    let player: Option<Arc<dyn Player>> = if enable_vlc_player() {
        Some(Arc::new(VLCPlayer::start()))
    } else {
        None
    };

    let task_manager = Arc::new(TaskManager::new(Arc::new(TokioProcessSpawner::new())));

    let file_storer: FileStorer = Arc::new(FileSystemStore::new(&get_movie_dir()));

    Ok(Context::new(
        Arc::new(MediaStore::new(file_storer, repository.clone(), messenger.get_local_sender())),
        SearchService::new(task_manager.clone()),
        messenger,
        player,
        task_manager,
        repository,
    ))
}
