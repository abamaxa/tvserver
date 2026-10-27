#[cfg(not(target_os = "android"))]
use std::path::PathBuf;
#[cfg(not(target_os = "android"))]
use std::process::Command;

pub(crate) struct YtdlpCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Conversions use the same installed executable and environment as downloads.
#[cfg(target_os = "android")]
pub(crate) async fn ffmpeg_command() -> anyhow::Result<YtdlpCommand> {
    let runtime = android_runtime().await?;
    let mut args: Vec<String> = runtime
        .environment
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    args.push(runtime.ffmpeg.clone());
    Ok(YtdlpCommand {
        program: "/system/bin/env".into(),
        args,
    })
}

/// One launch contract for native Android and desktop downloads/search.
pub(crate) async fn command() -> anyhow::Result<YtdlpCommand> {
    #[cfg(target_os = "android")]
    {
        let runtime = android_runtime().await?;
        let mut args = runtime.python_args();
        args.extend([
            runtime.script.clone(),
            "--ignore-config".into(),
            "--js-runtimes".into(),
            format!("quickjs:{}", runtime.quickjs),
            "--ffmpeg-location".into(),
            std::path::Path::new(&runtime.ffmpeg)
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Invalid FFmpeg location"))?
                .to_string_lossy()
                .into_owned(),
        ]);
        return Ok(YtdlpCommand {
            program: "/system/bin/env".into(),
            args,
        });
    }
    #[cfg(not(target_os = "android"))]
    {
        let python = python_with_pip()
            .ok_or_else(|| anyhow::anyhow!("No Python interpreter with pip found for yt-dlp"))?;
        Ok(YtdlpCommand {
            program: python.to_string_lossy().into_owned(),
            args: vec![
                "-c".into(),
                RUN_YTDLP.into(),
                package_dir().to_string_lossy().into_owned(),
                "--ignore-config".into(),
                "--js-runtimes".into(),
                "node".into(),
            ],
        })
    }
}

/// Run embedded Python with the managed yt-dlp import path as its first argument.
pub(crate) async fn python_command(script: &str) -> anyhow::Result<YtdlpCommand> {
    #[cfg(target_os = "android")]
    {
        let runtime = android_runtime().await?;
        return Ok(YtdlpCommand {
            program: "/system/bin/env".into(),
            args: runtime.script_args(script),
        });
    }
    #[cfg(not(target_os = "android"))]
    {
        let python = python_with_pip()
            .ok_or_else(|| anyhow::anyhow!("No Python interpreter with pip found for yt-dlp"))?;
        Ok(YtdlpCommand {
            program: python.to_string_lossy().into_owned(),
            args: vec![
                "-c".into(),
                script.into(),
                package_dir().to_string_lossy().into_owned(),
            ],
        })
    }
}

#[cfg(any(target_os = "android", test))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct AndroidRuntime {
    python: String,
    script: String,
    ffmpeg: String,
    ffprobe: String,
    quickjs: String,
    version: String,
    environment: std::collections::BTreeMap<String, String>,
}

#[cfg(any(target_os = "android", test))]
impl AndroidRuntime {
    fn python_args(&self) -> Vec<String> {
        let mut args: Vec<String> = self
            .environment
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        args.push(self.python.clone());
        args
    }

    fn script_args(&self, script: &str) -> Vec<String> {
        let mut args = self.python_args();
        args.extend(["-c".into(), script.into(), self.script.clone()]);
        args
    }
}

#[cfg(target_os = "android")]
struct PreparedAndroidRuntime {
    bundled: AndroidRuntime,
    active: tokio::sync::RwLock<AndroidRuntime>,
}

#[cfg(target_os = "android")]
async fn android_runtime() -> anyhow::Result<AndroidRuntime> {
    Ok(prepared_android_runtime()
        .await?
        .active
        .read()
        .await
        .clone())
}

#[cfg(target_os = "android")]
async fn prepared_android_runtime() -> anyhow::Result<&'static PreparedAndroidRuntime> {
    static RUNTIME: tokio::sync::OnceCell<PreparedAndroidRuntime> =
        tokio::sync::OnceCell::const_new();
    RUNTIME
        .get_or_try_init(|| async {
            let json = tokio::task::spawn_blocking(|| {
                super::android_platform::call("prepareRuntime", None)
            })
            .await??;
            let runtime: AndroidRuntime = serde_json::from_str(&json)?;
            for path in [
                &runtime.python,
                &runtime.script,
                &runtime.ffmpeg,
                &runtime.ffprobe,
                &runtime.quickjs,
            ] {
                anyhow::ensure!(
                    std::path::Path::new(path).is_file(),
                    "Bundled dependency is missing: {path}"
                );
            }
            let mut active = runtime.clone();
            let store = super::android_ytdlp_update::UpdateStore::new(&runtime.script)?;
            match store.current(&runtime.version) {
                Ok(Some(release)) => {
                    let mut updated = runtime.clone();
                    updated.script = store.script(&release).to_string_lossy().into_owned();
                    updated.version = release.version;
                    match verify_android_update(&updated).await {
                        Ok(()) => active = updated,
                        Err(error) => tracing::warn!(
                            "Cached yt-dlp is incompatible; using bundled copy: {error}"
                        ),
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!("Cached yt-dlp is unavailable; using bundled copy: {error}")
                }
            }
            Ok(PreparedAndroidRuntime {
                bundled: runtime,
                active: tokio::sync::RwLock::new(active),
            })
        })
        .await
}

/// Called once on cold startup, away from the UI and without locking downloads.
#[cfg(target_os = "android")]
pub(crate) async fn update_android_on_startup() -> anyhow::Result<()> {
    let prepared = prepared_android_runtime().await?;
    let current_version = prepared.active.read().await.version.clone();
    tracing::info!("Checking for yt-dlp updates (current {current_version})");
    let store = super::android_ytdlp_update::UpdateStore::new(&prepared.bundled.script)?;
    // Network/validation failures leave the cached or bundled runtime available.
    let result: anyhow::Result<()> = async {
        if let Some(release) =
            super::android_ytdlp_update::fetch_update(&store, &current_version).await?
        {
            let mut updated = prepared.bundled.clone();
            updated.script = store.script(&release).to_string_lossy().into_owned();
            updated.version = release.version.clone();
            verify_android_update(&updated).await?;
            store.activate(&release)?;
            *prepared.active.write().await = updated;
            tracing::info!("yt-dlp updated to {}", release.version);
        } else {
            tracing::info!("yt-dlp {current_version} is up to date");
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!("yt-dlp startup update skipped; keeping {current_version}: {error}");
    }
    Ok(())
}

#[cfg(target_os = "android")]
async fn verify_android_update(runtime: &AndroidRuntime) -> anyhow::Result<()> {
    let mut args = runtime.python_args();
    // Import the shipped EJS dependency too: a bare yt-dlp package is insufficient
    // for YouTube's JavaScript challenges. Native Python/QuickJS/FFmpeg stay in APK.
    args.extend(["-c".into(),
        "import sys,ssl;sys.path.insert(0,sys.argv[1]);import yt_dlp,yt_dlp_ejs;assert yt_dlp.version.__version__==sys.argv[2];yt_dlp.YoutubeDL({'quiet':True});print('runtime-ok')".into(),
        runtime.script.clone(), runtime.version.clone()]);
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::process::Command::new("/system/bin/env")
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    anyhow::ensure!(
        output.status.success(),
        "yt-dlp update cannot run with bundled Python: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(target_os = "android")]
pub(crate) async fn runtime_info() -> anyhow::Result<serde_json::Value> {
    let runtime = android_runtime().await?;
    let mut args = runtime.python_args();
    args.extend(["-c".into(), format!(
        "import sys,ssl,json;sys.path.insert(0,{:?});import yt_dlp,yt_dlp_ejs;print(json.dumps(dict(python=sys.version.split()[0],ytDlp=yt_dlp.version.__version__,ejs=yt_dlp_ejs.version,tls=ssl.OPENSSL_VERSION)))", runtime.script
    )]);
    let result = tokio::process::Command::new("/system/bin/env")
        .args(args)
        .kill_on_drop(true)
        .output()
        .await?;
    anyhow::ensure!(
        result.status.success(),
        "Python runtime verification failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut info: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    for (key, program, arguments) in [
        ("ffmpeg", runtime.ffmpeg.as_str(), vec!["-version"]),
        ("ffprobe", runtime.ffprobe.as_str(), vec!["-version"]),
        (
            "quickjs",
            runtime.quickjs.as_str(),
            vec!["-e", "console.log('QuickJS works')"],
        ),
    ] {
        let mut args: Vec<String> = runtime
            .environment
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        args.push(program.into());
        args.extend(arguments.into_iter().map(String::from));
        let result = tokio::process::Command::new("/system/bin/env")
            .args(args)
            .kill_on_drop(true)
            .output()
            .await?;
        anyhow::ensure!(
            result.status.success(),
            "Bundled {key} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        info[key] = String::from_utf8_lossy(&result.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .into();
    }
    Ok(info)
}

#[cfg(not(target_os = "android"))]
pub(crate) fn package_dir() -> PathBuf {
    PathBuf::from(crate::domain::config::get_downloads_dir()).join(".yt-dlp-python")
}

#[cfg(not(target_os = "android"))]
pub(crate) fn python_with_pip() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("TVSERVER_PYTHON") {
        return choose_python_with_pip([PathBuf::from(explicit)]);
    }
    let path = std::env::var_os("PATH")?;
    choose_python_with_pip(std::env::split_paths(&path).map(|dir| dir.join("python3")))
}

#[cfg(not(target_os = "android"))]
fn choose_python_with_pip(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| {
        candidate.is_file()
            && Command::new(candidate)
                .args(["-m", "pip", "--version"])
                .output()
                .is_ok_and(|output| output.status.success())
    })
}

#[cfg(not(target_os = "android"))]
pub(crate) const RUN_YTDLP: &str =
    "import sys; sys.path.insert(0, sys.argv.pop(1)); from yt_dlp import main; main()";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_launch_preserves_environment_values_without_shell_interpretation() {
        let runtime: AndroidRuntime = serde_json::from_value(serde_json::json!({
            "python": "/data/app/package/lib/arm64/libpython.so", "script": "/private/yt-dlp",
            "ffmpeg": "/private/media-bin/ffmpeg", "ffprobe": "/private/media-bin/ffprobe",
            "quickjs": "/data/app/package/lib/arm64/libqjs.so", "version": "test",
            "environment": { "PYTHONHOME": "/private/Python home", "SSL_CERT_FILE": "/private/CA file.pem" }
        })).unwrap();
        assert_eq!(
            runtime.python_args(),
            vec![
                "PYTHONHOME=/private/Python home",
                "SSL_CERT_FILE=/private/CA file.pem",
                "/data/app/package/lib/arm64/libpython.so"
            ]
        );
        assert_eq!(
            runtime.script_args("print('query & value')"),
            [runtime.python_args(), vec![
                "-c".into(), "print('query & value')".into(), "/private/yt-dlp".into()
            ]].concat()
        );
    }

    #[cfg(unix)]
    #[test]
    fn chooses_a_python_interpreter_that_can_import_pip() {
        let root = std::env::temp_dir().join(format!("tvserver-python-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        let broken = root.join("broken-python3");
        let working = root.join("working-python3");
        std::fs::write(&broken, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::write(&working, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        for path in [&broken, &working] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert_eq!(
            choose_python_with_pip(vec![broken, working.clone()]),
            Some(working)
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
