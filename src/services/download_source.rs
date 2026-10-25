use std::time::Duration;

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{Client, RequestBuilder, Url};
use serde_json::{json, Value};

use crate::domain::messages::DownloadSource;
use crate::domain::models::VideoDetails;

/// A connection to one source. Authentication is attached only to its requests.
pub struct RemoteLibrary {
    base: Url,
    client: Client,
    source: DownloadSource,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
        Json, Router,
    };

    fn source(host: &str) -> DownloadSource {
        DownloadSource {
            host_url: host.into(),
            username: None,
            password: None,
        }
    }

    #[test]
    fn preserves_schemes_ports_base_paths_and_encodes_library_paths() {
        for (host, expected) in [
            ("http://db.qcc.cc/", "http://db.qcc.cc"),
            ("https://beta.qcc.cc", "https://beta.qcc.cc"),
            (
                "media.example:4081/library/",
                "http://media.example:4081/library",
            ),
        ] {
            let remote = RemoteLibrary::new(source(host)).unwrap();
            assert_eq!(remote.host_url(), expected);
            assert_eq!(
                remote
                    .api_url("media", "Show/Season 1 & café?#%")
                    .unwrap()
                    .as_str(),
                format!("{expected}/api/media/Show/Season%201%20&%20caf%C3%A9%3F%23%25")
            );
        }
    }

    #[test]
    fn rejects_invalid_hosts_and_paths_and_redacts_source_credentials() {
        for host in [
            "",
            "ftp://example.com",
            "https://user:secret@example.com",
            "https://example.com?q=secret",
            "https://example.com#fragment",
        ] {
            assert!(RemoteLibrary::new(source(host)).is_err(), "accepted {host}");
        }
        let remote = RemoteLibrary::new(source("https://beta.qcc.cc")).unwrap();
        assert!(remote.api_url("media", "Show/../secret").is_err());
        assert!(remote
            .thumbnail_url("https://other.example/api/thumbnails/image.jpg")
            .is_err());
        assert!(remote.thumbnail_url("/etc/passwd").is_err());
        assert!(remote
            .stream_url(&VideoDetails {
                collection: "../escape".into(),
                video: "clip.mp4".into(),
                ..Default::default()
            })
            .is_err());
        let credentials = DownloadSource {
            username: Some("viewer".into()),
            password: Some("secret".into()),
            ..source("https://beta.qcc.cc")
        };
        let debug = format!("{credentials:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("viewer"));
    }

    #[test]
    fn accepts_thumbnail_paths_and_scopes_authentication_to_the_source() {
        let remote = RemoteLibrary::new(DownloadSource {
            username: Some("viewer".into()),
            password: Some("secret".into()),
            ..source("https://beta.qcc.cc")
        })
        .unwrap();
        for path in [
            "image 1.jpg",
            "/api/thumbnails/image 1.jpg",
            "https://beta.qcc.cc/api/thumbnails/image%201.jpg",
        ] {
            assert_eq!(
                remote.thumbnail_url(path).unwrap().as_str(),
                "https://beta.qcc.cc/api/thumbnails/image%201.jpg"
            );
        }
        let request = remote
            .get(remote.api_url("media", "").unwrap())
            .build()
            .unwrap();
        assert_eq!(
            request.headers()[reqwest::header::AUTHORIZATION],
            "Basic dmlld2VyOnNlY3JldA=="
        );
        let anonymous = RemoteLibrary::new(source("http://db.qcc.cc")).unwrap();
        let request = anonymous
            .get(anonymous.api_url("media", "").unwrap())
            .build()
            .unwrap();
        assert!(!request
            .headers()
            .contains_key(reqwest::header::AUTHORIZATION));
    }

    #[tokio::test]
    async fn browses_and_loads_thumbnails_with_basic_auth_and_reports_rejected_credentials() {
        async fn handler(headers: HeaderMap, uri: axum::http::Uri) -> axum::response::Response {
            if headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Basic dmlld2VyOnNlY3JldA==")
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            if uri.path().starts_with("/library/api/thumbnails/") {
                return ([("content-type", "image/jpeg")], vec![0xff, 0xd8, 0xff]).into_response();
            }
            Json(json!({ "Collection": { "collection": "Show", "child_collections": [], "series": {}, "videos": [], "errors": [] } })).into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/library/api/media/{*path}", get(handler))
            .route("/library/api/thumbnails/{*path}", get(handler));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let host = format!("http://{address}/library");
        let remote = RemoteLibrary::new(DownloadSource {
            username: Some("viewer".into()),
            password: Some("secret".into()),
            ..source(&host)
        })
        .unwrap();
        assert_eq!(
            remote.browse("Show & café?#%").await.unwrap()["Collection"]["collection"],
            "Show"
        );
        assert_eq!(
            remote
                .thumbnail("/api/thumbnails/image 1.jpg")
                .await
                .unwrap()["dataUrl"],
            "data:image/jpeg;base64,/9j/"
        );
        let anonymous = RemoteLibrary::new(source(&host)).unwrap();
        assert!(anonymous
            .browse("Show")
            .await
            .unwrap_err()
            .to_string()
            .contains("HTTP 401"));
        server.abort();
    }
}

impl RemoteLibrary {
    pub fn new(source: DownloadSource) -> Result<Self> {
        let host = source.host_url.trim();
        let host = if host.contains("://") {
            host.to_string()
        } else {
            format!("http://{host}")
        };
        let mut base = Url::parse(&host).context("Enter a valid HTTP or HTTPS host address")?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            bail!("Enter a valid HTTP or HTTPS host address");
        }
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            bail!("The host cannot include credentials, a query, or a fragment");
        }
        if source
            .password
            .as_ref()
            .is_some_and(|password| !password.is_empty())
            && source
                .username
                .as_ref()
                .is_none_or(|username| username.is_empty())
        {
            bail!("Enter a username to use a password");
        }
        base.set_path(base.path().trim_end_matches('/').to_string().as_str());
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            base,
            client,
            source,
        })
    }

    pub fn host_url(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_string()
    }

    pub fn api_url(&self, endpoint: &str, path: &str) -> Result<Url> {
        let mut url = self.base.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid host address"))?;
        segments.pop_if_empty().push("api").push(endpoint);
        for part in path.split('/').filter(|part| !part.is_empty()) {
            if matches!(part, "." | "..") || part.contains('\\') {
                bail!("Invalid library path");
            }
            segments.push(part);
        }
        drop(segments);
        Ok(url)
    }

    pub fn stream_url(&self, video: &VideoDetails) -> Result<Url> {
        if video.video.is_empty()
            || video.video.contains(['/', '\\'])
            || matches!(video.video.as_str(), "." | "..")
            || video.collection.starts_with('/')
        {
            bail!("Invalid video path");
        }
        self.api_url("stream", &format!("{}/{}", video.collection, video.video))
    }

    pub fn thumbnail_url(&self, path: &str) -> Result<Url> {
        if path.contains("://") {
            let url = Url::parse(path).context("Invalid thumbnail address")?;
            let prefix = self.api_url("thumbnails", "")?;
            if url.origin() != self.base.origin()
                || !url.path().starts_with(&format!("{}/", prefix.path()))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!("Thumbnail must belong to the selected host's library");
            }
            return Ok(url);
        }
        let path = path.strip_prefix("/api/thumbnails/").unwrap_or(path);
        if path.starts_with('/') {
            bail!("Invalid thumbnail path");
        }
        self.api_url("thumbnails", path)
    }

    pub fn get(&self, url: Url) -> RequestBuilder {
        let request = self.client.get(url);
        match self
            .source
            .username
            .as_ref()
            .filter(|username| !username.is_empty())
        {
            Some(username) => request.basic_auth(username, self.source.password.as_deref()),
            None => request,
        }
    }

    pub async fn browse(&self, collection: &str) -> Result<Value> {
        let url = self.api_url("media", collection)?;
        let response = self
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("Could not connect to the download host")?;
        Self::check_status(response.status())?;
        let data: Value = response
            .json()
            .await
            .context("The host did not return a media library")?;
        if !data.get("Collection").is_some_and(Value::is_object) {
            bail!("The host did not return a media library");
        }
        Ok(data)
    }

    pub async fn thumbnail(&self, path: &str) -> Result<Value> {
        let mut response = self
            .get(self.thumbnail_url(path)?)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        Self::check_status(response.status())?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_string();
        if !matches!(
            content_type.as_str(),
            "image/jpeg" | "image/png" | "image/webp" | "image/gif"
        ) {
            bail!("The host did not return a supported thumbnail");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > 5 * 1024 * 1024 {
                bail!("Thumbnail is too large");
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(json!({ "dataUrl": format!("data:{content_type};base64,{}", STANDARD.encode(bytes)) }))
    }

    pub fn check_status(status: reqwest::StatusCode) -> Result<()> {
        if matches!(status.as_u16(), 401 | 403) {
            bail!(
                "Authentication required or credentials rejected (HTTP {})",
                status.as_u16()
            );
        }
        if !status.is_success() {
            bail!("The download host returned HTTP {}", status.as_u16());
        }
        Ok(())
    }
}
