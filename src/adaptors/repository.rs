use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::{Error, Sqlite, Row};
use sqlx::migrate::{
    MigrateDatabase, 
    MigrateError, Migrator
};
use sqlx::sqlite::{SqlitePool, SqliteRow};
use std::path;
use serde_json;

use crate::domain::config::get_database_migration_dir;
use crate::domain::messages::{LocalMessage, LocalMessageSender, VideoEvent};
use crate::domain::models::{SeriesDetails, VideoDetails, VideoMetadata, CollectionItem};
use crate::domain::traits::Databaser;
use itertools::Itertools;

const MEMORY_DB_URL: &str = ":memory:";

pub struct SqlRepository {
    pool: SqlitePool,
    sender: Option<LocalMessageSender>,
}


impl SqlRepository {
    pub async fn new(url: &str, sender: Option<LocalMessageSender>) -> Result<Self, Error> {
        if url != MEMORY_DB_URL && !Sqlite::database_exists(&url).await.unwrap_or(false) {
            Sqlite::create_database(&url).await?;
        }

        let pool = SqlitePool::connect(&url).await?;

        SqlRepository::do_migrations(&pool).await?;

        Ok(Self { pool, sender })
    }

    async fn do_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
        let migrations_dir = get_database_migration_dir();

        let absolue_dir = path::Path::new(&std::env::current_dir().unwrap()).join(&migrations_dir);

        let m = Migrator::new(absolue_dir).await?;

        m.run(pool).await
    }

    fn from_record(row: &SqliteRow) -> VideoDetails {
        // Parse thumbnail JSON string into Vec<String>
        let thumbnail_str = row.get::<Option<String>, _>("thumbnail").unwrap_or_default();
        let thumbnail: Vec<String> = serde_json::from_str(&thumbnail_str).unwrap_or_default();

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
            thumbnail,
            metadata: VideoMetadata {
                duration: row.get::<Option<f64>, _>("duration").unwrap_or_default(),
                width: row.get::<Option<i32>, _>("width").unwrap_or(0) as u32,
                height: row.get::<Option<i32>, _>("height").unwrap_or(0) as u32,
                aspect_width: row.get::<Option<i32>, _>("aspect_width").unwrap_or(0) as u32,
                aspect_height: row.get::<Option<i32>, _>("aspect_height").unwrap_or(0) as u32,
                audio_tracks: row.get::<Option<i32>, _>("audio_tracks").unwrap_or(1) as u32,
                probe_data: row.get::<Option<String>, _>("probe_data"),
            },
            checksum: row.get("checksum"),
            search_phrase: row.get("search_phrase"),
            state: row.get::<i32,_>("state").into(),
            created_on: row.get("created_on"),
            updated_on: row.get("updated_on"),
            play_from: None,
            last_viewed: None,
            dir_path: None,
        }
    }

    fn from_record_with_last_seen(row: &SqliteRow) -> VideoDetails {
        let mut video_details = Self::from_record(row);

        video_details.last_viewed = row.get::<Option<NaiveDateTime>, _>("last_viewed");
        video_details.play_from = row.get::<Option<NaiveDateTime>, _>("play_from");

        video_details
    }
}


#[async_trait]
impl Databaser for SqlRepository {
    async fn save_video(&self, details: &VideoDetails) -> Result<i64, sqlx::Error> {
        // Convert Vec<String> to JSON string
        let thumbnail = serde_json::to_string(&details.thumbnail).unwrap_or_default();
        let state: i32 = details.state as i32;

        // Using raw query to handle both conflict scenarios directly
        let query = r#"
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
                aspect_width, 
                aspect_height, 
                audio_tracks, 
                probe_data,
                search_phrase,
                state
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(collection, video) DO UPDATE SET
                checksum = ?,
                description = ?,
                series_title = ?,
                season = ?,
                episode = ?,
                episode_title = ?,
                thumbnail = ?,
                duration = ?,
                width = ?,
                height = ?,
                aspect_width = ?,
                aspect_height = ?,
                audio_tracks = ?,
                probe_data = ?,
                search_phrase = ?,
                state = ?,
                updated_on = CURRENT_TIMESTAMP
            ON CONFLICT(checksum) DO UPDATE SET
                video = ?,
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
                aspect_width = ?,
                aspect_height = ?,
                audio_tracks = ?,
                probe_data = ?,
                search_phrase = ?,
                state = ?,
                updated_on = CURRENT_TIMESTAMP
        "#;

        // Execute with all bindings (for INSERT, then UPDATE)
        let result = sqlx::query(query)
            // Values for the INSERT
            .bind(details.checksum)
            .bind(&details.video)
            .bind(&details.collection)
            .bind(&details.description)
            .bind(&details.series.series_title)
            .bind(&details.series.season)
            .bind(&details.series.episode)
            .bind(&details.series.episode_title)
            .bind(&thumbnail)
            .bind(details.metadata.duration)
            .bind(details.metadata.width as i32)
            .bind(details.metadata.height as i32)
            .bind(details.metadata.aspect_width as i32)
            .bind(details.metadata.aspect_height as i32)
            .bind(details.metadata.audio_tracks as i32)
            .bind(&details.metadata.probe_data)
            .bind(&details.search_phrase)
            .bind(state)
            // Values for the ON CONFLICT UPDATE (video, collection)
            .bind(details.checksum)
            .bind(&details.description)
            .bind(&details.series.series_title)
            .bind(&details.series.season)
            .bind(&details.series.episode)
            .bind(&details.series.episode_title)
            .bind(&thumbnail)
            .bind(details.metadata.duration)
            .bind(details.metadata.width as i32)
            .bind(details.metadata.height as i32)
            .bind(details.metadata.aspect_width as i32)
            .bind(details.metadata.aspect_height as i32)
            .bind(details.metadata.audio_tracks as i32)
            .bind(&details.metadata.probe_data)
            .bind(&details.search_phrase)
            .bind(state)
            // Values for the ON CONFLICT UPDATE (checksum)
            .bind(&details.video)
            .bind(&details.collection)
            .bind(&details.description)
            .bind(&details.series.series_title)
            .bind(&details.series.season)
            .bind(&details.series.episode)
            .bind(&details.series.episode_title)
            .bind(&thumbnail)
            .bind(details.metadata.duration)
            .bind(details.metadata.width as i32)
            .bind(details.metadata.height as i32)
            .bind(details.metadata.aspect_width as i32)
            .bind(details.metadata.aspect_height as i32)
            .bind(details.metadata.audio_tracks as i32)
            .bind(&details.metadata.probe_data)
            .bind(&details.search_phrase)
            .bind(state)
            .execute(&self.pool)
            .await;

        if let Err(e) = result {
            tracing::error!("Error saving video: {}", e);
            return Err(e);
        }

        let last_id = result.unwrap().last_insert_rowid();

        if let Some(sender) = &self.sender {
            let message = match last_id {
                0 => LocalMessage::Video(VideoEvent::new_video_changed_event(details.clone())),
                _ => LocalMessage::Video(VideoEvent::new_video_added_event(details.clone())),
            };
            
            if let Err(e) = sender.send(message).await {
                tracing::error!("Error sending video event {} {}", last_id, e);
            }
        }

        Ok(last_id)
    }

    async fn list_videos(&self, collection: &str)  -> Result<Vec<VideoDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
		SELECT
			vd.*, 
	   		h.updated_on as last_viewed, 
	   		h.stopped as play_from
   		FROM 
	   		video_details vd 
	   		LEFT JOIN history h 
	   		ON vd.checksum = h.checksum  
		WHERE 
			collection = ?
            "#,
        )
        .bind(collection)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            results.push(Self::from_record_with_last_seen(&row));
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

    async fn list_all_series(&self) -> Result<Vec<CollectionItem>, sqlx::Error> {
        // Execute the SQL query to retrieve series titles along with a representative thumbnail.
        let rows = sqlx::query!(
            r#"
            SELECT series_title, MIN(thumbnail) as thumbnail
            FROM video_details
            GROUP BY series_title
            ORDER BY series_title
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        
        let mut series = Vec::new();
        for row in rows {
            // Convert the JSON string in the thumbnail column to a Vec<String>.
            // If the thumbnail is null or invalid, default to an empty Vec.
            let thumbnails: Vec<String> = match row.thumbnail {
                Some(ts) => serde_json::from_str(&ts).unwrap_or_default(),
                None => Vec::new(),
            };
            
            // Create a new CollectionItem and add it to the result list.
            series.push(CollectionItem {
                collection: row.series_title.unwrap_or_default(),
                thumbnail: thumbnails,
            });
        }
        Ok(series)
    }

    async fn list_series_details(
        &self,
        series: &str,
        season: Option<&str>,
    ) -> Result<Vec<VideoDetails>, sqlx::Error> {
        // Build the SQL query based on whether `season` is provided.
        let query = if let Some(season_value) = season {
            sqlx::query(
                "
                SELECT
                    vd.*, 
                    h.updated_on AS last_viewed, 
                    h.stopped AS play_from
                FROM 
                    video_details vd 
                    LEFT JOIN history h ON vd.checksum = h.checksum
                WHERE 
                    series_title = ? AND season = ?
                ORDER BY
                    episode, episode_title
                ",
            )
            .bind(series)
            .bind(season_value)
        } else {
            sqlx::query(
                "
                SELECT
                    vd.*, 
                    h.updated_on AS last_viewed, 
                    h.stopped AS play_from
                FROM 
                    video_details vd 
                    LEFT JOIN history h ON vd.checksum = h.checksum
                WHERE 
                    series_title = ?
                ORDER BY
                    season, episode, episode_title
                ",
            )
            .bind(series)
        };

        // Execute the query asynchronously and await the results.
        let rows = query.fetch_all(&self.pool).await?;

        // Process each returned row using `from_record_with_last_seen`
        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            // Assume that from_record_with_last_seen returns a Result<VideoDetail, Error>
            let video_detail = Self::from_record_with_last_seen(&row);
            results.push(video_detail);
        }

        Ok(results)
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

    async fn delete_video(&self, checksum: i64) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!(
            r#"
            DELETE FROM video_details 
            WHERE checksum = ?
            "#,
            checksum
        )
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected()); // return number of rows affected

        if let Ok(rows) = result {
            if rows > 0 {
                if let Some(sender) = &self.sender {
                    if let Err(e) = sender.send(LocalMessage::Video(VideoEvent::new_video_deleted_event(checksum))).await {
                        tracing::error!("Error sending video deleted event {} {}", checksum, e);
                    }
                }
            }
        }

        result
    }

    async fn update_watched_video(&self, checksum: i64, current_time: f64) -> Result<(), sqlx::Error> {
        let query = r#"
            INSERT INTO history (
                checksum,
                started,
                stopped
            ) VALUES (
                ?,
                ?,
                ?
            )
            ON CONFLICT(checksum) DO UPDATE SET
                started = MIN(started, ?),
                stopped = ?,
                updated_on = CURRENT_TIMESTAMP
            WHERE checksum = ?
        "#;

        // Convert current_time from seconds to a timestamp
        let timestamp = DateTime::<Utc>::from_timestamp(current_time as i64, 0)
            .unwrap_or_else(|| Utc::now())
            .naive_local();

        sqlx::query(query)
            .bind(checksum)
            .bind(timestamp)
            .bind(timestamp)
            .bind(timestamp)
            .bind(timestamp)
            .bind(checksum)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn get_history(&self, offset: i32, limit: i32) -> Result<Vec<VideoDetails>, sqlx::Error> {
        let query = r#"
            SELECT
                vd.*, 
                h.updated_on as last_viewed, 
                h.stopped as play_from
            FROM 
                video_details vd 
                INNER JOIN history h 
                    ON vd.checksum = h.checksum 
            ORDER BY    
                h.updated_on DESC
            LIMIT ?
            OFFSET ?
        "#;

        let rows = sqlx::query(query)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let mut history = Vec::with_capacity(rows.len());
        for row in rows {
            // Assume from_record_with_last_seen takes the row and returns a Result<VideoDetails, _>
            let video_details = Self::from_record_with_last_seen(&row);
            history.push(video_details);
        }
        Ok(history)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Local;

    use crate::domain::models::VideoState;

    use super::*;

    #[tokio::test]
    async fn test_save_video_details() {
        // Create an in-memory SQLite database for testing.
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
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
            thumbnail: vec!["test_path".to_string()],
            metadata: VideoMetadata {
                duration: 120.0,
                width: 1920,
                height: 1080,
                aspect_width: 1920,
                aspect_height: 1080,
                audio_tracks: 2,
                probe_data: None,
            },
            checksum: 1234,
            search_phrase: None,
            state: VideoState::Ready,
            created_on: now,
            updated_on: now,
            play_from: None,
            last_viewed: None,
            dir_path: None,
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
