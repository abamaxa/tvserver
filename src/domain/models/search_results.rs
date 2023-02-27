use serde::{Deserialize, Serialize};
use crate::domain::SearchEngineType;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DownloadableItem {
    pub title: String,
    pub description: String,
    pub link: String,
    pub engine: SearchEngineType,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SearchResults<T> {
    pub results: Option<Vec<T>>,
    pub error: Option<String>,
}


impl<T> SearchResults<T> {
    pub fn success(results: Vec<T>) -> Self {
        SearchResults{results: Some(results), error: None}
    }

    pub fn error(message: &str) -> Self {
        SearchResults{error: Some(message.to_string()), results: None}
    }
}


/*use serde::{Deserialize, Serialize};


#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
}

#[derive(Debug, Serialize,Deserialize, Clone, Eq, Hash, PartialEq, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
}


impl User {
    pub fn laugh(&self) -> String {
        String::from("ha ha")
    }
}*/

