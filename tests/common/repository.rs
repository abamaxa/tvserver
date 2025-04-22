use std::sync::Arc;
use app_lib::adaptors::SqlRepository;
use app_lib::domain::traits::Repository;

pub async fn get_repository() -> Repository {
    Arc::new(SqlRepository::new(":memory:", None).await.unwrap())
}
