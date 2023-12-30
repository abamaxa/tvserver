use crate::domain::algorithm::get_collection_and_video_from_path;
use crate::domain::config::{get_movie_dir, get_thumbnail_dir};
use crate::domain::models::{VideoDetails, VideoMetadata, VideoState};
use crate::domain::traits::Repository;
use rand::Rng;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::Hasher;
use std::os::unix::ffi::OsStrExt;
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
    GetVideoMetaData = 7
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

pub async fn generate_video_metadatas(path: PathBuf, repo: Repository) -> Result<Option<VideoDetails>, MetaDataError> {
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

    let (details, err) = match make_video_metadatas(&path).await {
        Ok(details) => (details, None),
        Err(err) => (err.video_details.clone(), Some(err))
    };

    if details.checksum != 0 {
        if let Err(err) = repo.save_video(&details).await {
            return Err(MetaDataError::from_error(MetaDataErrorCode::SaveVideo, &err, &path, details));
        };
    }

    match err {
        Some(err) => Err(err),
        None => Ok(Some(details))
    }
}


async fn make_video_metadatas(path: &PathBuf) -> Result<VideoDetails, MetaDataError> {

    let (collection, video) = get_collection_and_video_from_path(&path);

    let mut details: VideoDetails = VideoDetails::new(video, collection, &path);

    details.checksum = match calculate_checksum(&path).await {
        Ok(checksum) => checksum,
        Err(err) => return Err(MetaDataError::from_error(MetaDataErrorCode::CalculateChecksum, &err, &path, details)),
    };

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

    details.metadata = match get_video_metadata(&path).await {
        Ok(video_info) => video_info,
        Err(e) => {
            return Err(MetaDataError::from_error(MetaDataErrorCode::GetVideoMetaData, e.as_ref(), &path, details));
        }
    };
    
    details.state = VideoState::NeedThumbnail;
    let thumbnail_dir: PathBuf = get_thumbnail_dir(&get_movie_dir());
    let output_path = get_thumbnail_path(&thumbnail_dir, &path);

    if let Err(err) = extract_random_frame(path, &output_path, details.metadata.clone()).await {
        return Err(MetaDataError::from_error(MetaDataErrorCode::ExtractFrame, &err, &path, details));
    }

    details.thumbnail = match output_path.strip_prefix(&thumbnail_dir) {
        Ok(thumbnail) => PathBuf::from(thumbnail),
        _ => PathBuf::new(),
    };

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

pub async fn calculate_checksum<P: AsRef<Path>>(path: P) -> io::Result<i64> {
    let file = File::open(&path).await?;
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
        hasher.write(path.as_ref().as_os_str().as_bytes());
    }

    Ok(hasher.finish() as i64)
}

fn get_thumbnail_path<P: AsRef<Path>>(thumbnail_dir: &PathBuf, video: P) -> PathBuf {
    let input_filename = video
        .as_ref()
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let output_filename = format!("{}_thumbnail.jpg", input_filename);
    thumbnail_dir.join(output_filename)
}

async fn get_video_metadata<P: AsRef<Path>>(path: P) -> Result<VideoMetadata, Box<dyn Error>> {
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

async fn extract_random_frame<P: AsRef<Path>>(
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


