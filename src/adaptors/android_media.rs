//! Android's media APIs replace desktop ffprobe/ffmpeg during library ingestion.
//! JNI work runs on a blocking worker, never on the WebView thread.
use anyhow::{bail, Context, Result};
use jni::objects::{JByteArray, JObject, JString, JValue};
use jni::JNIEnv;
use super::android_platform::with_env;
use serde_json::{json, Value};
use std::path::Path;

// Clear a pending Java exception before releasing the native media resource.
fn with_media<T>(
    env: &mut JNIEnv,
    class: &str,
    action: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T>,
) -> Result<T> {
    let media = env.new_object(class, "()V", &[])?;
    let result = action(env, &media);
    if env.exception_check()? {
        env.exception_clear()?;
    }
    let release = env.call_method(&media, "release", "()V", &[]);
    if env.exception_check()? {
        env.exception_clear()?;
    }
    match result {
        Err(error) => Err(error),
        Ok(value) => {
            release?;
            Ok(value)
        }
    }
}

fn set_source(env: &mut JNIEnv, media: &JObject, path: &Path) -> Result<()> {
    let path = env.new_string(path.to_str().context("Media path is not UTF-8")?)?;
    env.call_method(
        media,
        "setDataSource",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&path)],
    )?;
    Ok(())
}

fn has_key(env: &mut JNIEnv, format: &JObject, key: &JString) -> Result<bool> {
    Ok(env
        .call_method(
            format,
            "containsKey",
            "(Ljava/lang/String;)Z",
            &[JValue::Object(key)],
        )?
        .z()?)
}

fn string_value(env: &mut JNIEnv, format: &JObject, key: &str) -> Result<Option<String>> {
    let key = env.new_string(key)?;
    if !has_key(env, format, &key)? {
        return Ok(None);
    }
    let value = env
        .call_method(
            format,
            "getString",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&key)],
        )?
        .l()?;
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(env.get_string(&JString::from(value))?.into()))
}

fn integer_value(env: &mut JNIEnv, format: &JObject, key: &str) -> Result<i32> {
    let key = env.new_string(key)?;
    if !has_key(env, format, &key)? {
        return Ok(0);
    }
    Ok(env
        .call_method(
            format,
            "getInteger",
            "(Ljava/lang/String;)I",
            &[JValue::Object(&key)],
        )?
        .i()?)
}

/// Return the same stream schema consumed by VideoMetadata's persisted probe data.
pub(crate) fn probe(path: &Path) -> Result<String> {
    with_env(|env| {
        with_media(env, "android/media/MediaExtractor", |env, extractor| {
            set_source(env, extractor, path)?;
            let count = env
                .call_method(extractor, "getTrackCount", "()I", &[])?
                .i()?;
            let mut duration_us = 0_i64;
            let mut streams = Vec::new();
            for index in 0..count {
                let (stream, duration) = env.with_local_frame(32, |env| -> Result<(Value, i64)> {
                let format = env.call_method(extractor, "getTrackFormat", "(I)Landroid/media/MediaFormat;", &[JValue::Int(index)])?.l()?;
                let mime = string_value(env, &format, "mime")?.unwrap_or_default();
                let kind = if mime.starts_with("video/") { "video" }
                    else if mime.starts_with("audio/") { "audio" }
                    else if mime.starts_with("text/") || mime.contains("subtitle") || mime.contains("subrip") { "subtitle" }
                    else { "data" };
                let mut width = integer_value(env, &format, "width")?;
                let mut height = integer_value(env, &format, "height")?;
                if matches!(integer_value(env, &format, "rotation-degrees")?, 90 | 270) {
                    std::mem::swap(&mut width, &mut height);
                }
                let duration_key = env.new_string("durationUs")?;
                let duration = if has_key(env, &format, &duration_key)? {
                    env.call_method(&format, "getLong", "(Ljava/lang/String;)J", &[JValue::Object(&duration_key)])?.j()?
                } else { 0 };
                Ok((json!({
                    "index": index,
                    "codec_type": kind,
                    "codec_name": mime.split('/').nth(1),
                    "width": width.max(0), "height": height.max(0),
                    "tags": { "language": string_value(env, &format, "language")?.unwrap_or_else(|| "und".into()) }
                }), duration))
            })?;
                duration_us = duration_us.max(duration);
                streams.push(stream);
            }
            if !streams
                .iter()
                .any(|stream| matches!(stream["codec_type"].as_str(), Some("video" | "audio")))
            {
                bail!("Android found no audio or video streams");
            }
            Ok(json!({ "streams": streams, "format": { "duration": (duration_us as f64 / 1_000_000.0).to_string() } }).to_string())
        })
    })
}

pub(crate) fn frame_jpeg(path: &Path, timestamp_us: i64) -> Result<Option<Vec<u8>>> {
    with_env(|env| {
        with_media(env, "android/media/MediaMetadataRetriever", |env, retriever| {
            set_source(env, retriever, path)?;
            let bitmap = env
                .call_method(
                    retriever,
                    "getFrameAtTime",
                    "(JI)Landroid/graphics/Bitmap;",
                    &[JValue::Long(timestamp_us), JValue::Int(2)],
                )?
                .l()?;
            if bitmap.is_null() {
                return Ok(None);
            }
            let output = env.new_object("java/io/ByteArrayOutputStream", "()V", &[])?;
            let jpeg = env
                .get_static_field(
                    "android/graphics/Bitmap$CompressFormat",
                    "JPEG",
                    "Landroid/graphics/Bitmap$CompressFormat;",
                )?
                .l()?;
            let compressed = env
                .call_method(
                    &bitmap,
                    "compress",
                    "(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z",
                    &[JValue::Object(&jpeg), JValue::Int(90), JValue::Object(&output)],
                )?
                .z()?;
            env.call_method(&bitmap, "recycle", "()V", &[])?;
            if !compressed {
                bail!("Android could not encode the thumbnail");
            }
            let bytes = env.call_method(&output, "toByteArray", "()[B", &[])?.l()?;
            Ok(Some(env.convert_byte_array(JByteArray::from(bytes))?))
        })
    })
}
