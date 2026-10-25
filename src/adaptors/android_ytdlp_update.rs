//! Android's Python payload can update independently of the APK's native binaries.
//! Each process gets an immutable script; current.json changes only after validation.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
#[cfg(target_os = "android")]
use std::time::Duration;

#[cfg(target_os = "android")]
const LATEST_RELEASE: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const MAX_SCRIPT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Release {
    pub version: String,
    sha256: String,
}

impl Release {
    fn validate(&self) -> Result<()> {
        version_parts(&self.version)?;
        ensure!(
            self.sha256.len() == 64 && self.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid yt-dlp SHA-256"
        );
        Ok(())
    }

    fn download_url(&self) -> String {
        format!(
            "https://github.com/yt-dlp/yt-dlp/releases/download/{}/yt-dlp",
            self.version
        )
    }

    fn from_metadata(metadata: &serde_json::Value) -> Result<Self> {
        ensure!(
            metadata["draft"] == false && metadata["prerelease"] == false,
            "Expected a stable yt-dlp release"
        );
        let asset = metadata["assets"]
            .as_array()
            .context("Missing yt-dlp assets")?
            .iter()
            .find(|asset| asset["name"] == "yt-dlp")
            .context("Missing Python yt-dlp asset")?;
        let release = Self {
            version: metadata["tag_name"]
                .as_str()
                .context("Missing yt-dlp version")?
                .into(),
            sha256: asset["digest"]
                .as_str()
                .and_then(|s| s.strip_prefix("sha256:"))
                .context("Missing yt-dlp SHA-256")?
                .to_ascii_lowercase(),
        };
        release.validate()?;
        ensure!(
            asset["browser_download_url"].as_str() == Some(release.download_url().as_str()),
            "Unexpected yt-dlp download URL"
        );
        ensure!(
            asset["size"]
                .as_u64()
                .is_some_and(|size| size > 0 && size <= MAX_SCRIPT_BYTES as u64),
            "Unexpected yt-dlp asset size"
        );
        Ok(release)
    }
}

fn version_parts(version: &str) -> Result<Vec<u32>> {
    let parts = version.split('.').collect::<Vec<_>>();
    ensure!(
        (3..=4).contains(&parts.len())
            && parts[0].len() == 4
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit())),
        "Invalid stable yt-dlp version"
    );
    parts
        .into_iter()
        .map(|part| part.parse().map_err(Into::into))
        .collect()
}

pub(super) struct UpdateStore {
    root: PathBuf,
}

impl UpdateStore {
    pub fn new(bundled_script: &str) -> Result<Self> {
        Ok(Self {
            root: Path::new(bundled_script)
                .parent()
                .context("Invalid bundled yt-dlp path")?
                .join("updates"),
        })
    }

    pub fn script(&self, release: &Release) -> PathBuf {
        self.root
            .join(format!("{}-{}.pyz", release.version, release.sha256))
    }

    pub fn current(&self, bundled_version: &str) -> Result<Option<Release>> {
        let manifest = self.root.join("current.json");
        if !manifest.exists() {
            return Ok(None);
        }
        let release: Release = serde_json::from_slice(&std::fs::read(manifest)?)?;
        release.validate()?;
        if version_parts(&release.version)? <= version_parts(bundled_version)? {
            return Ok(None);
        }
        verify_bytes(&release, &std::fs::read(self.script(&release))?)?;
        Ok(Some(release))
    }

    fn stage(&self, release: &Release, bytes: &[u8]) -> Result<()> {
        release.validate()?;
        verify_bytes(release, bytes)?;
        std::fs::create_dir_all(&self.root)?;
        self.atomic_write(&self.script(release), bytes)
    }

    pub fn activate(&self, release: &Release) -> Result<()> {
        // A killed/failed download must never replace the offline selection.
        verify_bytes(release, &std::fs::read(self.script(release))?)?;
        self.atomic_write(&self.root.join("current.json"), &serde_json::to_vec(release)?)
    }

    fn atomic_write(&self, destination: &Path, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        let temporary = destination.with_extension("new");
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(temporary, destination)?;
        Ok(())
    }
}

fn verify_bytes(release: &Release, bytes: &[u8]) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_SCRIPT_BYTES,
        "Invalid yt-dlp payload size"
    );
    ensure!(
        format!("{:x}", Sha256::digest(bytes)) == release.sha256,
        "yt-dlp update checksum does not match"
    );
    Ok(())
}

#[cfg(target_os = "android")]
async fn response_bytes(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut response = response.error_for_status()?;
    ensure!(
        response.content_length().unwrap_or(0) <= limit as u64,
        "yt-dlp response is too large"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= limit,
            "yt-dlp response is too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(target_os = "android")]
pub(super) async fn fetch_update(
    store: &UpdateStore,
    current_version: &str,
) -> Result<Option<Release>> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .user_agent("LotsOfVideos-Android/yt-dlp-updater")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .build()?;
    let metadata = response_bytes(
        client
            .get(LATEST_RELEASE)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await?,
        2 * 1024 * 1024,
    )
    .await?;
    let release = Release::from_metadata(&serde_json::from_slice(&metadata)?)?;
    if version_parts(&release.version)? <= version_parts(current_version)? {
        return Ok(None);
    }
    let bytes = response_bytes(
        client.get(release.download_url()).send().await?,
        MAX_SCRIPT_BYTES,
    )
    .await?;
    store.stage(&release, &bytes)?;
    Ok(Some(release))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        store: UpdateStore,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("lov-ytdlp-{}", rand::random::<u64>()));
            std::fs::create_dir_all(&root).unwrap();
            let store = UpdateStore::new(root.join("bundled").to_str().unwrap()).unwrap();
            Self { root, store }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn release(version: &str, bytes: &[u8]) -> Release {
        Release {
            version: version.into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    #[test]
    fn failed_or_unvalidated_updates_leave_the_offline_version_selected() {
        let fixture = Fixture::new();
        assert!(fixture.store.current("2026.08.19").unwrap().is_none());
        let first = release("2026.10.21", b"working");
        fixture.store.stage(&first, b"working").unwrap();
        // Staging is insufficient: the caller must first test Python compatibility.
        assert!(fixture.store.current("2026.08.19").unwrap().is_none());
        fixture.store.activate(&first).unwrap();
        let bad = release("2026.11.01", b"expected");
        assert!(fixture.store.stage(&bad, b"truncated").is_err());
        assert!(fixture.store.activate(&bad).is_err());
        assert_eq!(
            fixture
                .store
                .current("2026.08.19")
                .unwrap()
                .unwrap()
                .version,
            first.version
        );
    }

    #[test]
    fn new_process_restores_checked_update_but_newer_apk_takes_precedence() {
        let fixture = Fixture::new();
        let update = release("2026.10.21", b"script");
        fixture.store.stage(&update, b"script").unwrap();
        fixture.store.activate(&update).unwrap();
        let reopened = UpdateStore {
            root: fixture.store.root.clone(),
        };
        assert_eq!(
            reopened.current("2026.08.19").unwrap().unwrap().version,
            "2026.10.21"
        );
        assert!(reopened.current("2026.10.21").unwrap().is_none());
        assert!(reopened.current("2027.01.01").unwrap().is_none());
        std::fs::write(reopened.script(&update), b"corrupted").unwrap();
        assert!(reopened.current("2026.08.19").is_err());
    }

    #[test]
    fn activation_preserves_script_used_by_an_existing_download() {
        let fixture = Fixture::new();
        let first = release("2026.10.21", b"old download");
        fixture.store.stage(&first, b"old download").unwrap();
        fixture.store.activate(&first).unwrap();
        let old_path = fixture.store.script(&first);
        let next = release("2026.11.01", b"new download");
        fixture.store.stage(&next, b"new download").unwrap();
        fixture.store.activate(&next).unwrap();
        assert_eq!(std::fs::read(old_path).unwrap(), b"old download");
        assert_eq!(
            fixture
                .store
                .current("2026.08.19")
                .unwrap()
                .unwrap()
                .version,
            next.version
        );
    }

    #[test]
    fn metadata_accepts_only_checked_stable_python_asset_from_official_repository() {
        let mut metadata = serde_json::json!({"tag_name": "2026.10.21", "draft": false,
            "prerelease": false, "assets": [{"name": "yt-dlp", "size": 1024,
            "digest": format!("sha256:{}", "a".repeat(64)),
            "browser_download_url": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.21/yt-dlp"}]});
        assert!(Release::from_metadata(&metadata).is_ok());
        metadata["prerelease"] = true.into();
        assert!(Release::from_metadata(&metadata).is_err());
        metadata["prerelease"] = false.into();
        metadata["assets"][0]["browser_download_url"] = "http://other.example/yt-dlp".into();
        assert!(Release::from_metadata(&metadata).is_err());
        metadata["tag_name"] = "../../escape".into();
        assert!(Release::from_metadata(&metadata).is_err());
        assert!(version_parts("2026.10.21").unwrap() > version_parts("2026.9.30").unwrap());
    }
}
