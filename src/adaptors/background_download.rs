//! Keep the platform's download lifetime aligned with the real engine, including
//! initialization, completion, errors and cancellation. UI polling is not involved.
use crate::domain::messages::{DownloadInfo, DownloadRequest};
use crate::domain::traits::{Download, DownloadProgress, DownloadProgressMonitor, Downloader};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Interruption {
    cancelled: CancellationToken,
    reason: Mutex<String>,
}

impl Interruption {
    fn stop(&self, reason: String) {
        *self.reason.lock().unwrap() = reason;
        self.cancelled.cancel();
    }

    fn info(&self) -> DownloadInfo {
        DownloadInfo {
            total_size: None,
            downloaded_size: 0,
            uploaded_size: None,
            finished: true,
            error_message: self.reason.lock().unwrap().clone(),
            progress_message: String::new(),
            files: vec![],
        }
    }
}

struct DownloadPermit {
    interruption: Arc<Interruption>,
    update: Box<dyn Fn(&DownloadInfo) + Send + Sync>,
    finish: Box<dyn Fn() + Send + Sync>,
}

impl Drop for DownloadPermit {
    fn drop(&mut self) {
        (self.finish)();
    }
}

#[async_trait]
trait DownloadLifetime: Send + Sync {
    async fn start(&self, name: &str) -> anyhow::Result<DownloadPermit>;
}

struct BackgroundDownloader {
    inner: Downloader,
    lifetime: Arc<dyn DownloadLifetime>,
}

#[async_trait]
impl Download for BackgroundDownloader {
    async fn download(&self, request: DownloadRequest) -> anyhow::Result<DownloadProgressMonitor> {
        // Acquire platform protection before starting Python or resolving a magnet.
        let permit = self.lifetime.start(&request.name).await?;
        if permit.interruption.cancelled.is_cancelled() {
            anyhow::bail!("{}", permit.interruption.info().error_message);
        }
        let inner = self.inner.download(request).await?;
        if permit.interruption.cancelled.is_cancelled() {
            inner.terminate();
            anyhow::bail!("{}", permit.interruption.info().error_message);
        }
        Ok(Arc::new(BackgroundProgress {
            inner,
            interruption: permit.interruption.clone(),
            permit: Mutex::new(Some(permit)),
        }))
    }
}

struct BackgroundProgress {
    inner: DownloadProgressMonitor,
    interruption: Arc<Interruption>,
    permit: Mutex<Option<DownloadPermit>>,
}

impl BackgroundProgress {
    fn finish(&self) {
        // Drop outside the lock: platform callbacks may re-enter the JNI bridge.
        let permit = self.permit.lock().unwrap().take();
        drop(permit);
    }
}

#[async_trait]
impl DownloadProgress for BackgroundProgress {
    fn terminate(&self) {
        self.inner.terminate();
        self.interruption.stop("Download cancelled".into());
        self.finish();
    }

    async fn observe(&self) -> DownloadInfo {
        let info = tokio::select! {
            biased;
            _ = self.interruption.cancelled.cancelled() => {
                self.inner.terminate();
                self.interruption.info()
            }
            info = self.inner.observe() => info,
        };
        if info.finished || !info.error_message.is_empty() {
            self.finish();
        } else if let Some(permit) = self.permit.lock().unwrap().as_ref() {
            (permit.update)(&info);
        }
        info
    }
}

#[cfg(target_os = "android")]
pub(crate) fn protect(downloader: Downloader) -> Downloader {
    Arc::new(BackgroundDownloader {
        inner: downloader,
        lifetime: Arc::new(android::AndroidDownloadLifetime),
    })
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use crate::adaptors::android_platform;
    use jni::objects::{JClass, JString};
    use jni::JNIEnv;
    use once_cell::sync::Lazy;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Weak;

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    static ACTIVE: Lazy<Mutex<HashMap<String, Weak<Interruption>>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    pub(super) struct AndroidDownloadLifetime;

    #[async_trait]
    impl DownloadLifetime for AndroidDownloadLifetime {
        async fn start(&self, name: &str) -> anyhow::Result<DownloadPermit> {
            let name = name.to_owned();
            // Keep the permit inside the blocking operation so cancellation of
            // the caller cannot leave a late-starting Android job orphaned.
            tokio::task::spawn_blocking(move || {
                let id = format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                );
                let interruption = Arc::new(Interruption::default());
                ACTIVE
                    .lock()
                    .unwrap()
                    .insert(id.clone(), Arc::downgrade(&interruption));
                let update_id = id.clone();
                let finish_id = id.clone();
                let permit = DownloadPermit {
                    interruption,
                    update: Box::new(move |info| {
                        let payload = serde_json::json!({
                            "id": update_id, "downloaded": info.downloaded_size,
                            "total": info.total_size, "details": info.progress_message,
                        });
                        if let Err(error) =
                            android_platform::call("updateDownload", Some(&payload.to_string()))
                        {
                            tracing::warn!("Could not update download notification: {error:#}");
                        }
                    }),
                    finish: Box::new(move || {
                        ACTIVE.lock().unwrap().remove(&finish_id);
                        let payload = serde_json::json!({ "id": finish_id });
                        if let Err(error) =
                            android_platform::call("finishDownload", Some(&payload.to_string()))
                        {
                            tracing::warn!("Could not finish Android download: {error:#}");
                        }
                    }),
                };
                let payload = serde_json::json!({ "id": id, "name": name });
                let output = android_platform::call("beginDownload", Some(&payload.to_string()))?;
                let result: serde_json::Value = serde_json::from_str(&output)?;
                if let Some(error) = result["error"].as_str() {
                    anyhow::bail!("{error}");
                }
                Ok(permit)
            })
            .await?
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_abamaxa_lots_1of_1videos_NativeBridge_nativeDownloadStopped(
        mut env: JNIEnv,
        _class: JClass,
        id: JString,
        reason: JString,
    ) {
        let Ok(id) = env.get_string(&id).map(String::from) else {
            return;
        };
        let Ok(reason) = env.get_string(&reason).map(String::from) else {
            return;
        };
        let interruption = ACTIVE.lock().unwrap().get(&id).and_then(Weak::upgrade);
        if let Some(interruption) = interruption {
            interruption.stop(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::traits::{MockDownload, MockDownloadProgress};
    use crate::domain::SearchEngineType;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Lifetime {
        active: Arc<AtomicUsize>,
        signals: Mutex<Vec<Arc<Interruption>>>,
    }

    #[async_trait]
    impl DownloadLifetime for Lifetime {
        async fn start(&self, _: &str) -> anyhow::Result<DownloadPermit> {
            let interruption = Arc::new(Interruption::default());
            self.signals.lock().unwrap().push(interruption.clone());
            self.active.fetch_add(1, Ordering::SeqCst);
            let active = self.active.clone();
            Ok(DownloadPermit {
                interruption,
                update: Box::new(|_| {}),
                finish: Box::new(move || {
                    active.fetch_sub(1, Ordering::SeqCst);
                }),
            })
        }
    }

    fn request(engine: SearchEngineType) -> DownloadRequest {
        DownloadRequest {
            name: "test".into(),
            link: "test".into(),
            engine,
            series: None,
        }
    }

    fn info(finished: bool, error: &str) -> DownloadInfo {
        DownloadInfo {
            total_size: None,
            downloaded_size: 42,
            uploaded_size: None,
            finished,
            error_message: error.into(),
            progress_message: String::new(),
            files: vec![],
        }
    }

    #[tokio::test]
    async fn protects_both_engines_before_start_and_releases_each_independently() {
        let lifetime = Arc::new(Lifetime::default());
        let active = lifetime.active.clone();
        let mut engine = MockDownload::new();
        engine.expect_download().times(2).returning(move |_| {
            assert!(
                active.load(Ordering::SeqCst) > 0,
                "engine started without protection"
            );
            let mut progress = MockDownloadProgress::new();
            progress.expect_terminate().return_const(());
            progress.expect_observe().returning(|| info(false, ""));
            Ok(Arc::new(progress))
        });
        let downloader = BackgroundDownloader {
            inner: Arc::new(engine),
            lifetime: lifetime.clone(),
        };
        let youtube = downloader
            .download(request(SearchEngineType::YouTube))
            .await
            .unwrap();
        let torrent = downloader
            .download(request(SearchEngineType::Torrent))
            .await
            .unwrap();
        assert_eq!(lifetime.active.load(Ordering::SeqCst), 2);
        assert!(!youtube.observe().await.finished);
        youtube.terminate();
        youtube.terminate();
        assert_eq!(lifetime.active.load(Ordering::SeqCst), 1);
        assert!(!torrent.observe().await.finished);
        drop(torrent);
        assert_eq!(lifetime.active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn releases_on_completion_and_reported_errors_even_while_task_is_retained() {
        for (finished, error) in [(true, ""), (false, "network failed")] {
            let lifetime = Arc::new(Lifetime::default());
            let mut engine = MockDownload::new();
            engine.expect_download().returning(move |_| {
                let mut progress = MockDownloadProgress::new();
                progress
                    .expect_observe()
                    .returning(move || info(finished, error));
                Ok(Arc::new(progress))
            });
            let downloader = BackgroundDownloader {
                inner: Arc::new(engine),
                lifetime: lifetime.clone(),
            };
            let task = downloader
                .download(request(SearchEngineType::YouTube))
                .await
                .unwrap();
            task.observe().await;
            assert_eq!(lifetime.active.load(Ordering::SeqCst), 0);
            task.observe().await;
            assert_eq!(lifetime.active.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn startup_error_releases_the_platform_job() {
        let lifetime = Arc::new(Lifetime::default());
        let mut engine = MockDownload::new();
        engine
            .expect_download()
            .returning(|_| anyhow::bail!("runtime unavailable"));
        let downloader = BackgroundDownloader {
            inner: Arc::new(engine),
            lifetime: lifetime.clone(),
        };
        assert!(downloader
            .download(request(SearchEngineType::YouTube))
            .await
            .is_err());
        assert_eq!(lifetime.active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn system_stop_terminates_the_engine_and_surfaces_a_terminal_error() {
        let lifetime = Arc::new(Lifetime::default());
        let mut engine = MockDownload::new();
        let terminated = Arc::new(AtomicUsize::new(0));
        let count = terminated.clone();
        engine.expect_download().returning(move |_| {
            let count = count.clone();
            let mut progress = MockDownloadProgress::new();
            progress.expect_terminate().returning(move || {
                count.fetch_add(1, Ordering::SeqCst);
            });
            Ok(Arc::new(progress))
        });
        let downloader = BackgroundDownloader {
            inner: Arc::new(engine),
            lifetime: lifetime.clone(),
        };
        let task = downloader
            .download(request(SearchEngineType::Torrent))
            .await
            .unwrap();
        lifetime.signals.lock().unwrap()[0]
            .stop("Android stopped the transfer; retry the download".into());
        let state = task.observe().await;
        assert!(state.finished);
        assert!(state.error_message.contains("retry"));
        assert_eq!(terminated.load(Ordering::SeqCst), 1);
        assert_eq!(lifetime.active.load(Ordering::SeqCst), 0);
    }
}
