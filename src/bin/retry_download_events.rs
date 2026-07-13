use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use app_lib::adaptors::{FileSystemStore, SqlRepository, TokioProcessSpawner};
use app_lib::domain::algorithm::{
    get_collection_from_path, get_videos_for_series_or_id, skip_file, title_case,
};
use app_lib::domain::config::{get_database_url, get_movie_dir, get_thumbnail_dir};
use app_lib::domain::messages::MediaItem;
use app_lib::domain::models::CollectionDetails;
use app_lib::domain::services::generate_video_metadatas;
use app_lib::domain::traits::{FileStorer, MediaStorer, Repository, Storer};
use async_trait::async_trait;
use tokio::fs as tokio_fs;

#[derive(Debug)]
struct FailedMediaEvent {
    path: PathBuf,
    search: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let log_path = args.get(1).map(String::as_str).unwrap_or("download.log");
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let copy_only = args.iter().any(|arg| arg == "--copy");
    let limit = parse_limit(&args)?;
    let offset = parse_offset(&args)?;
    env::set_var("TVSERVER_METADATA_DEBUG", "1");

    let mut events = read_failed_events(log_path)
        .with_context(|| format!("failed to read failed media events from {}", log_path))?;
    let total_events = events.len();

    if offset > 0 {
        events = events.into_iter().skip(offset).collect();
    }

    if let Some(limit) = limit {
        events.truncate(limit);
        println!(
            "found {} failed media events in {} (skipping {}, processing {})",
            total_events,
            log_path,
            offset,
            events.len()
        );
    } else {
        println!(
            "found {} failed media events in {} (skipping {}, processing {})",
            total_events,
            log_path,
            offset,
            events.len()
        );
    }

    if dry_run {
        for event in &events {
            println!(
                "would process: {} search={:?}",
                event.path.display(),
                event.search
            );
        }
        return Ok(());
    }

    let repository: Repository =
        Arc::new(SqlRepository::new(&get_database_url(), None).await?);
    let storer: Storer = if copy_only {
        Arc::new(CopyingMediaStore::new(repository.clone()))
    } else {
        let file_storer: FileStorer = Arc::new(FileSystemStore::new(&get_movie_dir()));
        Arc::new(app_lib::services::MediaStore::new(
            file_storer,
            repository.clone(),
        ))
    };
    let spawner = Arc::new(TokioProcessSpawner::new());

    let mut processed = 0usize;
    let mut skipped_missing = 0usize;
    let mut failed = 0usize;

    for event in events {
        if !event.path.exists() {
            skipped_missing += 1;
            println!("skip missing source: {}", event.path.display());
            continue;
        }

        println!("processing: {}", event.path.display());
        match generate_video_metadatas(
            event.path.clone(),
            storer.clone(),
            repository.clone(),
            event.search.clone(),
            spawner.clone(),
        )
        .await
        {
            Ok(Some(details)) => {
                processed += 1;
                make_processed_files_readable(&details.thumbnail).await;
                println!(
                    "processed: {}/{} checksum={}",
                    details.collection, details.video, details.checksum
                );
            }
            Ok(None) => {
                skipped_missing += 1;
                println!("skipped without processing: {}", event.path.display());
            }
            Err(err) => {
                failed += 1;
                eprintln!("failed: {}: {}", event.path.display(), err);
            }
        }
    }

    println!(
        "retry complete: processed={}, skipped_missing={}, failed={}",
        processed, skipped_missing, failed
    );

    if failed > 0 {
        anyhow::bail!("{} failed media events remain", failed);
    }

    Ok(())
}

fn parse_limit(args: &[String]) -> Result<Option<usize>> {
    parse_optional_usize_arg(args, "--limit")
}

fn parse_offset(args: &[String]) -> Result<usize> {
    Ok(parse_optional_usize_arg(args, "--offset")?.unwrap_or(0))
}

fn parse_optional_usize_arg(args: &[String], name: &str) -> Result<Option<usize>> {
    let equals_prefix = format!("{}=", name);
    if let Some(value) = args.iter().find_map(|arg| arg.strip_prefix(&equals_prefix)) {
        return Ok(Some(
            value
                .parse::<usize>()
                .with_context(|| format!("{} must be a number", name))?,
        ));
    }

    let Some(index) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };

    let value = args
        .get(index + 1)
        .with_context(|| format!("{} requires a number", name))?;
    Ok(Some(
        value
            .parse::<usize>()
            .with_context(|| format!("{} must be a number", name))?,
    ))
}

fn read_failed_events<P: AsRef<Path>>(log_path: P) -> Result<Vec<FailedMediaEvent>> {
    let contents = fs::read_to_string(log_path)?;
    let mut searches: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut failures = Vec::new();
    let mut seen_failures = HashSet::new();

    for line in contents.lines() {
        if let Some((path, search)) = parse_sent_media_event(line) {
            searches.insert(path, search);
            continue;
        }

        if let Some(path) = parse_failed_media_event(line) {
            if seen_failures.insert(path.clone()) {
                failures.push(path);
            }
        }
    }

    Ok(failures
        .into_iter()
        .map(|path| {
            let search = searches.get(&path).cloned().unwrap_or(None);
            FailedMediaEvent { path, search }
        })
        .collect())
}

fn parse_sent_media_event(line: &str) -> Option<(PathBuf, Option<String>)> {
    const PREFIX: &str = "MediaAdded { full_path: \"";
    let start = line.find(PREFIX)? + PREFIX.len();
    let remaining = &line[start..];
    let path_end = remaining.find("\", search: ")?;
    let path = PathBuf::from(&remaining[..path_end]);

    let search_start = start + path_end + "\", search: ".len();
    let search_remaining = &line[search_start..];
    let search_end = search_remaining.find(", date:")?;
    let search = parse_optional_string(&search_remaining[..search_end]);

    Some((path, search))
}

fn parse_failed_media_event(line: &str) -> Option<PathBuf> {
    if !line.contains("ERROR processing MediaAvailable") {
        return None;
    }

    let start = line.find("(\"")? + 2;
    let remaining = &line[start..];
    let end = remaining.find("\")")?;

    Some(PathBuf::from(&remaining[..end]))
}

fn parse_optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value == "None" {
        return None;
    }

    value
        .strip_prefix("Some(\"")
        .and_then(|inner| inner.strip_suffix("\")"))
        .map(str::to_string)
}

#[derive(Clone)]
struct CopyingMediaStore {
    repo: Repository,
}

impl CopyingMediaStore {
    fn new(repo: Repository) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl MediaStorer for CopyingMediaStore {
    async fn list(&self, series: &str) -> anyhow::Result<MediaItem> {
        let videos = get_videos_for_series_or_id(self.repo.clone(), series).await?;
        Ok(MediaItem::Collection(CollectionDetails::new(
            series.to_string(),
            Vec::new(),
            videos,
        )))
    }

    async fn add_file(
        &self,
        full_path: &Path,
        suggested_series: Option<String>,
    ) -> anyhow::Result<PathBuf> {
        let full_path_str = full_path.to_str().unwrap_or_default();
        if skip_file(full_path_str) {
            anyhow::bail!("Skipping file: {}", full_path_str);
        }

        let collection = match suggested_series {
            Some(series) => title_case(&series),
            None => get_collection_from_path(full_path),
        };

        let destination_dir = Path::new(&get_movie_dir()).join(collection);
        tokio_fs::create_dir_all(&destination_dir).await?;
        tokio_fs::set_permissions(&destination_dir, fs::Permissions::from_mode(0o775)).await?;

        let destination = destination_dir.join(full_path.file_name().unwrap_or_default());
        if full_path == destination {
            eprintln!("stage: copy:already-in-place: {}", destination.display());
            return Ok(destination);
        }

        if !destination.exists() {
            let bytes = fs::metadata(full_path).map(|metadata| metadata.len()).unwrap_or(0);
            eprintln!(
                "stage: copy:start: {} -> {} bytes={}",
                full_path.display(),
                destination.display(),
                bytes
            );
            let started = Instant::now();
            let copied = tokio_fs::copy(full_path, &destination).await?;
            eprintln!(
                "stage: copy:done: {} bytes={} elapsed={:.2?}",
                destination.display(),
                copied,
                started.elapsed()
            );
        } else {
            eprintln!("stage: copy:skip-existing: {}", destination.display());
        }

        let _ = tokio_fs::set_permissions(&destination, fs::Permissions::from_mode(0o664)).await;
        Ok(destination)
    }

    async fn delete(&self, _video_id: String) -> anyhow::Result<()> {
        anyhow::bail!("delete is not supported by retry_download_events")
    }
}

async fn make_processed_files_readable(thumbnails: &[String]) {
    let thumbnail_dir = get_thumbnail_dir(&get_movie_dir());
    for thumbnail in thumbnails {
        let path = thumbnail_dir.join(thumbnail);
        let _ = tokio_fs::set_permissions(path, fs::Permissions::from_mode(0o664)).await;
    }
}
