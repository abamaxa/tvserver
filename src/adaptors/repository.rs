use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::SqlitePool;
use std::{env, path};
use sqlx::Error;

pub async fn get_database() -> Result<SqlitePool, Error> {
    let url = env::var("DATABASE_URL").expect("DATABASE_URL environment variable is not set");

    SqlitePool::connect(&url).await
}


pub async fn do_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
    let m = Migrator::new(path::Path::new("./migrations")).await?;

    m.run(pool).await
}

/*
async fn create_user(State(pool): State<SqlitePool>, Json(payload): Json<CreateUser>) -> (StatusCode, Json<User>) {
    // curl -v -X POST -H 'Content-Type: application/json'  http://localhost:3000/user -d '{"username": "john"}'

    let sql = "INSERT INTO users (name) VALUES(?1)".to_string();

    let id = sqlx::query::<_>(&sql)
        .bind(&payload.username)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

    let user = User {
        id,
        username: payload.username
    };

    println!("laugh: @{}", user.laugh());

    (StatusCode::CREATED, Json(user))
}*/