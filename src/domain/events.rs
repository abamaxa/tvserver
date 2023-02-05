use serde::{Serialize, Deserialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Command {
    pub command: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayRequest {
    pub collection: String,
    pub video: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Response {
    pub message: String,
    pub errors: Vec<String>,
}

impl Response {
    pub fn success(message: String) -> Response {
        Response{message: message, ..Default::default()}
    }

    pub fn error(error: String) -> Response {
        Response{errors: vec![error], ..Default::default()}
    }
}
