use std::collections::BTreeSet;

use async_trait::async_trait;
use sqlx::{Error, Row, Sqlite, Transaction};

use super::SqlRepository;
use crate::domain::{
    models::{PlaylistDetails, PlaylistSummary},
    traits::PlaylistDatabaser,
};

fn playlist_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(Error::Protocol(
            "Use a playlist name between 1 and 100 characters".into(),
        ));
    }
    Ok(name)
}

// Take the write lock before reading item positions so concurrent additions cannot
// choose the same position, and a stale reorder cannot silently lose another edit.
async fn lock_playlist(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<(), Error> {
    let result = sqlx::query("UPDATE playlists SET updated_on = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(Error::RowNotFound);
    }
    Ok(())
}

#[async_trait]
impl PlaylistDatabaser for SqlRepository {
    async fn list_playlists(&self) -> Result<Vec<PlaylistSummary>, Error> {
        let rows = sqlx::query(
            "SELECT p.id, p.name, COUNT(i.video_checksum) AS item_count
             FROM playlists p LEFT JOIN playlist_items i ON i.playlist_id = p.id
             GROUP BY p.id ORDER BY p.name COLLATE NOCASE, p.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| PlaylistSummary {
                id: row.get("id"),
                name: row.get("name"),
                item_count: row.get("item_count"),
            })
            .collect())
    }

    async fn retrieve_playlist(&self, id: &str) -> Result<PlaylistDetails, Error> {
        let mut tx = self.pool.begin().await?;
        let name = sqlx::query_scalar("SELECT name FROM playlists WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        let rows = sqlx::query(
            "SELECT vd.*, h.updated_on AS last_viewed, h.stopped AS play_from
             FROM playlist_items i JOIN video_details vd ON vd.checksum = i.video_checksum
             LEFT JOIN history h ON h.checksum = vd.checksum
             WHERE i.playlist_id = ? ORDER BY i.position, i.video_checksum",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(PlaylistDetails {
            id: id.into(),
            name,
            videos: rows.iter().map(Self::from_record_with_last_seen).collect(),
        })
    }

    async fn create_playlist(&self, name: &str) -> Result<PlaylistDetails, Error> {
        let name = playlist_name(name)?.to_owned();
        let id = format!("{:032x}", rand::random::<u128>());
        sqlx::query("INSERT INTO playlists (id, name) VALUES (?, ?)")
            .bind(&id)
            .bind(&name)
            .execute(&self.pool)
            .await?;
        Ok(PlaylistDetails {
            id,
            name,
            videos: Vec::new(),
        })
    }

    async fn rename_playlist(&self, id: &str, name: &str) -> Result<(), Error> {
        let name = playlist_name(name)?;
        let result = sqlx::query(
            "UPDATE playlists SET name = ?, updated_on = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(name)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(Error::RowNotFound);
        }
        Ok(())
    }

    async fn delete_playlist(&self, id: &str) -> Result<(), Error> {
        let result = sqlx::query("DELETE FROM playlists WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(Error::RowNotFound);
        }
        Ok(())
    }

    async fn add_playlist_video(&self, id: &str, checksum: i64) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        lock_playlist(&mut tx, id).await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_details WHERE checksum = ?)")
                .bind(checksum)
                .fetch_one(&mut *tx)
                .await?;
        if !exists {
            return Err(Error::RowNotFound);
        }
        sqlx::query(
            "INSERT INTO playlist_items (playlist_id, video_checksum, position)
             SELECT ?, ?, COALESCE(MAX(position) + 1, 0) FROM playlist_items WHERE playlist_id = ?
             ON CONFLICT(playlist_id, video_checksum) DO NOTHING",
        )
        .bind(id)
        .bind(checksum)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    async fn remove_playlist_video(&self, id: &str, checksum: i64) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        lock_playlist(&mut tx, id).await?;
        sqlx::query("DELETE FROM playlist_items WHERE playlist_id = ? AND video_checksum = ?")
            .bind(id)
            .bind(checksum)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    async fn reorder_playlist(&self, id: &str, checksums: &[i64]) -> Result<(), Error> {
        let mut tx = self.pool.begin().await?;
        lock_playlist(&mut tx, id).await?;
        let existing: Vec<i64> =
            sqlx::query_scalar("SELECT video_checksum FROM playlist_items WHERE playlist_id = ?")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        let requested: BTreeSet<_> = checksums.iter().copied().collect();
        if requested.len() != checksums.len() || requested != existing.into_iter().collect() {
            return Err(Error::Protocol(
                "The playlist changed. Refresh it and try again".into(),
            ));
        }
        for (position, checksum) in checksums.iter().enumerate() {
            sqlx::query("UPDATE playlist_items SET position = ? WHERE playlist_id = ? AND video_checksum = ?")
                .bind(position as i64).bind(id).bind(checksum).execute(&mut *tx).await?;
        }
        tx.commit().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{models::VideoDetails, traits::Databaser};

    async fn with_videos(db: &SqlRepository) -> String {
        for checksum in [i64::MIN, i64::MAX, 3] {
            db.save_video(&VideoDetails {
                checksum,
                video: format!("{checksum}.mp3"),
                ..Default::default()
            })
            .await
            .unwrap();
        }
        let playlist = db.create_playlist("  My favourites  ").await.unwrap();
        for checksum in [i64::MIN, i64::MAX, 3] {
            db.add_playlist_video(&playlist.id, checksum).await.unwrap();
        }
        playlist.id
    }

    async fn ids(db: &SqlRepository, id: &str) -> Vec<i64> {
        db.retrieve_playlist(id)
            .await
            .unwrap()
            .videos
            .iter()
            .map(|video| video.checksum)
            .collect()
    }

    #[tokio::test]
    async fn playlist_order_duplicates_and_full_width_ids_survive_reopening() {
        let file =
            std::env::temp_dir().join(format!("playlists-{:032x}.sqlite", rand::random::<u128>()));
        let url = format!("sqlite://{}", file.display());
        let db = SqlRepository::new(&url, None).await.unwrap();
        let id = with_videos(&db).await;
        db.add_playlist_video(&id, i64::MIN).await.unwrap();
        assert_eq!(ids(&db, &id).await, [i64::MIN, i64::MAX, 3]);
        db.reorder_playlist(&id, &[3, i64::MAX, i64::MIN])
            .await
            .unwrap();
        db.rename_playlist(&id, "Evening").await.unwrap();
        db.pool.close().await;
        let reopened = SqlRepository::new(&url, None).await.unwrap();
        assert_eq!(ids(&reopened, &id).await, [3, i64::MAX, i64::MIN]);
        let summaries = reopened.list_playlists().await.unwrap();
        assert_eq!(summaries[0].name, "Evening");
        assert_eq!(summaries[0].item_count, 3);
        let json = serde_json::to_value(reopened.retrieve_playlist(&id).await.unwrap()).unwrap();
        assert_eq!(json["videos"][1]["checksum"], i64::MAX.to_string());
        reopened.pool.close().await;
        std::fs::remove_file(file).unwrap();
    }

    #[tokio::test]
    async fn playlist_edits_validate_input_and_stale_reorders_are_atomic() {
        let db = SqlRepository::new(":memory:", None).await.unwrap();
        for name in ["".to_string(), "  \n ".to_string(), "a".repeat(101)] {
            assert!(db.create_playlist(&name).await.is_err());
        }
        let id = with_videos(&db).await;
        assert!(db.add_playlist_video(&id, 404).await.is_err());
        for order in [vec![3], vec![3, 3, i64::MIN], vec![3, i64::MIN, 404]] {
            assert!(db.reorder_playlist(&id, &order).await.is_err());
            assert_eq!(ids(&db, &id).await, [i64::MIN, i64::MAX, 3]);
        }
        assert!(db.rename_playlist(&id, " ").await.is_err());
        assert!(db.retrieve_playlist("missing").await.is_err());
        assert!(db.add_playlist_video("missing", 3).await.is_err());
        assert!(db.rename_playlist("missing", "Name").await.is_err());
        assert!(db.delete_playlist("missing").await.is_err());
    }

    #[tokio::test]
    async fn deleting_playlists_or_memberships_preserves_media_and_media_deletion_cascades() {
        let db = SqlRepository::new(":memory:", None).await.unwrap();
        let id = with_videos(&db).await;
        let other = db.create_playlist("Other").await.unwrap();
        db.add_playlist_video(&other.id, 3).await.unwrap();
        db.remove_playlist_video(&id, 3).await.unwrap();
        assert!(db.retrieve_video(3).await.is_ok());
        assert_eq!(ids(&db, &other.id).await, [3]);
        db.delete_video(i64::MIN).await.unwrap();
        assert_eq!(ids(&db, &id).await, [i64::MAX]);
        db.delete_playlist(&id).await.unwrap();
        assert!(db.retrieve_video(i64::MAX).await.is_ok());
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM playlist_items WHERE playlist_id = ?")
                .bind(&id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn playlist_references_follow_media_metadata_refreshes_and_checksum_changes() {
        let db = SqlRepository::new(":memory:", None).await.unwrap();
        let id = with_videos(&db).await;
        let mut video = db.retrieve_video(3).await.unwrap();
        video.description = "Updated metadata".into();
        db.save_video(&video).await.unwrap();
        assert_eq!(
            db.retrieve_playlist(&id).await.unwrap().videos[2].description,
            "Updated metadata"
        );
        video.checksum = 4;
        db.save_video(&video).await.unwrap();
        assert_eq!(ids(&db, &id).await, [i64::MIN, i64::MAX, 4]);
    }

    #[tokio::test]
    async fn concurrent_additions_get_distinct_positions() {
        let db = SqlRepository::new(":memory:", None).await.unwrap();
        let id = with_videos(&db).await;
        db.remove_playlist_video(&id, 3).await.unwrap();
        db.remove_playlist_video(&id, i64::MAX).await.unwrap();
        let (a, b) = tokio::join!(
            db.add_playlist_video(&id, 3),
            db.add_playlist_video(&id, i64::MAX)
        );
        a.unwrap();
        b.unwrap();
        let positions: Vec<i64> = sqlx::query_scalar(
            "SELECT position FROM playlist_items WHERE playlist_id = ? ORDER BY position",
        )
        .bind(id)
        .fetch_all(&db.pool)
        .await
        .unwrap();
        assert_eq!(positions, [0, 1, 2]);
    }
}
