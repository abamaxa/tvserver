use std::sync::Arc;

use anyhow::Result;
use app_lib::{
    adaptors::{SqlRepository, TokioProcessSpawner},
    domain::{
        config::get_database_url,
        services::{extract_subtitles, get_video_metadata},
        traits::{Databaser, ProcessSpawner},
    },
    services::setup_logging,
};

const LOG_FILTER: &str = "subtitle_tool=info,app_lib=info";

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging(LOG_FILTER);

    let db_url = get_database_url();
    let repository = Arc::new(SqlRepository::new(&db_url, None).await?);
    let spawner: Arc<dyn ProcessSpawner> = Arc::new(TokioProcessSpawner::new());

    let videos = repository.list_all_videos().await?;
    println!("Found {} videos in database", videos.len());

    let mut extracted_count = 0;
    let mut skipped_count = 0;
    let mut error_count = 0;

    for mut video in videos {
        let path = video.get_full_path();

        if !path.exists() {
            skipped_count += 1;
            continue;
        }

        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Check if VTT files already exist for this video
        let file_stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let parent_dir = path.parent().unwrap_or(std::path::Path::new(""));
        let has_existing_vtts = std::fs::read_dir(parent_dir)
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with(file_stem.as_ref()) && name.ends_with(".vtt")
                })
            })
            .unwrap_or(false);

        if has_existing_vtts {
            skipped_count += 1;
            continue;
        }

        // Re-probe to get fresh subtitle track info (with codec data)
        let needs_reprobe = video.metadata.subtitle_tracks.is_none()
            || video
                .metadata
                .subtitle_tracks
                .as_ref()
                .map(|tracks| tracks.iter().any(|t| t.codec.is_none()))
                .unwrap_or(false);

        if needs_reprobe {
            match get_video_metadata(&path).await {
                Ok(new_metadata) => {
                    video.metadata.subtitle_tracks = new_metadata.subtitle_tracks;
                    video.metadata.probe_data = new_metadata.probe_data;
                    if video.metadata.subtitle_tracks.as_ref().map_or(false, |t| !t.is_empty()) {
                        if let Err(e) = repository.save_video(&video).await {
                            tracing::warn!("Could not update metadata for {}: {}", video.video, e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to probe {} ({}): {}", video.video, extension, e);
                    error_count += 1;
                    continue;
                }
            }
        }

        let subtitle_tracks = match &video.metadata.subtitle_tracks {
            Some(tracks) if !tracks.is_empty() => tracks,
            _ => {
                skipped_count += 1;
                continue;
            }
        };

        let text_tracks: Vec<_> = subtitle_tracks
            .iter()
            .filter(|t| {
                !matches!(
                    t.codec.as_deref(),
                    Some("hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub")
                )
            })
            .collect();

        if text_tracks.is_empty() {
            tracing::info!(
                "No text subtitle tracks in {} (only bitmap subtitles)",
                video.video
            );
            skipped_count += 1;
            continue;
        }

        println!(
            "Extracting {} subtitle track(s) from: {}",
            text_tracks.len(),
            video.video
        );

        match extract_subtitles(&video, spawner.clone()).await {
            Ok(files) => {
                if files.is_empty() {
                    tracing::warn!("No subtitles extracted from {}", video.video);
                    error_count += 1;
                } else {
                    for f in &files {
                        println!("  -> {}", f.display());
                    }
                    extracted_count += 1;
                }
            }
            Err(e) => {
                tracing::error!("Error extracting subtitles from {}: {}", video.video, e);
                error_count += 1;
            }
        }
    }

    println!();
    println!("Done!");
    println!(
        "  Extracted: {} videos, Skipped: {}, Errors: {}",
        extracted_count, skipped_count, error_count
    );

    Ok(())
}
