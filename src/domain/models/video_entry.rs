use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VideoEntry {
    pub collection: String,
    pub parent_collection: String,
    pub child_collections: Vec<String>,
    pub videos: Vec<String>,
    pub errors: Vec<String>,
}

impl VideoEntry {
    pub fn from(
        collection: String,
        child_collections: Vec<String>,
        videos: Vec<String>,
    ) -> VideoEntry {
        let mut parent_collection = String::new();

        if collection.find('/').is_some() {
            let v: Vec<&str> = collection.rsplitn(2, '/').collect();
            parent_collection = v[1].to_string();
        }

        VideoEntry {
            collection,
            parent_collection,
            child_collections,
            videos,
            ..Default::default()
        }
    }

    pub fn error(error: String) -> VideoEntry {
        VideoEntry {
            errors: vec![error],
            ..Default::default()
        }
    }
}
