use crate::domain::algorithm::get_collection_and_video_from_path;
use crate::domain::config::{get_movie_dir, get_thumbnail_dir};
use crate::domain::models::{VideoDetails, VideoMetadata, VideoState};
use crate::domain::traits::{ProcessSpawner, Repository, Storer};
use crate::domain::services::extract_subtitles;
use rand::Rng;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::Hasher;
use std::sync::Arc;
use std::{io, fmt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio::{fs, process::Command};

#[derive(Debug)]
pub enum MetaDataErrorCode {
    Exception = 10,
    ZeroFileSize = 1,
    NoVideoSize = 2,
    CreateThumbnailDir = 3,
    CalculateChecksum = 4,
    ExtractFrame = 5,
    SaveVideo = 6,
    GetVideoMetaData = 7,
    AddFile = 8,
    ExtractSubtitles = 9,
    FileStillBeingWritten = 11,
}


#[derive(Debug)]
pub struct MetaDataError {
    pub code: MetaDataErrorCode,
    pub path: PathBuf,
    pub message: Option<String>,
    pub video_details: VideoDetails,
}

impl fmt::Display for MetaDataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Metadata Error: {:?} ({:?}) ({:?})", self.code, self.path, self.message)
    }
}

impl Error for MetaDataError {}

impl MetaDataError {
    pub fn new(code: MetaDataErrorCode, path: &PathBuf, details: VideoDetails) -> Self {
        Self{code, path: path.to_path_buf(), message: None, video_details: details}
    }

    pub fn new_no_details(code: MetaDataErrorCode, path: &PathBuf) -> Self {
        Self{code, path: path.to_path_buf(), message: None, video_details: VideoDetails{..VideoDetails::default()}}
    }

    pub fn from_error_no_details(code: MetaDataErrorCode, err: &dyn Error, path: &PathBuf) -> Self {
        Self{code, path: path.to_path_buf(), message: Some(err.to_string()), video_details: VideoDetails{..VideoDetails::default()}}
    }

    pub fn from_error(code: MetaDataErrorCode, err: &dyn Error, path: &PathBuf, details: VideoDetails) -> Self {
        Self{code, path: path.to_path_buf(), message: Some(err.to_string()), video_details: details}
    }
}

fn metadata_debug(path: &Path, stage: &str) {
    if std::env::var_os("TVSERVER_METADATA_DEBUG").is_some() {
        eprintln!("stage: {}: {}", stage, path.display());
    }
}

pub async fn generate_video_metadatas(path: PathBuf, storer: Storer, repo: Repository, suggested_series: Option<String>, spawner: Arc<dyn ProcessSpawner>) -> Result<Option<VideoDetails>, MetaDataError> {
    eprintln!("processing: {}", path.to_str().unwrap());
    let thumbnail_dir: PathBuf = get_thumbnail_dir(&get_movie_dir());
    if !thumbnail_dir.exists() {
        if let Err(err) = fs::create_dir_all(&thumbnail_dir).await {
            tracing::error!("could not create thumbnail dir {}", err.to_string());
            return Err(MetaDataError::new_no_details(MetaDataErrorCode::CreateThumbnailDir, &thumbnail_dir))
        }
    }

    if is_subdirectory(&path, &thumbnail_dir) {
        return Ok(None);
    }

    // Check if file is still being written to
    match is_file_being_written(&path).await {
        Ok(true) => {
            tracing::info!("Skipping file that is still being written: {}", path.display());
            return Ok(None);
        }
        Ok(false) => {
            // File is stable, continue processing
        }
        Err(e) => {
            tracing::warn!("Could not check if file is being written: {}", e);
            // Continue processing anyway
        }
    }

    metadata_debug(&path, "metadata:start");
    let mut details = match make_video_metadatas(&path, suggested_series.clone()).await {
        Ok(details) => details,
        Err(err) => return Err(err)
    };
    metadata_debug(&path, "metadata:done");

    if details.checksum == 0 {
        return Err(MetaDataError::new(MetaDataErrorCode::ZeroFileSize, &path, details));
    }

    metadata_debug(&path, "add-file:start");
    let new_path = match storer.add_file(&details.get_full_path(), suggested_series.clone()).await {
        Ok(path) => path,
        Err(err) => return Err(MetaDataError::from_error(MetaDataErrorCode::AddFile, err.as_ref(), &path, details))
    };
    metadata_debug(&new_path, "add-file:done");

    let (collection, video) = get_collection_and_video_from_path(&new_path);

    details.collection = collection;
    details.video = video;
    details.dir_path = None;
    details.search_phrase = suggested_series;

    metadata_debug(&new_path, "save-video:start");
    if let Err(err) = repo.save_video(&details).await {
        return Err(MetaDataError::from_error(MetaDataErrorCode::SaveVideo, &err, &path, details));
    };
    metadata_debug(&new_path, "save-video:done");

    metadata_debug(&new_path, "subtitles:start");
    if let Err(err) = extract_subtitles(&details, spawner).await {
        return Err(MetaDataError::from_error(MetaDataErrorCode::ExtractSubtitles, &err.as_ref(), &path, details));
    }
    metadata_debug(&new_path, "subtitles:done");

    Ok(Some(details))
}


async fn make_video_metadatas(path: &PathBuf, suggested_series: Option<String>) -> Result<VideoDetails, MetaDataError> {

    let (collection, video) = get_collection_and_video_from_path(&path);

    let mut details: VideoDetails = VideoDetails::new(video, collection, path, suggested_series);

    metadata_debug(path, "checksum:start");
    details.checksum = match calculate_checksum(&path).await {
        Ok(checksum) => checksum,
        Err(err) => return Err(MetaDataError::from_error(MetaDataErrorCode::CalculateChecksum, &err, &path, details)),
    };
    metadata_debug(path, "checksum:done");

    match fs::metadata(&path).await {
        Ok(metadata) => {
            let file_size = metadata.len();
            if file_size == 0 {
                details.state = VideoState::ZeroFileSize;
                return Err(MetaDataError::new(MetaDataErrorCode::ZeroFileSize, &path, details).into())
            } else {
                details.state = VideoState::NeedVideoMetaData;
            }
        }
        Err(e) => {
            details.state = VideoState::Exception;
            return Err(MetaDataError::from_error_no_details(MetaDataErrorCode::Exception, &e, &path).into())
        }
    }

    metadata_debug(path, "ffprobe:start");
    details.metadata = match get_video_metadata(&path).await {
        Ok(video_info) => video_info,
        Err(e) => {
            return Err(MetaDataError::from_error(MetaDataErrorCode::GetVideoMetaData, e.as_ref(), &path, details));
        }
    };
    metadata_debug(path, "ffprobe:done");
    
    details.state = VideoState::NeedThumbnail;

    metadata_debug(path, "thumbnail:start");
    let thumbnails = match extract_random_frame(path, &details.metadata).await {
        Ok(thumbnails) => thumbnails,
        Err(err) => return Err(MetaDataError::from_error(MetaDataErrorCode::ExtractFrame, &err, &path, details))
    };
    metadata_debug(path, "thumbnail:done");

    details.thumbnail = thumbnails;

    details.state = VideoState::Ready;

    Ok(details)
}


fn is_subdirectory(path: &Path, base: &Path) -> bool {
    // Canonicalize the paths to remove symbolic links and other artifacts
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_owned());

    // Check if the canonical path starts with the canonical base path
    canonical_path.starts_with(&canonical_base)
}

async fn is_file_being_written<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    // Get initial metadata
    let initial_metadata = fs::metadata(&path).await?;
    let initial_size = initial_metadata.len();

    // Wait a short time
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Get metadata again
    let current_metadata = fs::metadata(&path).await?;
    let current_size = current_metadata.len();

    // If size changed, file is still being written
    Ok(initial_size != current_size)
}

pub async fn calculate_checksum<P: AsRef<Path>>(path: P) -> io::Result<i64> {
    let file = match File::open(&path).await {
        Ok(file) => file,
        Err(e) => {
            tracing::error!("Failed to open file {}: {}", path.as_ref().display(), e);
            return Err(e);
        }
    };
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

    if total_read == 0 {
        hasher.write(path.as_ref().as_os_str().as_encoded_bytes());
    }

    Ok(hasher.finish() as i64)
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

    if let Some(streams) = json.get("streams").and_then(|s| s.as_array()) {
        for stream in streams {
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

    let (aspect_width, aspect_height) = match video_stream.get("display_aspect_ratio") {
        Some(Value::String(aspect_ratio)) => {
            let aspect_ratio_parts: Vec<&str> = aspect_ratio.splitn(2, ":").collect();
            match aspect_ratio_parts.as_slice() {
                [width, height] => (width.parse::<u32>()?, height.parse::<u32>()?),
                _ => (width, height),
            }
        }
        _ => (width, height),
    };

    Ok(VideoMetadata {
        duration,
        width,
        height,
        aspect_width,
        aspect_height,
        audio_tracks: audio_track_count,
        probe_data: Some(output_str.clone()),
        audio_track_list: None,
        subtitle_tracks: None,
    }.from_probe_data(&Some(output_str)))
}

async fn extract_random_frame<P: AsRef<Path>>(
    input_path: P,
    metadata: &VideoMetadata,
) -> io::Result<Vec<String>> {
    // Get video duration
    let mut duration = metadata.duration;
    
    if duration < 0.1 {
        return Ok(vec![]); // Early return if the duration is too short
    }

    // Skip the last 3 minutes if longer than 10 minutes
    if duration > 600.0 {
        duration -= 180.0;
    }

    // Generate a random timestamp for the final 1/4 of the video
    let random_time = {
        let mut rng = rand::thread_rng();
        rng.gen_range((3.0 * duration / 4.0)..duration)
    };

    let sizes = [
        (720, 450),
        (480, 300),
        (360, 225),
        (160, 100),
    ];

    let thumbnail_dir = get_thumbnail_dir(&get_movie_dir());
    
    // Ensure the thumbnail directory exists
    if !thumbnail_dir.exists() {
        std::fs::create_dir_all(&thumbnail_dir)?;
    }
    
    let mut paths = Vec::with_capacity(sizes.len());

    for (width, height) in sizes {
        let output_path = get_thumbnail_path_with_size(&thumbnail_dir, &input_path, width);
        
        // Create scale parameter for ffmpeg
        let scale = format!("scale={}:{}:force_original_aspect_ratio=increase,crop={}:{}", 
                           width, height, width, height);

        let output = Command::new("ffmpeg")
            .arg("-ss")
            .arg(format!("{}", random_time))
            .arg("-i")
            .arg(input_path.as_ref().as_os_str())
            .arg("-vf")
            .arg(scale)
            .arg("-vframes")
            .arg("1")
            .arg("-q:v")
            .arg("2")
            .arg("-update")
            .arg("1")
            .arg("-y")
            .arg(&output_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr).unwrap_or_default();
            tracing::error!("could not generate thumbnail: {}", stderr);
            let error_message = match stderr.trim() {
                "" => "ffmpeg exited with an error".to_string(),
                stderr => format!("ffmpeg exited with an error: {}", stderr),
            };
            return Err(io::Error::new(io::ErrorKind::Other, error_message));
        }

        // Add the filename to paths
        if let Some(filename) = output_path.file_name() {
            paths.push(filename.to_string_lossy().to_string());
        }
    }

    Ok(paths)
}

// Helper function to create a thumbnail path with a specific width
fn get_thumbnail_path_with_size<P: AsRef<Path>>(thumbnail_dir: &PathBuf, video: P, width: u32) -> PathBuf {
    let input_filename = video
        .as_ref()
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    // Replace problematic characters to avoid ffmpeg image2 muxer pattern matching issues
    // Spaces and dots can confuse the image2 muxer into thinking the filename is a pattern
    let sanitized_filename = input_filename
        .replace('.', "_")
        .replace(' ', "_")
        .replace('(', "_")
        .replace(')', "_");
    let output_filename = format!("{}_thumbnail_{}.jpg", sanitized_filename, width);
    thumbnail_dir.join(output_filename)
}
