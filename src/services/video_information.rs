use crate::domain::messages::VideoMetadata;
use rand::Rng;
use serde_json::Value;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

pub async fn get_video_metadata<P: AsRef<Path>>(path: P) -> Result<VideoMetadata, Box<dyn Error>> {
    //let spawner = Arc::new(TokioProcessSpawner::new());

    // Build the ffprobe command
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-print_format")
        .arg("json")
        .arg("-show_format")
        .arg("-show_streams")
        .arg(path.as_ref().as_os_str())
        .stdout(Stdio::piped())
        .spawn()?
        .wait_with_output()
        .await?;

    /*let args = vec![
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
        path.as_ref().as_os_str(),
    ];

    let task = spawner.execute(name, cmd, args).await;*/

    if !output.status.success() {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ffprobe exited with an error",
        )));
    }

    // Parse ffprobe output
    let output_str = String::from_utf8(output.stdout)?;
    let json: Value = serde_json::from_str(&output_str)?;

    let mut video_stream = None;
    let mut audio_track_count = 0;

    if let Some(streams) = json.get("streams") {
        for stream in streams.as_array().unwrap() {
            match stream.get("codec_type").and_then(Value::as_str) {
                Some("video") if video_stream.is_none() => {
                    video_stream = Some(stream);
                }
                Some("audio") => {
                    audio_track_count += 1;
                }
                _ => (),
            }
        }
    }

    let video_stream = video_stream.ok_or("No video stream found")?;

    let duration = json
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(Value::as_str)
        .ok_or("No duration found")?
        .parse::<f64>()?;

    let width = video_stream
        .get("width")
        .and_then(Value::as_u64)
        .ok_or("No width found")? as u32;

    let height = video_stream
        .get("height")
        .and_then(Value::as_u64)
        .ok_or("No height found")? as u32;

    Ok(VideoMetadata {
        duration,
        width,
        height,
        audio_tracks: audio_track_count,
    })
}

pub fn get_thumbnail_path<P: AsRef<Path>>(thumbnail_dir: PathBuf, video: P) -> PathBuf {
    let input_filename = video
        .as_ref()
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let output_filename = format!("{}_thumbnail.jpg", input_filename);
    thumbnail_dir.join(output_filename)
}

pub async fn extract_random_frame<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    metadata: VideoMetadata,
) -> io::Result<()> {
    // Get video duration
    let duration = metadata.duration;
    let random_time;

    // Generate a random timestamp
    {
        let mut rng = rand::thread_rng();
        random_time = rng.gen_range(0.0..duration);
    }

    // Run ffmpeg command
    let status = Command::new("ffmpeg")
        .arg("-ss")
        .arg(format!("{}", random_time))
        .arg("-i")
        .arg(input_path.as_ref().as_os_str())
        .arg("-vf")
        .arg("scale=-1:480")
        .arg("-vframes")
        .arg("1")
        .arg("-q:v")
        .arg("2")
        .arg("-y")
        .arg(output_path.as_ref().as_os_str())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "ffmpeg exited with an error",
        ));
    }

    Ok(())
}

pub async fn store_video_info(thumbnail_dir: PathBuf, video: &Path) -> io::Result<()> {
    let output_path = get_thumbnail_path(thumbnail_dir, video);
    let video_meta = match get_video_metadata(video).await {
        Ok(video_info) => video_info,
        Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e.to_string())),
    };

    extract_random_frame(video, output_path.as_path(), video_meta).await
}
