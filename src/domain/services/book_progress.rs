use crate::domain::{
    models::{BookLocator, BookLocatorType, BookProgress, SaveBookProgressRequest},
    traits::{
        DeleteBookProgressOutcome, GetBookProgressOutcome, Repository, SaveBookProgressOutcome,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum BookProgressError {
    #[error("invalid book checksum")]
    InvalidChecksum,
    #[error("book not found")]
    BookNotFound,
    #[error("invalid book locator type")]
    InvalidLocatorType,
    #[error("book locator value must not be blank")]
    BlankLocatorValue,
    #[error("book progression must be finite and between 0 and 1")]
    InvalidProgression,
    #[error("book progress repository failure")]
    Repository(#[source] sqlx::Error),
}

pub struct BookProgressService {
    repository: Repository,
}

#[derive(Debug)]
struct ValidatedBookProgressRequest {
    locator: BookLocator,
    progression: Option<f64>,
}

impl BookProgressService {
    pub fn new(repository: Repository) -> Self {
        Self { repository }
    }

    pub fn validate_checksum(&self, checksum: &str) -> Result<i64, BookProgressError> {
        checksum
            .parse()
            .map_err(|_| BookProgressError::InvalidChecksum)
    }

    fn validate_request(
        &self,
        request: SaveBookProgressRequest,
    ) -> Result<ValidatedBookProgressRequest, BookProgressError> {
        let locator_type = match request.locator.locator_type.as_str() {
            "epub-cfi" => BookLocatorType::EpubCfi,
            "pdf-page" => BookLocatorType::PdfPage,
            _ => return Err(BookProgressError::InvalidLocatorType),
        };
        if request.locator.value.trim().is_empty() {
            return Err(BookProgressError::BlankLocatorValue);
        }
        if request.progression.is_some_and(|progression| {
            !progression.is_finite() || !(0.0..=1.0).contains(&progression)
        }) {
            return Err(BookProgressError::InvalidProgression);
        }

        Ok(ValidatedBookProgressRequest {
            locator: BookLocator {
                locator_type,
                value: request.locator.value,
            },
            progression: request.progression,
        })
    }

    pub async fn list(&self) -> Result<Vec<BookProgress>, BookProgressError> {
        self.repository
            .list_book_progress()
            .await
            .map_err(BookProgressError::Repository)
    }

    pub async fn get(&self, checksum: &str) -> Result<Option<BookProgress>, BookProgressError> {
        let checksum = self.validate_checksum(checksum)?;
        match self
            .repository
            .get_book_progress(checksum)
            .await
            .map_err(BookProgressError::Repository)?
        {
            GetBookProgressOutcome::BookNotFound => Err(BookProgressError::BookNotFound),
            GetBookProgressOutcome::NoProgress => Ok(None),
            GetBookProgressOutcome::Progress(progress) => Ok(Some(progress)),
        }
    }

    pub async fn save(
        &self,
        checksum: &str,
        request: SaveBookProgressRequest,
    ) -> Result<BookProgress, BookProgressError> {
        let checksum = self.validate_checksum(checksum)?;
        let request = self.validate_request(request)?;
        match self
            .repository
            .save_book_progress(checksum, &request.locator, request.progression)
            .await
            .map_err(BookProgressError::Repository)?
        {
            SaveBookProgressOutcome::BookNotFound => Err(BookProgressError::BookNotFound),
            SaveBookProgressOutcome::Saved(progress) => Ok(progress),
        }
    }

    pub async fn delete(&self, checksum: &str) -> Result<(), BookProgressError> {
        let checksum = self.validate_checksum(checksum)?;
        match self
            .repository
            .delete_book_progress(checksum)
            .await
            .map_err(BookProgressError::Repository)?
        {
            DeleteBookProgressOutcome::BookNotFound => Err(BookProgressError::BookNotFound),
            DeleteBookProgressOutcome::Deleted => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::BookProgressService;
    use crate::domain::{
        models::{
            BookLocator, BookLocatorType, BookProgress, RawBookLocator, SaveBookProgressRequest,
        },
        traits::{
            Databaser, DeleteBookProgressOutcome, GetBookProgressOutcome, Repository,
            SaveBookProgressOutcome,
        },
    };

    fn raw(locator_type: &str, value: &str, progression: Option<f64>) -> SaveBookProgressRequest {
        SaveBookProgressRequest {
            locator: RawBookLocator {
                locator_type: locator_type.into(),
                value: value.into(),
            },
            progression,
        }
    }

    fn progress(checksum: i64) -> BookProgress {
        BookProgress {
            checksum,
            locator: BookLocator {
                locator_type: BookLocatorType::EpubCfi,
                value: "epubcfi(/6/4!/4/2/8)".into(),
            },
            progression: Some(0.42),
            updated_on: "2026-07-19T12:00:00.000Z".into(),
        }
    }

    struct FakeRepository;

    #[async_trait::async_trait]
    impl Databaser for FakeRepository {
        async fn save_book(
            &self,
            _details: &crate::domain::models::BookDetails,
        ) -> Result<i64, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_book_collections(
            &self,
            _collection: &str,
        ) -> Result<Vec<String>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_books(
            &self,
            _collection: &str,
        ) -> Result<Vec<crate::domain::models::BookDetails>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_all_books(
            &self,
        ) -> Result<Vec<crate::domain::models::BookDetails>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn retrieve_book(
            &self,
            _checksum: i64,
        ) -> Result<crate::domain::models::BookDetails, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn delete_book(&self, _checksum: i64) -> Result<u64, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn delete_book_if_path_matches(
            &self,
            _checksum: i64,
            _collection: &str,
            _file_name: &str,
        ) -> Result<u64, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn save_video(
            &self,
            _details: &crate::domain::models::VideoDetails,
        ) -> Result<i64, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_collection(&self, _collection: &str) -> Result<Vec<String>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_videos(
            &self,
            _collection: &str,
        ) -> Result<Vec<crate::domain::models::VideoDetails>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_all_series(
            &self,
        ) -> Result<Vec<crate::domain::models::CollectionItem>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_series_details(
            &self,
            _series: &str,
            _season: Option<&str>,
        ) -> Result<Vec<crate::domain::models::VideoDetails>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn retrieve_video(
            &self,
            _checksum: i64,
        ) -> Result<crate::domain::models::VideoDetails, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn delete_video(&self, _checksum: i64) -> Result<u64, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn update_watched_video(
            &self,
            _checksum: i64,
            _current_time: f64,
        ) -> Result<(), sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn get_history(
            &self,
            _offset: i32,
            _limit: i32,
        ) -> Result<Vec<crate::domain::models::VideoDetails>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_all_videos(
            &self,
        ) -> Result<Vec<crate::domain::models::VideoDetails>, sqlx::Error> {
            unreachable!("not used by book progress service tests")
        }

        async fn list_book_progress(&self) -> Result<Vec<BookProgress>, sqlx::Error> {
            Ok(vec![progress(2), progress(3)])
        }

        async fn get_book_progress(
            &self,
            checksum: i64,
        ) -> Result<GetBookProgressOutcome, sqlx::Error> {
            Ok(match checksum {
                404 => GetBookProgressOutcome::BookNotFound,
                0 => GetBookProgressOutcome::NoProgress,
                checksum => GetBookProgressOutcome::Progress(progress(checksum)),
            })
        }

        async fn save_book_progress(
            &self,
            checksum: i64,
            progress: &BookLocator,
            progression: Option<f64>,
        ) -> Result<SaveBookProgressOutcome, sqlx::Error> {
            if checksum == 404 {
                return Ok(SaveBookProgressOutcome::BookNotFound);
            }

            Ok(SaveBookProgressOutcome::Saved(BookProgress {
                checksum,
                locator: progress.clone(),
                progression,
                updated_on: "2026-07-19T12:00:00.000Z".into(),
            }))
        }

        async fn delete_book_progress(
            &self,
            checksum: i64,
        ) -> Result<DeleteBookProgressOutcome, sqlx::Error> {
            Ok(if checksum == 404 {
                DeleteBookProgressOutcome::BookNotFound
            } else {
                DeleteBookProgressOutcome::Deleted
            })
        }
    }

    fn service() -> BookProgressService {
        let repository: Repository = Arc::new(FakeRepository);
        BookProgressService::new(repository)
    }

    #[test]
    fn validation_rejects_invalid_checksums() {
        let service = service();

        for checksum in ["", "book", "9223372036854775808"] {
            assert_eq!(
                service.validate_checksum(checksum).unwrap_err().to_string(),
                "invalid book checksum"
            );
        }
    }

    #[test]
    fn validation_rejects_unknown_locator_type_and_blank_value() {
        let service = service();

        assert_eq!(
            service
                .validate_request(raw("future", "x", None))
                .unwrap_err()
                .to_string(),
            "invalid book locator type"
        );
        assert_eq!(
            service
                .validate_request(raw("epub-cfi", "  \t", None))
                .unwrap_err()
                .to_string(),
            "book locator value must not be blank"
        );
    }

    #[test]
    fn validation_rejects_non_finite_and_out_of_range_progression() {
        let service = service();

        for progression in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
            assert_eq!(
                service
                    .validate_request(raw("epub-cfi", "epubcfi(/6/4)", Some(progression)))
                    .unwrap_err()
                    .to_string(),
                "book progression must be finite and between 0 and 1"
            );
        }
    }

    #[test]
    fn validation_accepts_boundary_and_opaque_locators() {
        let service = service();

        for progression in [Some(0.0), Some(1.0), None] {
            assert!(service
                .validate_request(raw("epub-cfi", "epubcfi(/6/4!/4/2/8)", progression))
                .is_ok());
        }

        let request = service
            .validate_request(raw("pdf-page", "chapter-a", Some(1.0)))
            .unwrap();
        assert_eq!(request.locator.value, "chapter-a");
        assert_eq!(request.locator.locator_type, BookLocatorType::PdfPage);
    }

    #[tokio::test]
    async fn service_maps_repository_outcomes_without_preflight_queries() {
        let service = service();

        assert_eq!(service.list().await.unwrap(), vec![progress(2), progress(3)]);
        assert_eq!(service.get("0").await.unwrap(), None);
        assert_eq!(service.get("7").await.unwrap(), Some(progress(7)));
        assert_eq!(
            service.get("404").await.unwrap_err().to_string(),
            "book not found"
        );

        let saved = service
            .save("7", raw("pdf-page", "chapter-a", Some(1.0)))
            .await
            .unwrap();
        assert_eq!(saved.locator.locator_type, BookLocatorType::PdfPage);
        assert_eq!(saved.locator.value, "chapter-a");
        assert_eq!(saved.progression, Some(1.0));
        assert_eq!(
            service
                .save("404", raw("epub-cfi", "epubcfi(/6/4)", None))
                .await
                .unwrap_err()
                .to_string(),
            "book not found"
        );

        service.delete("7").await.unwrap();
        assert_eq!(
            service.delete("404").await.unwrap_err().to_string(),
            "book not found"
        );
    }
}
