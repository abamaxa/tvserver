use sqlx::migrate::{MigrateDatabase, MigrateError, Migrator};
use sqlx::sqlite::SqlitePool;
use sqlx::{Error, Sqlite};
use std::path;

use crate::domain::config::{get_database_migration_dir, get_database_url};

pub async fn get_database() -> Result<SqlitePool, Error> {
    let url = get_database_url();

    if url != ":memory:" && !Sqlite::database_exists(&url).await.unwrap_or(false) {
        match Sqlite::create_database(&url).await {
            Ok(_) => tracing::info!("Created db {} successfully", url),
            Err(error) => panic!("error: {}", error),
        }
    }

    SqlitePool::connect(&url).await
}

pub async fn do_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
    let migrations_dir = get_database_migration_dir();

    let m = Migrator::new(path::Path::new(&migrations_dir)).await?;

    m.run(pool).await
}
