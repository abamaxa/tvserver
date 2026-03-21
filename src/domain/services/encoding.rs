use std::{
    error::Error,
    fmt,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use tracing;

use crate::domain::{
    models::VideoDetails,
    traits::ProcessSpawner,
};

/// FFProbeOutput structure to parse ffprobe output
#[derive(Debug, Deserialize, Serialize)]
pub struct FFProbeOutput {
    pub streams: Vec<Stream>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Stream {
    pub codec_name: String,
    pub codec_type: String,
}

pub struct CodecArgs {
    pub video_args: Vec<String>,
    pub audio_args: Vec<String>,
    pub current_codecs: Vec<String>,
}

const VIDEO_CODECS: &[&str] = &["h264", "h265", "mpeg4", "x264", "x265"];
const AUDIO_CODECS: &[&str] = &["aac", "mp3", "ac3"];

#[derive(Debug)]
pub struct AlreadyEncodedError;

impl fmt::Display for AlreadyEncodedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "video is already encoded")
    }
}

impl Error for AlreadyEncodedError {}

/// Converts a video file to MP4 format
pub async fn convert_to_mp4(video: &mut VideoDetails) -> Result<(), Box<dyn Error>> {
    let current_path = video.get_full_path();
    let extension = current_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    if extension == "mp4" {
        return Err(Box::new(AlreadyEncodedError));
    }

    let file_stem = current_path.file_stem().unwrap_or_default();
    let parent_dir = current_path.parent().unwrap_or_else(|| Path::new(""));
    
    let tmp_output_path = parent_dir.join(format!("{}.tmp.mp4", file_stem.to_string_lossy()));
    let output_path = parent_dir.join(format!("{}.mp4", file_stem.to_string_lossy()));

    tracing::info!(
        "re-encoding video input={} output={}", 
        current_path.display(), 
        output_path.display()
    );

    let current_path_str = current_path.to_string_lossy();
    let tmp_output_path_str = tmp_output_path.to_string_lossy();

    let args_list = [
        vec![
            "-i", 
            &current_path_str, 
            "-c:v", 
            "copy", 
            "-c:a", 
            "copy", 
            "-y", 
            &tmp_output_path_str
        ],
        vec![
            "-i", 
            &current_path_str, 
            "-map", 
            "0:v", 
            "-map", 
            "0:a", 
            "-c:v", 
            "copy", 
            "-c:a", 
            "copy", 
            "-y", 
            &tmp_output_path_str
        ],
    ];

    let mut last_error = None;

    for args in &args_list {
        let args_vec: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
        
        let mut cmd = tokio::process::Command::new("ffmpeg");
        cmd.args(&args_vec);

        let output = cmd.output().await;
        
        match output {
            Ok(output) => {
                if output.status.success() {
                    last_error = None;
                    break;
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    
                    println!("ffmpeg stdout:\n{}\n", stdout);
                    println!("ffmpeg stderr:\n{}\n", stderr);
                    
                    last_error = Some(format!("ffmpeg error: {}", stderr));
                }
            }
            Err(e) => {
                last_error = Some(format!("ffmpeg error: {}", e));
            }
        }
    }

    if let Some(err) = last_error {
        tracing::info!("ffmpeg error: {}", err);
        return Err(err.into());
    }

    // Replace the original file with the re-encoded file
    fs::rename(&tmp_output_path, &output_path)?;

    if current_path != output_path {
        fs::remove_file(current_path)?;
    }

    video.video = output_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    Ok(())
}

/// Re-encodes a video file
pub async fn re_encode(
    video: &mut VideoDetails,
    replace_original: bool,
    spawner: Arc<dyn ProcessSpawner>,
) -> Result<(), Box<dyn Error>> {
    extract_subtitles(video, spawner.clone()).await?;
    
    re_encode_video(
        video,
        replace_original,
        VIDEO_CODECS,
        AUDIO_CODECS,
        spawner,
    ).await
}

/// Checks if a video should be re-encoded
pub fn should_re_encode(video: &VideoDetails) -> bool {
    get_re_encoding_args(video, VIDEO_CODECS, AUDIO_CODECS).is_ok()
}

/// Gets the arguments for re-encoding a video
pub fn get_re_encoding_args(
    video: &VideoDetails,
    skip_video_codecs: &[&str],
    skip_audio_codecs: &[&str],
) -> Result<CodecArgs, Box<dyn Error>> {
    let current_path = video.get_full_path();
    let mut args = CodecArgs {
        video_args: vec!["-c:v".to_string(), "copy".to_string()],
        audio_args: vec!["-c:a".to_string(), "copy".to_string()],
        current_codecs: Vec::new(),
    };

    // Parse ffprobe output
    if video.metadata.probe_data.is_none() {
        return Err("No probe data available".into());
    }
    
    let probe_data = video.metadata.probe_data.as_ref().unwrap();
    let probe_output: FFProbeOutput = serde_json::from_str(probe_data)?;

    // Determine if re-encoding is necessary
    for stream in &probe_output.streams {
        if stream.codec_type == "video" && !skip_video_codecs.contains(&stream.codec_name.as_str()) {
            args.video_args = vec![
                "-c:v".to_string(),
                "libx264".to_string(),
                "-pix_fmt".to_string(),
                "yuv420p".to_string(),
            ];
            args.current_codecs.push(format!("video:{}", stream.codec_name));
        }
        if stream.codec_type == "audio" && !skip_audio_codecs.contains(&stream.codec_name.as_str()) {
            args.audio_args = vec![
                "-c:a".to_string(),
                "aac".to_string(),
                "-b:a".to_string(),
                "128k".to_string(),
            ];
            args.current_codecs.push(format!("audio:{}", stream.codec_name));
        }
    }

    let extension = current_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    if extension == "mp4" && args.video_args[1] == "copy" && args.audio_args[1] == "copy" {
        return Err(Box::new(AlreadyEncodedError));
    }

    Ok(args)
}

/// Re-encodes a video file with the specified codecs
pub async fn re_encode_video(
    video: &mut VideoDetails,
    replace_original: bool,
    skip_video_codecs: &[&str],
    skip_audio_codecs: &[&str],
    spawner: Arc<dyn ProcessSpawner>,
) -> Result<(), Box<dyn Error>> {
    let codec_args = get_re_encoding_args(video, skip_video_codecs, skip_audio_codecs)?;

    let current_path = video.get_full_path();
    let file_stem = current_path.file_stem().unwrap_or_default();
    let parent_dir = current_path.parent().unwrap_or_else(|| Path::new(""));

    let tmp_output_path = parent_dir.join(format!("{}.tmp.mp4", file_stem.to_string_lossy()));
    let output_path = parent_dir.join(format!("{}.mp4", file_stem.to_string_lossy()));

    tracing::info!(
        "re-encoding video input={} output={} codecs={:?}",
        current_path.display(),
        output_path.display(),
        codec_args.current_codecs
    );

    let mut args = vec![
        "-i",
        current_path.to_str().unwrap_or_default(),
        "-y",
        "-map",
        "0:v",
        "-map",
        "0:a"
    ];

    // Convert String vec to &str vec and extend args
    args.extend(codec_args.video_args.iter().map(|s| s.as_str()));
    args.extend(codec_args.audio_args.iter().map(|s| s.as_str()));
    args.push(tmp_output_path.to_str().unwrap_or_default());

    let task = spawner
        .execute(// Context is unused in the Go code
            &format!("ReEncode {}", video.video),
            "ffmpeg",
            args,
        )
        .await;

    // Wait for the task to complete
    for _ in 0..7200 {
        if task.has_finished() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    // Get the task state
    let result = task.get_state().await;

    if !result.error_string.is_empty() {
        tracing::info!("ffmpeg error: {}", result.error_string);
        return Err(format!("ffmpeg error: {}", result.error_string).into());
    }

    if replace_original {
        // Replace the original file with the re-encoded file
        fs::rename(&tmp_output_path, &output_path)?;

        if current_path != output_path {
            fs::remove_file(current_path)?;
        }
    } else {
        // Just use the temporary output path
        // No need to rename
        // output_path = tmp_output_path; // In Go, but Rust doesn't allow this without moving
        video.video = tmp_output_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        return Ok(());
    }

    video.video = output_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    // The Go code does this, but it's not implemented in this conversion
    // We would need access to the video_metadata service
    // let metadata = get_video_metadata(&output_path, logger).await?;
    // video.metadata = metadata;

    Ok(())
}

/// Extracts subtitles from a video file to VTT format
pub async fn extract_subtitles(video: &VideoDetails, spawner: Arc<dyn ProcessSpawner>) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let subtitle_tracks = match &video.metadata.subtitle_tracks {
        Some(tracks) if !tracks.is_empty() => tracks,
        _ => {
            return Ok(Vec::new());
        }
    };

    let current_path = video.get_full_path();
    let file_stem = current_path.file_stem().unwrap_or_default();
    let parent_dir = current_path.parent().unwrap_or_else(|| Path::new(""));

    let mut extracted_files = Vec::new();

    for (index, track) in subtitle_tracks.iter().enumerate() {
        let lang_code = if track.language != "und" && !track.language.is_empty() {
            track.language.to_uppercase()
        } else {
            format!("track{}", index)
        };

        let output_path = parent_dir.join(format!("{}-{}.vtt", file_stem.to_string_lossy(), lang_code));

        tracing::info!(
            "Extracting subtitle track {} (language: {}) from {} to {}",
            track.id,
            track.language,
            current_path.display(),
            output_path.display()
        );

        let current_path_str = current_path.to_string_lossy();
        let output_path_str = output_path.to_string_lossy();

        let task_cmd = format!("0:s:{}", index);
        let args = vec![
            "-i",
            &current_path_str,
            "-map",
            &task_cmd,
            "-c:s",
            "webvtt",
            "-y",
            &output_path_str,
        ];

        let task_name = format!("Extract Subtitles {}", video.video);
        let task = spawner.execute(&task_name,"ffmpeg", args).await;
        // Wait for the task to complete
        for _ in 0..7200 {
            if task.has_finished() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        // Get the task state
        let result = task.get_state().await;

        if result.error_string.is_empty() {
            tracing::info!("Successfully extracted subtitle track {} to {}", track.id, output_path.display());
            extracted_files.push(output_path);
        } else {
            tracing::warn!(
                "Failed to extract subtitle track {} (language: {}): {}",
                track.id,
                track.language,
                result.error_string
            );
            // Continue with other tracks even if one fails
        }
    }

    if extracted_files.is_empty() && !subtitle_tracks.is_empty() {
        tracing::info!("Failed to extract any subtitle tracks from {}", video.video);
    }

    Ok(extracted_files)
}
