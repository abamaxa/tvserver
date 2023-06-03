use std::sync::Arc;
use tvserver::adaptors::SqlRepository;
use tvserver::domain::traits::Repository;

pub async fn get_repository() -> Repository {
    Arc::new(SqlRepository::new(":memory:").await.unwrap())
}
