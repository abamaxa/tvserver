use anyhow::{Context, Result};
use jni::objects::{GlobalRef, JClass, JString, JValue};
use jni::{JNIEnv, JavaVM};
use once_cell::sync::{Lazy, OnceCell};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

struct AndroidContext {
    vm: JavaVM,
    activity: GlobalRef,
    bridge_class: GlobalRef,
}

static CONTEXT: OnceCell<std::result::Result<AndroidContext, String>> = OnceCell::new();
static APP: OnceCell<tauri::AppHandle> = OnceCell::new();
static PLAYER_STATE: Lazy<Mutex<Value>> = Lazy::new(|| Mutex::new(serde_json::json!({})));
static LAST_HISTORY_SAVE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn initialize(app: &tauri::App) -> Result<()> {
    let _ = APP.set(app.handle().clone());
    app.get_webview_window("main")
        .context("Android WebView is unavailable")?
        .with_webview(|webview| {
            webview.jni_handle().exec(|env, activity, _| {
                let result = (|| -> Result<AndroidContext> {
                    // Resolve the application class on Android's class-loader thread.
                    // Worker threads can then use the retained class without FindClass failures.
                    let name = env.new_string("com.abamaxa.lots_of_videos.NativeBridge")?;
                    let class = env
                        .call_method(
                            activity,
                            "getAppClass",
                            "(Ljava/lang/String;)Ljava/lang/Class;",
                            &[JValue::Object(&name)],
                        )?
                        .l()?;
                    Ok(AndroidContext {
                        vm: env.get_java_vm()?,
                        activity: env.new_global_ref(activity)?,
                        bridge_class: env.new_global_ref(class)?,
                    })
                })();
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_clear();
                }
                let _ = CONTEXT.set(result.map_err(|error| error.to_string()));
            });
        })?;
    Ok(())
}

pub(crate) fn with_env<T>(action: impl FnOnce(&mut JNIEnv) -> Result<T>) -> Result<T> {
    let context = CONTEXT
        .wait()
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let mut env = context.vm.attach_current_thread()?;
    let result = env.with_local_frame(64, action);
    if env.exception_check()? {
        let exception = env.exception_occurred()?;
        env.exception_clear()?;
        let description = (|| -> Result<String> {
            let message = env.call_method(exception, "toString", "()Ljava/lang/String;", &[])?.l()?;
            Ok(env.get_string(&JString::from(message))?.into())
        })().unwrap_or_else(|_| "Android native operation failed".into());
        if env.exception_check()? { env.exception_clear()?; }
        return Err(match result {
            Err(error) => error.context(description),
            Ok(_) => anyhow::anyhow!(description),
        });
    }
    result
}

pub(crate) fn call(method: &str, payload: Option<&str>) -> Result<String> {
    let context = CONTEXT
        .wait()
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    with_env(|env| {
        let class: &JClass = context.bridge_class.as_obj().into();
        let value = match (method, payload) {
            ("prepareRuntime", _) => env.call_static_method(
                class,
                method,
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(context.activity.as_obj())],
            )?,
            ("openPlayer" | "shareVideos" | "exportVideo" | "beginDownload", Some(payload)) => {
                let payload = env.new_string(payload)?;
                env.call_static_method(
                    class,
                    method,
                    "(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(context.activity.as_obj()), JValue::Object(&payload)],
                )?
            }
            (_, Some(payload)) => {
                let payload = env.new_string(payload)?;
                env.call_static_method(
                    class,
                    method,
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&payload)],
                )?
            }
            _ => anyhow::bail!("Missing native command payload"),
        }
        .l()?;
        let output: String = env.get_string(&JString::from(value))?.into();
        Ok(output)
    })
}

pub(crate) fn player_state() -> Value {
    PLAYER_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

#[no_mangle]
pub extern "system" fn Java_com_abamaxa_lots_1of_1videos_NativeBridge_nativePlayerEvent(
    mut env: JNIEnv,
    _class: JClass,
    payload: JString,
) {
    let Ok(text) = env.get_string(&payload).map(String::from) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    *PLAYER_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = value.clone();
    let Some(app) = APP.get() else {
        return;
    };
    let _ = app.emit("android-player-state", &value);
    let Ok(state) = serde_json::from_value::<crate::domain::messages::RemotePlayerState>(value)
    else {
        return;
    };
    let Ok(checksum) = state.video_id.parse::<i64>() else {
        return;
    };
    if !state.current_time.is_finite() || state.current_time < 0.0 {
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if !state.paused && now.saturating_sub(LAST_HISTORY_SAVE.load(Ordering::Relaxed)) < 5 {
        return;
    }
    LAST_HISTORY_SAVE.store(now, Ordering::Relaxed);
    if let Some(context) = app.try_state::<crate::entrypoints::SharedState>() {
        let repo = context.get_repository();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = repo
                .update_watched_video(checksum, state.current_time)
                .await
            {
                tracing::warn!("Could not save native playback progress: {error}");
            }
        });
    }
}
