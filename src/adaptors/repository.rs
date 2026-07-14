use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde_json;
use sqlx::migrate::{MigrateDatabase, MigrateError, Migrator};
use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{Error, Row, Sqlite};

use crate::domain::algorithm::get_thumbnails_url;
use crate::domain::config::get_database_migration_dir;
use crate::domain::messages::{BookEvent, LocalMessage, LocalMessageSender, VideoEvent};
use crate::domain::models::{
    BookDetails, BookFormat, BookMetadata, CollectionItem, SeriesDetails, VideoDetails,
    VideoMetadata,
};
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

        let m = Migrator::new(migrations_dir.as_path()).await?;

        m.run(pool).await
    }

    fn from_record(row: &SqliteRow) -> VideoDetails {
        // Parse thumbnail JSON string into Vec<String>
        let thumbnail_str = row.get::<Option<String>, _>("thumbnail").unwrap_or_default();
        let thumbnail: Vec<String> = serde_json::from_str(&thumbnail_str).unwrap_or_default();

        let probe_data_str = row.get::<Option<String>, _>("probe_data");
        let metadata = VideoMetadata {
            duration: row.get::<Option<f64>, _>("duration").unwrap_or_default(),
            width: row.get::<Option<i32>, _>("width").unwrap_or(0) as u32,
            height: row.get::<Option<i32>, _>("height").unwrap_or(0) as u32,
            aspect_width: row.get::<Option<i32>, _>("aspect_width").unwrap_or(0) as u32,
            aspect_height: row.get::<Option<i32>, _>("aspect_height").unwrap_or(0) as u32,
            audio_tracks: row.get::<Option<i32>, _>("audio_tracks").unwrap_or(1) as u32,
            probe_data: probe_data_str.clone(),
            audio_track_list: None,
            subtitle_tracks: None,
        }.from_probe_data(&probe_data_str);

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
            metadata,
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
        video_details.play_from = row.get::<Option<f32>, _>("play_from");

        video_details
    }

    fn book_from_record(row: &SqliteRow) -> BookDetails {
        let authors = row
            .get::<Option<String>, _>("authors")
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default();
        let metadata = row
            .get::<Option<String>, _>("metadata")
            .and_then(|value| serde_json::from_str::<BookMetadata>(&value).ok())
            .unwrap_or_default();
        let format = match row.get::<String, _>("format").as_str() {
            "epub" => BookFormat::Epub,
            _ => BookFormat::Pdf,
        };

        BookDetails {
            file_name: row.get("file_name"),
            collection: row.get("collection"),
            title: row.get("title"),
            authors,
            description: row.get("description"),
            publisher: row.get("publisher"),
            published_date: row.get("published_date"),
            language: row.get("language"),
            isbn: row.get("isbn"),
            format,
            page_count: row.get("page_count"),
            thumbnail: row.get("thumbnail"),
            metadata,
            checksum: row.get("checksum"),
            search_phrase: row.get("search_phrase"),
            state: row.get::<i32, _>("state").into(),
            created_on: row.get("created_on"),
            updated_on: row.get("updated_on"),
            dir_path: None,
        }
    }
}

#[async_trait]
impl Databaser for SqlRepository {
    async fn save_book(&self, details: &BookDetails) -> Result<i64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let is_update = sqlx::query(
            r#"
            SELECT 1
            FROM books
            WHERE checksum = ? OR (collection = ? AND file_name = ?)
            "#,
        )
        .bind(details.checksum)
        .bind(&details.collection)
        .bind(&details.file_name)
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
        let authors = serde_json::to_string(&details.authors).unwrap_or_default();
        let metadata = serde_json::to_string(&details.metadata).unwrap_or_default();
        let format = match details.format {
            BookFormat::Pdf => "pdf",
            BookFormat::Epub => "epub",
        };

        sqlx::query(
            r#"
            INSERT INTO books (
                checksum, file_name, collection, title, authors, description,
                publisher, published_date, language, isbn, format, page_count,
                thumbnail, metadata, search_phrase, state
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(collection, file_name) DO UPDATE SET
                checksum = excluded.checksum,
                title = excluded.title,
                authors = excluded.authors,
                description = excluded.description,
                publisher = excluded.publisher,
                published_date = excluded.published_date,
                language = excluded.language,
                isbn = excluded.isbn,
                format = excluded.format,
                page_count = excluded.page_count,
                thumbnail = excluded.thumbnail,
                metadata = excluded.metadata,
                search_phrase = excluded.search_phrase,
                state = excluded.state,
                updated_on = CURRENT_TIMESTAMP
            ON CONFLICT(checksum) DO UPDATE SET
                file_name = excluded.file_name,
                collection = excluded.collection,
                title = excluded.title,
                authors = excluded.authors,
                description = excluded.description,
                publisher = excluded.publisher,
                published_date = excluded.published_date,
                language = excluded.language,
                isbn = excluded.isbn,
                format = excluded.format,
                page_count = excluded.page_count,
                thumbnail = excluded.thumbnail,
                metadata = excluded.metadata,
                search_phrase = excluded.search_phrase,
                state = excluded.state,
                updated_on = CURRENT_TIMESTAMP
            "#,
        )
        .bind(details.checksum)
        .bind(&details.file_name)
        .bind(&details.collection)
        .bind(&details.title)
        .bind(authors)
        .bind(&details.description)
        .bind(&details.publisher)
        .bind(&details.published_date)
        .bind(&details.language)
        .bind(&details.isbn)
        .bind(format)
        .bind(details.page_count)
        .bind(&details.thumbnail)
        .bind(metadata)
        .bind(&details.search_phrase)
        .bind(details.state as i32)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        if let Some(sender) = &self.sender {
            let message = if is_update {
                LocalMessage::Book(BookEvent::new_book_changed_event(details.clone()))
            } else {
                LocalMessage::Book(BookEvent::new_book_added_event(details.clone()))
            };

            if let Err(error) = sender.send(message).await {
                tracing::error!("Error sending book event {}", error);
            }
        }

        Ok(details.checksum)
    }

    async fn retrieve_book(&self, checksum: i64) -> Result<BookDetails, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM books WHERE checksum = ?")
            .bind(checksum)
            .fetch_one(&self.pool)
            .await?;

        Ok(Self::book_from_record(&row))
    }

    async fn list_books(&self, collection: &str) -> Result<Vec<BookDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM books
            WHERE collection = ?
            ORDER BY title, file_name
            "#,
        )
        .bind(collection)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::book_from_record).collect())
    }

    async fn list_book_collections(
        &self,
        parent_collection: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        let rows = if parent_collection.is_empty() {
            sqlx::query(
                r#"
                SELECT DISTINCT collection
                FROM books
                WHERE collection <> ''
                "#,
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT DISTINCT collection
                FROM books
                WHERE collection LIKE ?
                "#,
            )
            .bind(format!("{}%", parent_collection))
            .fetch_all(&self.pool)
            .await?
        };
        let part = if parent_collection.is_empty() {
            0
        } else {
            parent_collection.matches('/').count() + 1
        };

        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("collection"))
            .filter_map(|collection| collection.split('/').nth(part).map(str::to_string))
            .unique()
            .sorted()
            .collect())
    }

    async fn list_all_books(&self) -> Result<Vec<BookDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM books
            ORDER BY collection, title, file_name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::book_from_record).collect())
    }

    async fn delete_book(&self, checksum: i64) -> Result<u64, sqlx::Error> {
        let rows_affected = sqlx::query("DELETE FROM books WHERE checksum = ?")
            .bind(checksum)
            .execute(&self.pool)
            .await?
            .rows_affected();

        if rows_affected > 0 {
            if let Some(sender) = &self.sender {
                let message = LocalMessage::Book(BookEvent::new_book_deleted_event(checksum));
                if let Err(error) = sender.send(message).await {
                    tracing::error!("Error sending book deleted event {} {}", checksum, error);
                }
            }
        }

        Ok(rows_affected)
    }

    async fn save_video(&self, details: &VideoDetails) -> Result<i64, sqlx::Error> {
        // Use a transaction so the existence check and upsert are atomic
        let mut tx = self.pool.begin().await?;

        let existing = sqlx::query("SELECT checksum FROM video_details WHERE checksum = ?")
            .bind(details.checksum)
            .fetch_optional(&mut *tx)
            .await?;
        let is_update = existing.is_some();

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
            .execute(&mut *tx)
            .await;

        match result {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Error saving video: {}", e);
                return Err(e);
            }
        };

        tx.commit().await?;

        if let Some(sender) = &self.sender {
            let message = if is_update {
                LocalMessage::Video(VideoEvent::new_video_changed_event(details.clone()))
            } else {
                LocalMessage::Video(VideoEvent::new_video_added_event(details.clone()))
            };

            if let Err(e) = sender.send(message).await {
                tracing::error!("Error sending video event {}", e);
            }
        }

        Ok(details.checksum)
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
                thumbnail: get_thumbnails_url(&thumbnails),
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
        "#;

        sqlx::query(query)
            .bind(checksum)
            .bind(current_time)
            .bind(current_time)
            .bind(current_time)
            .bind(current_time)
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

    async fn list_all_videos(&self) -> Result<Vec<VideoDetails>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            SELECT
                vd.*,
                h.updated_on as last_viewed,
                h.stopped as play_from
            FROM
                video_details vd
                LEFT JOIN history h ON vd.checksum = h.checksum
            ORDER BY
                collection, video
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            results.push(Self::from_record_with_last_seen(&row));
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Local;
    use serde_json::json;

    use crate::domain::messages::BookEventType;
    use crate::domain::models::{BookDetails, BookFormat, BookMetadata, BookState, VideoState};
    use tokio::sync::mpsc;

    use super::*;

    fn sample_book(checksum: i64, collection: &str, file_name: &str, title: &str) -> BookDetails {
        let now = Local::now().naive_local();

        BookDetails {
            file_name: file_name.to_string(),
            collection: collection.to_string(),
            title: title.to_string(),
            authors: vec!["Ursula K. Le Guin".to_string(), "Translator".to_string()],
            description: Some("A classic novel".to_string()),
            publisher: Some("Ace".to_string()),
            published_date: Some("1969".to_string()),
            language: Some("en".to_string()),
            isbn: Some("9780441478125".to_string()),
            format: BookFormat::Epub,
            page_count: Some(304),
            thumbnail: "left-hand-of-darkness.jpg".to_string(),
            metadata: BookMetadata {
                raw: Some(json!({
                    "subjects": ["science fiction", "gender"],
                    "source": {"kind": "epub"}
                })),
                extraction_error: Some("cover missing".to_string()),
            },
            checksum,
            search_phrase: Some("left hand darkness".to_string()),
            state: BookState::Ready,
            created_on: now,
            updated_on: now,
            dir_path: None,
        }
    }

    #[tokio::test]
    async fn migrations_create_books_table_and_indexes() {
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();

        let rows = sqlx::query(
            r#"
            SELECT name
            FROM sqlite_master
            WHERE type IN ('table', 'index')
              AND name IN (
                'books',
                'idx_books_collection_file',
                'idx_books_title',
                'idx_books_authors'
              )
            ORDER BY name
            "#,
        )
        .fetch_all(&db.pool)
        .await
        .unwrap();
        let names = rows
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "books",
                "idx_books_authors",
                "idx_books_collection_file",
                "idx_books_title",
            ]
        );
    }

    #[tokio::test]
    async fn save_and_retrieve_book_round_trips_all_fields_and_json() {
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
        let book = sample_book(
            101,
            "Science Fiction",
            "The Left Hand of Darkness.epub",
            "The Left Hand of Darkness",
        );

        assert_eq!(db.save_book(&book).await.unwrap(), book.checksum);

        let retrieved = db.retrieve_book(book.checksum).await.unwrap();
        let mut expected = book;
        expected.created_on = retrieved.created_on;
        expected.updated_on = retrieved.updated_on;

        assert_eq!(retrieved, expected);
    }

    #[tokio::test]
    async fn save_book_updates_on_collection_and_file_conflict() {
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
        let original = sample_book(201, "Fantasy", "Earthsea.epub", "A Wizard of Earthsea");
        db.save_book(&original).await.unwrap();
        let mut replacement = sample_book(202, "Fantasy", "Earthsea.epub", "Earthsea");
        replacement.authors = vec!["Ursula Le Guin".to_string()];

        assert_eq!(
            db.save_book(&replacement).await.unwrap(),
            replacement.checksum
        );

        assert!(matches!(
            db.retrieve_book(original.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
        let retrieved = db.retrieve_book(replacement.checksum).await.unwrap();
        assert_eq!(retrieved.title, replacement.title);
        assert_eq!(retrieved.authors, replacement.authors);
    }

    #[tokio::test]
    async fn save_book_updates_on_checksum_conflict() {
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
        let original = sample_book(301, "Fantasy", "Earthsea.epub", "Earthsea");
        db.save_book(&original).await.unwrap();
        let replacement = sample_book(
            original.checksum,
            "Classics/Fantasy",
            "A Wizard of Earthsea.epub",
            "A Wizard of Earthsea",
        );

        assert_eq!(
            db.save_book(&replacement).await.unwrap(),
            replacement.checksum
        );

        let retrieved = db.retrieve_book(original.checksum).await.unwrap();
        assert_eq!(retrieved.collection, replacement.collection);
        assert_eq!(retrieved.file_name, replacement.file_name);
        assert_eq!(retrieved.title, replacement.title);
    }

    #[tokio::test]
    async fn save_book_emits_added_then_changed_events() {
        let (sender, mut receiver) = mpsc::channel(2);
        let db = SqlRepository::new(MEMORY_DB_URL, Some(sender))
            .await
            .unwrap();
        let original = sample_book(401, "Fantasy", "Earthsea.epub", "Earthsea");

        db.save_book(&original).await.unwrap();

        let added = receiver.try_recv().unwrap();
        let LocalMessage::Book(added) = added else {
            panic!("expected a book event");
        };
        assert_eq!(added.event_type, BookEventType::BookEventAdded);
        assert_eq!(added.checksum, original.checksum.to_string());
        assert_eq!(added.book.unwrap().title, original.title);

        let replacement = sample_book(
            402,
            &original.collection,
            &original.file_name,
            "A Wizard of Earthsea",
        );
        db.save_book(&replacement).await.unwrap();

        let changed = receiver.try_recv().unwrap();
        let LocalMessage::Book(changed) = changed else {
            panic!("expected a book event");
        };
        assert_eq!(changed.event_type, BookEventType::BookEventChanged);
        assert_eq!(changed.checksum, replacement.checksum.to_string());
        assert_eq!(changed.book.unwrap().title, replacement.title);
    }

    #[tokio::test]
    async fn list_books_filters_collection_and_orders_by_title_then_file() {
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
        for book in [
            sample_book(501, "Essays", "Zulu.epub", "Collected Essays"),
            sample_book(502, "Essays", "Alpha.epub", "Collected Essays"),
            sample_book(503, "Essays", "Beginning.epub", "A Beginning"),
            sample_book(504, "Other", "Ignored.epub", "Ignored"),
        ] {
            db.save_book(&book).await.unwrap();
        }

        let books = db.list_books("Essays").await.unwrap();
        let files = books
            .into_iter()
            .map(|book| book.file_name)
            .collect::<Vec<_>>();

        assert_eq!(
            files,
            vec!["Beginning.epub", "Alpha.epub", "Zulu.epub"]
        );
    }

    #[tokio::test]
    async fn list_book_collections_returns_sorted_nested_collection_names() {
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
        for (checksum, collection) in [
            (601, "Fantasy"),
            (602, "Fiction/Fantasy"),
            (603, "Fiction/Classics"),
            (604, "Fiction/Fantasy/Epic"),
            (605, "Nonfiction/Essay"),
            (606, ""),
        ] {
            let book = sample_book(
                checksum,
                collection,
                &format!("{checksum}.epub"),
                "Title",
            );
            db.save_book(&book).await.unwrap();
        }

        assert_eq!(
            db.list_book_collections("").await.unwrap(),
            vec!["Fantasy", "Fiction", "Nonfiction"]
        );
        assert_eq!(
            db.list_book_collections("Fiction").await.unwrap(),
            vec!["Classics", "Fantasy"]
        );
        assert_eq!(
            db.list_book_collections("Fiction/Fantasy")
                .await
                .unwrap(),
            vec!["Epic"]
        );
    }

    #[tokio::test]
    async fn list_all_books_orders_by_collection_title_and_file() {
        let db = SqlRepository::new(MEMORY_DB_URL, None).await.unwrap();
        for book in [
            sample_book(701, "B", "Zulu.epub", "Same"),
            sample_book(702, "A", "Zulu.epub", "Same"),
            sample_book(703, "A", "Alpha.epub", "Same"),
            sample_book(704, "A", "Last.epub", "Zed"),
            sample_book(705, "A", "First.epub", "Able"),
        ] {
            db.save_book(&book).await.unwrap();
        }

        let ordered = db
            .list_all_books()
            .await
            .unwrap()
            .into_iter()
            .map(|book| (book.collection, book.file_name))
            .collect::<Vec<_>>();

        assert_eq!(
            ordered,
            vec![
                ("A".to_string(), "First.epub".to_string()),
                ("A".to_string(), "Alpha.epub".to_string()),
                ("A".to_string(), "Zulu.epub".to_string()),
                ("A".to_string(), "Last.epub".to_string()),
                ("B".to_string(), "Zulu.epub".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn delete_book_removes_row_and_emits_only_for_deleted_rows() {
        let (sender, mut receiver) = mpsc::channel(3);
        let db = SqlRepository::new(MEMORY_DB_URL, Some(sender))
            .await
            .unwrap();
        let book = sample_book(801, "Classics", "Dune.epub", "Dune");
        db.save_book(&book).await.unwrap();
        receiver.try_recv().unwrap();

        assert_eq!(db.delete_book(book.checksum).await.unwrap(), 1);
        assert!(matches!(
            db.retrieve_book(book.checksum).await,
            Err(sqlx::Error::RowNotFound)
        ));
        let deleted = receiver.try_recv().unwrap();
        let LocalMessage::Book(deleted) = deleted else {
            panic!("expected a book event");
        };
        assert_eq!(deleted.event_type, BookEventType::BookEventDeleted);
        assert_eq!(deleted.checksum, book.checksum.to_string());
        assert!(deleted.book.is_none());

        assert_eq!(db.delete_book(book.checksum).await.unwrap(), 0);
        assert!(receiver.try_recv().is_err());
    }

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
                audio_track_list: None,
                subtitle_tracks: None,
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
