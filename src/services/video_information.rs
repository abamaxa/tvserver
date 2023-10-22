use crate::domain::algorithm::get_collection_and_video_from_path;
use crate::domain::config::{get_movie_dir, get_thumbnail_dir};
use crate::domain::messages::{LocalMessage, LocalMessageReceiver, LocalMessageSender, MediaEvent};
use crate::domain::models::{SeriesDetails, VideoDetails, VideoMetadata};
use crate::domain::traits::Repository;
use anyhow::anyhow;
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::Hasher;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio::task::JoinHandle;
use tokio::{fs, io::AsyncWriteExt, process::Command};

pub struct MetaDataManager {
    repo: Repository,
    receiver: LocalMessageReceiver,
    sender: LocalMessageSender,
}

impl MetaDataManager {
    fn new(repo: Repository, receiver: LocalMessageReceiver, sender: LocalMessageSender) -> Self {
        Self {
            repo,
            receiver,
            sender,
        }
    }

    pub fn consume(
        repo: Repository,
        receiver: LocalMessageReceiver,
        sender: LocalMessageSender,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut manager = Self::new(repo, receiver, sender);
            manager.event_loop().await;
            eprintln!("local event loop exiting");
        })
    }

    async fn event_loop(&mut self) {
        while let Ok(msg) = self.receiver.recv().await {
            match msg {
                LocalMessage::Media(event) => self.handle_media_event(event).await,
                _ => continue,
            }
        }
    }

    async fn handle_media_event(&self, event: MediaEvent) {
        let _ = match event {
            MediaEvent::MediaAvailable(event) => {
                store_video_info(event.full_path, self.repo.clone()).await
            }
            _ => return,
        };
    }
}

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
        return Err(Box::new(io::Error::new(
            io::ErrorKind::Other,
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

pub fn get_thumbnail_path<P: AsRef<Path>>(thumbnail_dir: &PathBuf, video: P) -> PathBuf {
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
    let mut duration = metadata.duration;
    let random_time;

    if duration < 0.1 {
        return Ok(());
    }

    // if its longer than 10 minutes skip the last 10 minutes
    if duration > 600.0 {
        duration -= 180.0;
    }

    // Generate a random timestamp for the final 1/4 of the video
    {
        let mut rng = rand::thread_rng();
        random_time = rng.gen_range((3. * duration / 4.0)..duration);
    }

    // "scale=640:480:force_original_aspect_ratio=decrease,pad=640:480:(ow-iw)/2:(oh-ih)/2"
    // "scale=-1:480"
    // Run ffmpeg command
    let output = Command::new("ffmpeg")
        .arg("-ss")
        .arg(format!("{}", random_time))
        .arg("-i")
        .arg(input_path.as_ref().as_os_str())
        .arg("-vf")
        .arg("scale=640:480:force_original_aspect_ratio=decrease,pad=640:480:(ow-iw)/2:(oh-ih)/2")
        .arg("-vframes")
        .arg("1")
        .arg("-q:v")
        .arg("2")
        .arg("-y")
        .arg(output_path.as_ref().as_os_str())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await?;

    if !output.status.success() {
        let stdout = String::from_utf8(output.stdout).unwrap_or_default();
        let stderr = String::from_utf8(output.stderr).unwrap_or_default();
        eprintln!("{}", stdout);
        eprintln!("{}", stderr);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "ffmpeg exited with an error",
        ));
    }

    Ok(())
}

async fn calculate_checksum<P: AsRef<Path>>(path: P) -> io::Result<i64> {
    let file = File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut hasher = DefaultHasher::new();
    let mut buffer = vec![0; 1024 * 1024]; // Read in chunks of 4MB
    let mut total_read = 0;

    while total_read <= 10 * 1024 * 1024 {
        // Check if we've read more than 10MB
        match reader.read(&mut buffer).await {
            Ok(0) => break, // No more data to read
            Ok(n) => {
                hasher.write(&buffer[..n]);
                total_read += n;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(hasher.finish() as i64)
}

pub async fn store_video_info(path: PathBuf, repo: Repository) -> anyhow::Result<()> {
    eprintln!("processing: {}", path.to_str().unwrap());
    let thumbnail_dir: PathBuf = get_thumbnail_dir(&get_movie_dir());
    let data_file = path.with_extension("json");
    if data_file.exists() {
        return Ok(());
    }

    if !thumbnail_dir.exists() {
        fs::create_dir_all(&thumbnail_dir).await?;
    }

    if is_subdirectory(&path, &thumbnail_dir) {
        return Ok(());
    }

    let checksum = calculate_checksum(&path).await?;
    let output_path = get_thumbnail_path(&thumbnail_dir, &path);
    let metadata = match get_video_metadata(&path).await {
        Ok(video_info) => video_info,
        Err(e) => {
            tracing::error!("get_video_metadata failed for {:?} - {}", &data_file, e);
            /*VideoMetadata {
                ..Default::default()
            }*/
            return Err(anyhow!(e.to_string()));
        }
    };

    extract_random_frame(&path, &output_path, metadata.clone()).await?;

    let thumbnail = match output_path.strip_prefix(&thumbnail_dir) {
        Ok(thumbnail) => PathBuf::from(thumbnail),
        _ => PathBuf::new(),
    };

    let (collection, video) = get_collection_and_video_from_path(&path);

    let details = VideoDetails {
        video,
        collection,
        description: "".to_string(),
        series: SeriesDetails::from(path.as_path()),
        thumbnail,
        metadata,
        checksum,
        search_phrase: None,
    };

    write_struct_to_json_file(&details, &data_file).await?;

    match repo.save_video(&details).await {
        Ok(count) => {
            if count != 1 {
                tracing::info!("save details did not change any records: {:?}", details);
            }
            Ok(())
        }
        Err(err) => Err(anyhow!(err.to_string())),
    }
}

pub async fn write_struct_to_json_file<T: Serialize>(data: &T, file_path: &Path) -> io::Result<()> {
    // Serialize the struct to a JSON string with indentation.
    let json_string = serde_json::to_string_pretty(data)?;

    // Open the file in write mode or create it if it doesn't exist.
    let mut file = File::create(file_path).await?;

    // Write the JSON string to the file.
    file.write_all(json_string.as_bytes()).await?;

    // Flush and close the file.
    file.flush().await?;

    Ok(())
}

fn is_subdirectory(path: &Path, base: &Path) -> bool {
    // Canonicalize the paths to remove symbolic links and other artifacts
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_owned());

    // Check if the canonical path starts with the canonical base path
    canonical_path.starts_with(&canonical_base)
}
