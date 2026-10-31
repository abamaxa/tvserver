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

const VIDEO_CODECS: &[&str] = &["h264"];
const AUDIO_CODECS: &[&str] = &["aac"];

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

    // Wait for the task to complete (2 hour timeout)
    tokio::time::timeout(
        tokio::time::Duration::from_secs(7200),
        task.wait_finished(),
    ).await.ok();

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

/// Bitmap-based subtitle codecs that cannot be converted to text formats like WebVTT
const BITMAP_SUBTITLE_CODECS: &[&str] = &[
    "hdmv_pgs_subtitle",
    "dvd_subtitle",
    "dvb_subtitle",
    "xsub",
];

fn subtitle_is_current(path: &Path, source_modified: Option<std::time::SystemTime>) -> bool {
    use std::io::Read;

    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file()
        || !source_modified.zip(metadata.modified().ok())
            .is_some_and(|(source, subtitle)| subtitle >= source)
    {
        return false;
    }

    let mut header = [0; 6];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok() && &header == b"WEBVTT"
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
    let source_modified = fs::metadata(&current_path).and_then(|metadata| metadata.modified()).ok();

    let mut extracted_files = Vec::new();
    let mut attempted_tracks = 0;
    // Remember which outputs were reusable before this pass, including languages
    // shared by multiple tracks that currently use the same sidecar filename.
    let mut cached_outputs = std::collections::HashMap::new();

    for (subtitle_stream_index, track) in subtitle_tracks.iter().enumerate() {
        // Skip bitmap-based subtitle codecs that can't be converted to text
        if let Some(codec) = &track.codec {
            if BITMAP_SUBTITLE_CODECS.contains(&codec.as_str()) {
                tracing::info!(
                    "Skipping bitmap subtitle track {} (codec: {}, language: {})",
                    track.id,
                    codec,
                    track.language
                );
                continue;
            }
        }

        let lang_code = if track.language != "und" && !track.language.is_empty() {
            track.language.to_uppercase()
        } else {
            format!("track{}", subtitle_stream_index)
        };

        let output_path = parent_dir.join(format!("{}-{}.vtt", file_stem.to_string_lossy(), lang_code));
        if *cached_outputs.entry(output_path.clone())
            .or_insert_with(|| subtitle_is_current(&output_path, source_modified)) {
            extracted_files.push(output_path);
            continue;
        }
        // A failed or interrupted process must not leave a partial file that a
        // later metadata pass could mistake for a completed extraction.
        let temporary_path = output_path.with_extension("tmp.vtt");

        let codec_info = track.codec.as_deref().unwrap_or("unknown");
        tracing::info!(
            "Extracting subtitle track {} (codec: {}, language: {}) from {} to {}",
            track.id,
            codec_info,
            track.language,
            current_path.display(),
            output_path.display()
        );

        let current_path_str = current_path.to_string_lossy();
        let output_path_str = temporary_path.to_string_lossy();

        let task_cmd = format!("0:s:{}", subtitle_stream_index);
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

        attempted_tracks += 1;
        let task_name = format!(
            "Extract Subtitles {} ({}; track {})",
            video.video, lang_code, subtitle_stream_index + 1
        );
        let task = spawner.execute(&task_name,"ffmpeg", args).await;
        // Wait for the task to complete (2 hour timeout)
        if tokio::time::timeout(
            tokio::time::Duration::from_secs(7200),
            task.wait_finished(),
        ).await.is_err() {
            task.terminate();
            let _ = fs::remove_file(&temporary_path);
            tracing::warn!("Subtitle extraction timed out for track {} in {}", track.id, video.video);
            continue;
        }

        // Get the task state
        let result = task.get_state().await;

        if result.error_string.is_empty() {
            match fs::rename(&temporary_path, &output_path) {
                Ok(()) => {
                    tracing::info!("Successfully extracted subtitle track {} to {}", track.id, output_path.display());
                    extracted_files.push(output_path);
                }
                Err(error) => {
                    let _ = fs::remove_file(&temporary_path);
                    tracing::warn!("Could not save subtitle track {} for {}: {}", track.id, video.video, error);
                }
            }
        } else {
            let _ = fs::remove_file(&temporary_path);
            tracing::warn!(
                "Failed to extract subtitle track {} (codec: {}, language: {}): {} | ffmpeg output: {}",
                track.id,
                codec_info,
                track.language,
                result.error_string,
                result.process_details
            );
            // Continue with other tracks even if one fails
        }
    }

    if extracted_files.is_empty() && attempted_tracks > 0 {
        tracing::info!("Failed to extract any subtitle tracks from {}", video.video);
    }

    Ok(extracted_files)
}


#[cfg(test)]
mod subtitle_tests {
    use super::*;
    use crate::domain::models::VideoMetadata;
    use crate::domain::traits::{MockTaskMonitor, Task};
    use crate::services::TaskManager;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSubtitleSpawner {
        calls: Mutex<Vec<Vec<String>>>,
    }

    #[async_trait]
    impl ProcessSpawner for RecordingSubtitleSpawner {
        async fn execute(&self, name: &str, cmd: &str, args: Vec<&str>) -> Task {
            self.calls.lock().unwrap().push(args.iter().map(|arg| arg.to_string()).collect());
            fs::write(args.last().unwrap(), "WEBVTT\n\n").unwrap();
            let key = format!("{} {:?}", cmd, args);
            let name = name.to_owned();
            let mut task = MockTaskMonitor::new();
            task.expect_wait_finished().returning(|| ());
            task.expect_get_key().return_const(key.clone());
            task.expect_get_state().returning(move || crate::domain::messages::TaskState {
                key: key.clone(),
                name: name.clone(),
                display_name: name.clone(),
                finished: true,
                ..Default::default()
            });
            Arc::new(task)
        }
    }

    #[tokio::test]
    async fn subtitle_tracks_have_distinct_visible_task_names() {
        let video = subtitle_video();
        let spawner = Arc::new(RecordingSubtitleSpawner::default());
        let manager = Arc::new(TaskManager::new(spawner.clone()));

        let files = extract_subtitles(&video, manager.clone()).await.unwrap();

        assert_eq!(files.len(), 4);
        let tasks = manager.get_current_state().await;
        assert_eq!(tasks.len(), 4);
        let names: std::collections::HashSet<_> = tasks.iter().map(|task| &task.display_name).collect();
        assert_eq!(names.len(), 4, "subtitle tracks should not produce indistinguishable task rows");
        fs::remove_dir_all(video.dir_path.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn completed_subtitles_are_not_extracted_again() {
        let video = subtitle_video();
        let spawner = Arc::new(RecordingSubtitleSpawner::default());

        let first = extract_subtitles(&video, spawner.clone()).await.unwrap();
        assert_eq!(first.len(), 4);
        for _ in 0..7 {
            assert_eq!(extract_subtitles(&video, spawner.clone()).await.unwrap(), first);
        }
        assert_eq!(spawner.calls.lock().unwrap().len(), 4, "later metadata passes should reuse completed subtitles");
        fs::remove_dir_all(video.dir_path.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn invalid_empty_and_stale_subtitles_are_regenerated() {
        let video = subtitle_video();
        let spawner = Arc::new(RecordingSubtitleSpawner::default());
        let files = extract_subtitles(&video, spawner.clone()).await.unwrap();
        fs::write(&files[0], "not a VTT file").unwrap();
        fs::write(&files[1], "").unwrap();
        fs::File::open(&files[2]).unwrap().set_modified(std::time::UNIX_EPOCH).unwrap();

        assert_eq!(extract_subtitles(&video, spawner.clone()).await.unwrap(), files);
        assert_eq!(spawner.calls.lock().unwrap().len(), 7);
        for file in files {
            assert!(fs::read_to_string(&file).unwrap().starts_with("WEBVTT"));
            assert!(!file.with_extension("tmp.vtt").exists());
        }
        fs::remove_dir_all(video.dir_path.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn updated_video_requires_fresh_subtitles() {
        let video = subtitle_video();
        let spawner = Arc::new(RecordingSubtitleSpawner::default());
        let files = extract_subtitles(&video, spawner.clone()).await.unwrap();
        fs::File::open(video.get_full_path()).unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10)).unwrap();

        assert_eq!(extract_subtitles(&video, spawner.clone()).await.unwrap(), files);
        assert_eq!(spawner.calls.lock().unwrap().len(), 8);
        fs::remove_dir_all(video.dir_path.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn same_language_tracks_remain_distinguishable_and_reuse_existing_outputs() {
        let mut video = subtitle_video();
        video.metadata.subtitle_tracks.as_mut().unwrap()[1].language = "eng".into();
        let spawner = Arc::new(RecordingSubtitleSpawner::default());
        let manager = Arc::new(TaskManager::new(spawner.clone()));
        let files = extract_subtitles(&video, manager.clone()).await.unwrap();
        let tasks = manager.get_current_state().await;
        let names: std::collections::HashSet<_> = tasks.iter().map(|task| &task.display_name).collect();

        assert_eq!(names.len(), 4);
        assert_eq!(extract_subtitles(&video, manager).await.unwrap(), files);
        assert_eq!(spawner.calls.lock().unwrap().len(), 4);
        fs::remove_dir_all(video.dir_path.unwrap()).unwrap();
    }

    #[tokio::test]
    async fn bitmap_tracks_do_not_shift_text_stream_mappings() {
        let mut video = subtitle_video();
        video.metadata.subtitle_tracks.as_mut().unwrap()[0].codec = Some("dvb_subtitle".into());
        let spawner = Arc::new(RecordingSubtitleSpawner::default());

        assert_eq!(extract_subtitles(&video, spawner.clone()).await.unwrap().len(), 3);
        let mappings: Vec<String> = spawner.calls.lock().unwrap().iter()
            .map(|args| args[args.iter().position(|arg| arg == "-map").unwrap() + 1].clone())
            .collect();
        assert_eq!(mappings, ["0:s:1", "0:s:2", "0:s:3"]);
        fs::remove_dir_all(video.dir_path.unwrap()).unwrap();
    }

    fn subtitle_video() -> VideoDetails {
        let dir = std::env::temp_dir().join(format!("subtitle-task-test-{}", rand::random::<u64>()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("episode.mkv"), "video fixture").unwrap();
        VideoDetails {
            video: "episode.mkv".into(),
            dir_path: Some(dir),
            metadata: VideoMetadata {
                subtitle_tracks: Some(serde_json::from_value(serde_json::json!([
                    { "id": 2, "language": "eng", "codec": "subrip" },
                    { "id": 3, "language": "spa", "codec": "subrip" },
                    { "id": 4, "language": "fra", "codec": "subrip" },
                    { "id": 5, "language": "deu", "codec": "subrip" }
                ])).unwrap()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    struct FailedSubtitleSpawner;

    #[async_trait]
    impl ProcessSpawner for FailedSubtitleSpawner {
        async fn execute(&self, _: &str, _: &str, args: Vec<&str>) -> Task {
            fs::write(args.last().unwrap(), "WEBVTT\n\npartial output").unwrap();
            let mut task = MockTaskMonitor::new();
            task.expect_wait_finished().returning(|| ());
            task.expect_get_state().returning(|| crate::domain::messages::TaskState {
                error_string: "ffmpeg could not extract subtitles".into(),
                ..Default::default()
            });
            Arc::new(task)
        }
    }

    #[tokio::test]
    async fn skipped_bitmap_and_failed_text_subtitles_are_nonfatal() {
        for codec in ["dvb_subtitle", "subrip"] {
            let mut video = subtitle_video();
            let tracks = video.metadata.subtitle_tracks.as_mut().unwrap();
            tracks.truncate(1);
            tracks[0].codec = Some(codec.into());
            assert!(extract_subtitles(&video, Arc::new(FailedSubtitleSpawner)).await.unwrap().is_empty());
            let dir = video.dir_path.unwrap();
            assert!(!dir.join("episode-ENG.vtt").exists());
            assert!(!dir.join("episode-ENG.tmp.vtt").exists());
            fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    use crate::domain::models::VideoMetadata;

    fn video(file_name: &str, video_codec: &str, audio_codec: &str) -> VideoDetails {
        VideoDetails {
            video: file_name.into(),
            dir_path: Some(PathBuf::from("/tmp")),
            metadata: VideoMetadata {
                probe_data: Some(serde_json::json!({"streams": [
                    {"codec_type": "video", "codec_name": video_codec},
                    {"codec_type": "audio", "codec_name": audio_codec}
                ]}).to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn converts_mpeg4_and_mp3_in_an_avi_to_browser_codecs() {
        let args = get_re_encoding_args(&video("episode.avi", "mpeg4", "mp3"), VIDEO_CODECS, AUDIO_CODECS).unwrap();
        assert_eq!(args.video_args, ["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
        assert_eq!(args.audio_args, ["-c:a", "aac", "-b:a", "128k"]);
    }

    #[test]
    fn converts_hevc_in_mp4_but_skips_h264_aac_mp4() {
        let args = get_re_encoding_args(&video("episode.mp4", "hevc", "aac"), VIDEO_CODECS, AUDIO_CODECS).unwrap();
        assert_eq!(args.video_args[1], "libx264");
        assert!(!should_re_encode(&video("episode.mp4", "h264", "aac")));
    }
}
