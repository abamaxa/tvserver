use async_trait::async_trait;
use sqlx::migrate::{MigrateDatabase, MigrateError, Migrator};
use sqlx::sqlite::SqlitePool;
use sqlx::{Error, Sqlite};
use std::path;
use std::path::PathBuf;

use crate::domain::config::get_database_migration_dir;
use crate::domain::models::{SeriesDetails, VideoDetails, VideoMetadata};
use crate::domain::traits::Databaser;

const MEMORY_DB_URL: &str = ":memory:";

pub struct SqlRepository {
    pool: SqlitePool,
}

impl SqlRepository {
    pub async fn new(url: &str) -> Result<Self, Error> {
        if url != MEMORY_DB_URL && !Sqlite::database_exists(&url).await.unwrap_or(false) {
            Sqlite::create_database(&url).await?;
        }

        let pool = SqlitePool::connect(&url).await?;

        SqlRepository::do_migrations(&pool).await?;

        Ok(Self { pool })
    }

    async fn do_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
        let migrations_dir = get_database_migration_dir();

        let m = Migrator::new(path::Path::new(&migrations_dir)).await?;

        m.run(pool).await
    }
}

#[async_trait]
impl Databaser for SqlRepository {
    async fn save_video(&self, details: &VideoDetails) -> Result<i64, sqlx::Error> {
        let thumbnail = details.thumbnail.to_str().unwrap();

        sqlx::query!(
            r#"
            INSERT INTO video_details (
                checksum, 
                video, 
                collection, 
                description, 
                series_title, 
                season, 
                episode, 
                episode_title, 
                thumbnail, 
                duration, 
                width, 
                height, 
                audio_tracks, 
                search_phrase
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            details.checksum,
            details.video,
            details.collection,
            details.description,
            details.series.series_title,
            details.series.season,
            details.series.episode,
            details.series.episode_title,
            thumbnail,
            details.metadata.duration,
            details.metadata.width,
            details.metadata.height,
            details.metadata.audio_tracks,
            details.search_phrase,
        )
        .execute(&self.pool)
        .await
        .map(|result| result.last_insert_rowid()) // retrieve ID of last inserted row
    }

    async fn retrieve_video(&self, checksum: i64) -> Result<VideoDetails, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT 
                *
            FROM 
                video_details 
            WHERE 
                checksum = ?
            "#,
            checksum
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(VideoDetails {
            video: row.video,
            collection: row.collection,
            description: row.description.unwrap_or_default(),
            series: SeriesDetails {
                series_title: row.series_title.unwrap_or_default(),
                season: row.season.unwrap_or_default(),
                episode: row.episode.unwrap_or_default(),
                episode_title: row.episode_title.unwrap_or_default(),
            },
            thumbnail: PathBuf::from(row.thumbnail.unwrap_or_default()),
            metadata: VideoMetadata {
                duration: row.duration.unwrap_or_default(),
                width: row.width.unwrap_or(0) as u32,
                height: row.height.unwrap_or(0) as u32,
                audio_tracks: row.audio_tracks.unwrap_or(1) as u32,
            },
            checksum: row.checksum,
            search_phrase: row.search_phrase,
        })
    }

    async fn update_video(&self, details: &VideoDetails) -> Result<u64, sqlx::Error> {
        let thumbnail = details.thumbnail.to_str().unwrap_or_default(); // convert PathBuf to &str

        sqlx::query!(
            r#"
            UPDATE 
                video_details 
            SET 
                collection = ?, 
                description = ?, 
                series_title = ?, 
                season = ?, 
                episode = ?, 
                episode_title = ?, 
                thumbnail = ?, 
                duration = ?, 
                width = ?, 
                height = ?, 
                audio_tracks = ?,
                search_phrase = ?
            WHERE 
                checksum = ?
            "#,
            details.collection,
            details.description,
            details.series.series_title,
            details.series.season,
            details.series.episode,
            details.series.episode_title,
            thumbnail,
            details.metadata.duration,
            details.metadata.width,
            details.metadata.height,
            details.metadata.audio_tracks,
            details.search_phrase,
            details.checksum,
        )
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected())
    }

    async fn delete_video(&self, checksum: i64) -> Result<u64, sqlx::Error> {
        sqlx::query!(
            r#"
            DELETE FROM video_details 
            WHERE checksum = ?
            "#,
            checksum
        )
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected()) // return number of rows affected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_save_video_details() {
        // Create an in-memory SQLite database for testing.
        let db = SqlRepository::new(MEMORY_DB_URL).await.unwrap();

        // Define a VideoDetails instance.
        let video_details = VideoDetails {
            video: "test_video".to_string(),
            collection: "test_collection".to_string(),
            description: "test_description".to_string(),
            series: SeriesDetails {
                series_title: "test_series_title".to_string(),
                season: "test_season".to_string(),
                episode: "test_episode".to_string(),
                episode_title: "test_episode_title".to_string(),
            },
            thumbnail: PathBuf::from("test_path"),
            metadata: VideoMetadata {
                duration: 120.0,
                width: 1920,
                height: 1080,
                audio_tracks: 2,
            },
            checksum: 1234,
            search_phrase: None,
        };

        // Save the VideoDetails instance.
        let result = db.save_video(&video_details).await;

        // Verify that the method returned Ok.
        assert!(result.is_ok());

        // Verify that the ID of the inserted row is as expected (in this case, the video name as primary key).
        assert_eq!(result.unwrap(), video_details.checksum);

        let retrieved = db.retrieve_video(video_details.checksum).await;

        // Verify that the method returned Ok.
        assert!(retrieved.is_ok());

        let mut retrieved_row = retrieved.unwrap();

        assert_eq!(retrieved_row, video_details);

        retrieved_row.description = "A new description".to_string();

        let result = db.update_video(&retrieved_row).await;

        // Verify that the method returned Ok.
        assert!(result.is_ok());

        assert_eq!(result.unwrap(), 1);

        let updated_retrieved = db.retrieve_video(video_details.checksum).await;

        // Verify that the method returned Ok.
        assert!(updated_retrieved.is_ok());

        let updated_retrieved_row = updated_retrieved.unwrap();

        assert_eq!(updated_retrieved_row.description, retrieved_row.description);

        let result = db.delete_video(video_details.checksum).await;

        // Verify that the method returned Ok.
        assert!(result.is_ok());

        let fail_to_retrieved = db.retrieve_video(video_details.checksum).await;

        // Verify that the method returned Ok.
        assert!(fail_to_retrieved.is_err());
    }
}
