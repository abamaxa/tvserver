use sqlx::migrate::{MigrateDatabase, MigrateError, Migrator};
use sqlx::sqlite::SqlitePool;
use sqlx::{Error, Sqlite};
use std::{env, path};

use crate::domain::{DATABASE_MIGRATION_DIR, DATABASE_URL};

pub async fn get_database() -> Result<SqlitePool, Error> {
    let url = env::var(DATABASE_URL).expect("DATABASE_URL environment variable is not set");

    if url != ":memory:" && !Sqlite::database_exists(&url).await.unwrap_or(false) {
        match Sqlite::create_database(&url).await {
            Ok(_) => tracing::info!("Created db {} successfully", url),
            Err(error) => panic!("error: {}", error),
        }
    }

    SqlitePool::connect(&url).await
}

pub async fn do_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
    let migrations_dir =
        env::var(DATABASE_MIGRATION_DIR).unwrap_or_else(|_| String::from("./migrations"));

    let m = Migrator::new(path::Path::new(&migrations_dir)).await?;

    m.run(pool).await
}
