use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSource {
    pub host_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

// Source passwords must never appear in task descriptions or diagnostic output.
impl std::fmt::Debug for DownloadSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DownloadSource")
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseDownloadSourceRequest {
    #[serde(flatten)]
    pub source: DownloadSource,
    #[serde(default)]
    pub collection: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSourceThumbnailRequest {
    #[serde(flatten)]
    pub source: DownloadSource,
    pub path: String,
}
