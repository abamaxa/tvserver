use async_trait::async_trait;
use sqlx::{Error, Sqlite, Row};
use sqlx::migrate::{
    MigrateDatabase, 
    MigrateError, Migrator
};
use sqlx::sqlite::{SqlitePool, SqliteRow};
use std::path;
use std::path::PathBuf;

use crate::domain::config::get_database_migration_dir;
use crate::domain::models::{SeriesDetails, VideoDetails, VideoMetadata};
use crate::domain::traits::Databaser;
use itertools::Itertools;

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

        let absolue_dir = path::Path::new(&std::env::current_dir().unwrap()).join(&migrations_dir);

        let m = Migrator::new(absolue_dir).await?;

        m.run(pool).await
    }

    fn from_record(row: &SqliteRow) -> VideoDetails {
        VideoDetails {
            video: row.get("video"),
            collection: row.get("collection"),
            description: row.get::<Option<String>, _>("description").unwrap_or_default(),
            series: SeriesDetails {
                series_title: row.get::<Option<String>, _>("series_title").unwrap_or_default(),
                season: row.get::<Option<String>, _>("season").unwrap_or_default(),
                episode: row.get::<Option<String>, _>("episode").unwrap_or_default(),
                episode_title: row.get::<Option<String>, _>("episode_title").unwrap_or_default(),
            },
            thumbnail: PathBuf::from(row.get::<Option<String>, _>("thumbnail").unwrap_or_default()),
            metadata: VideoMetadata {
                duration: row.get::<Option<f64>, _>("duration").unwrap_or_default(),
                width: row.get::<Option<i32>, _>("width").unwrap_or(0) as u32,
                height: row.get::<Option<i32>, _>("height").unwrap_or(0) as u32,
                audio_tracks: row.get::<Option<i32>, _>("audio_tracks").unwrap_or(1) as u32,
            },
            checksum: row.get("checksum"),
            search_phrase: row.get("search_phrase"),
            state: row.get::<i32,_>("state").into(),
            created_on: row.get("created_on"),
            updated_on: row.get("updated_on"),
        }
    }
}

#[async_trait]
impl Databaser for SqlRepository {
    async fn save_video(&self, details: &VideoDetails) -> Result<i64, sqlx::Error> {
        let thumbnail = details.thumbnail.to_str().unwrap();
        let state: i32 = details.state as i32;

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
                search_phrase,
                state
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(checksum) DO UPDATE SET
                video = excluded.video, 
                collection = excluded.collection, 
                description = excluded.description, 
                series_title = excluded.series_title, 
                season = excluded.season, 
                episode = excluded.episode, 
                episode_title = excluded.episode_title, 
                thumbnail = excluded.thumbnail, 
                duration = excluded.duration, 
                width = excluded.width, 
                height = excluded.height, 
                audio_tracks = excluded.audio_tracks, 
                search_phrase = excluded.search_phrase,
                state = state,
                updated_on = CURRENT_TIMESTAMP
            WHERE
                (
                    video != excluded.video OR
                    collection != excluded.collection OR
                    description != excluded.description OR
                    series_title != excluded.series_title  OR
                    season != excluded.season OR 
                    episode != excluded.episode OR 
                    episode_title != excluded.episode_title OR 
                    thumbnail != excluded.thumbnail OR
                    duration != excluded.duration OR
                    width != excluded.width OR
                    height != excluded.height OR
                    audio_tracks != excluded.audio_tracks OR
                    state != excluded.state
                ) AND (
                    excluded.width != 0 AND
                    excluded.height != 0 AND
                    excluded.duration != 0
                )
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
            state
        )
        .execute(&self.pool)
        .await
        .map(|result| result.last_insert_rowid()) // retrieve ID of last inserted row
    }

    async fn list_videos(&self, collection: &str)  -> Result<Vec<VideoDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT 
                *
            FROM 
                video_details 
            WHERE 
                collection = ?
            "#,
        )
        .bind(collection)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            results.push(Self::from_record(&row));
        }

        Ok(results)
    }

    async fn list_collection(&self, parent_collection: &str)  -> Result<Vec<String>, sqlx::Error> {
        let rows = match parent_collection {
            "" => sqlx::query(
                    r#"
                    SELECT DISTINCT
                        collection
                    FROM 
                        video_details
                    WHERE
                        collection <> ""
                    "#
                )
                .fetch_all(&self.pool)
                .await?,
            _ => {
                let _collection = format!("{}%", parent_collection);
                sqlx::query(
                    r#"
                    SELECT DISTINCT
                        collection
                    FROM 
                        video_details 
                    WHERE 
                        collection LIKE ?
                    "#
                ).bind(_collection)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let pick_part = if parent_collection == "" { 0 } else {parent_collection.matches('/').count() + 1};

        Ok(
            rows.into_iter()
                .map(|row| row.get::<String, _>("collection"))
                .filter_map(|s| {
                    s.split('/')
                        .nth(pick_part)
                        .map(str::to_string)
                })
                .unique()
                .sorted()
                .collect()
        )

    }

    async fn retrieve_video(&self, checksum: i64) -> Result<VideoDetails, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT 
                *
            FROM 
                video_details 
            WHERE 
                checksum = ?
            "#
        )
        .bind(checksum)
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::from_record(&row))
    }

    async fn retrieve_video_by_name_and_collection(&self, name: &str, collection: &str) -> Result<VideoDetails, sqlx::Error> {
        let row = sqlx::query(
            r#"
            SELECT 
                *
            FROM 
                video_details 
            WHERE 
                video = ? and collection = ?
            "#
        )
        .bind(name)
        .bind(collection)
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::from_record(&row))
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
    use chrono::Local;

    use crate::domain::models::VideoState;

    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_save_video_details() {
        // Create an in-memory SQLite database for testing.
        let db = SqlRepository::new(MEMORY_DB_URL).await.unwrap();
        let now = Local::now().naive_local();

        // Define a VideoDetails instance.
        let mut video_details = VideoDetails {
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
            state: VideoState::Ready,
            created_on: now,
            updated_on: now,
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

        //assert!(retrieved_row.updated_on != video_details.updated_on);
        video_details.created_on = retrieved_row.created_on;
        video_details.updated_on = retrieved_row.updated_on;

        assert_eq!(retrieved_row, video_details);

        retrieved_row.description = "A new description".to_string();

        let result = db.save_video(&retrieved_row).await;

        // Verify that the method returned Ok.
        assert!(result.is_ok());

        assert_eq!(result.unwrap(), 1234);

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
