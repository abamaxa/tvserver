use chrono::{Local, NaiveDateTime};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use crate::domain::algorithm::{
    get_book_download_path, get_book_thumbnail_file_name, get_book_thumbnail_url, get_book_url,
};

pub const DEFAULT_BOOK_THUMBNAIL: &str = "default-book.jpg";
static NEXT_DEFAULT_THUMBNAIL_TEMP: AtomicU64 = AtomicU64::new(0);
static DEFAULT_BOOK_THUMBNAIL_LOCK: Mutex<()> = Mutex::new(());

pub fn default_book_thumbnail_bytes() -> &'static [u8] {
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/book/default-book.jpg"
    ))
}

pub fn ensure_default_book_thumbnail<P: AsRef<Path>>(thumbnail_dir: P) -> io::Result<PathBuf> {
    fs::create_dir_all(&thumbnail_dir)?;
    let _guard = DEFAULT_BOOK_THUMBNAIL_LOCK.lock().map_err(|_| {
        io::Error::other("default book thumbnail materialization lock is poisoned")
    })?;
    let thumbnail_path = thumbnail_dir.as_ref().join(DEFAULT_BOOK_THUMBNAIL);
    if fs::symlink_metadata(&thumbnail_path).is_ok_and(|metadata| metadata.file_type().is_file())
        && fs::read(&thumbnail_path).is_ok_and(|bytes| bytes == default_book_thumbnail_bytes())
    {
        return Ok(thumbnail_path);
    }

    let temp_id = NEXT_DEFAULT_THUMBNAIL_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp_path = thumbnail_dir.as_ref().join(format!(
        ".{DEFAULT_BOOK_THUMBNAIL}.{}.{}.tmp",
        std::process::id(),
        temp_id
    ));
    let result: io::Result<()> = (|| {
        let mut temp = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        temp.write_all(default_book_thumbnail_bytes())?;
        temp.sync_all()?;
        drop(temp);
        match fs::symlink_metadata(&thumbnail_path) {
            Ok(_) => fs::remove_file(&thumbnail_path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(&temp_path, &thumbnail_path)?;
        #[cfg(unix)]
        File::open(thumbnail_dir.as_ref())?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result?;
    Ok(thumbnail_path)
}

pub fn is_default_book_thumbnail(thumbnail: &str) -> bool {
    Path::new(thumbnail)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .map(|file_name| file_name == DEFAULT_BOOK_THUMBNAIL)
        .unwrap_or(false)
}

#[derive(Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    #[default]
    Pdf,
    Epub,
}

#[derive(Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BookState {
    #[default]
    Ready = 0,
    NewFile = 1,
    NeedMetadata = 2,
    NeedThumbnail = 3,
    MetadataError = 4,
    ZeroFileSize = 5,
    Exception = 10,
}

fn book_state_from_int<T: Into<i64>>(value: T) -> BookState {
    match value.into() {
        0 => BookState::Ready,
        1 => BookState::NewFile,
        2 => BookState::NeedMetadata,
        3 => BookState::NeedThumbnail,
        4 => BookState::MetadataError,
        5 => BookState::ZeroFileSize,
        10 => BookState::Exception,
        _ => BookState::default(),
    }
}

impl From<i8> for BookState {
    fn from(value: i8) -> Self {
        book_state_from_int(value)
    }
}

impl From<i16> for BookState {
    fn from(value: i16) -> Self {
        book_state_from_int(value)
    }
}

impl From<i32> for BookState {
    fn from(value: i32) -> Self {
        book_state_from_int(value)
    }
}

impl From<i64> for BookState {
    fn from(value: i64) -> Self {
        book_state_from_int(value)
    }
}

#[serde_with::skip_serializing_none]
#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookMetadata {
    pub raw: Option<Value>,
    pub extraction_error: Option<String>,
}

impl BookMetadata {
    pub fn is_empty(&self) -> bool {
        self.raw.is_none() && self.extraction_error.is_none()
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookCollectionItem {
    pub collection: String,
    pub thumbnail: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BookCollectionDetails {
    pub collection: String,
    pub parent_collection: String,
    pub child_collections: Vec<BookCollectionItem>,
    pub books: Vec<BookDetails>,
    pub errors: Vec<String>,
}

impl BookCollectionDetails {
    pub fn new(
        collection: String,
        child_collections: Vec<BookCollectionItem>,
        books: Vec<BookDetails>,
    ) -> Self {
        Self {
            parent_collection: Self::parent_collection(&collection),
            collection,
            child_collections,
            books,
            errors: Vec::new(),
        }
    }

    pub fn error(error: String) -> Self {
        Self {
            errors: vec![error],
            ..Default::default()
        }
    }

    fn parent_collection(collection: &str) -> String {
        if let Some(pos) = collection.find('/') {
            collection[..pos].to_string()
        } else {
            String::new()
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Default, Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BookDetails {
    pub file_name: String,
    pub collection: String,
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_date: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub format: BookFormat,
    pub page_count: Option<i64>,
    pub thumbnail: String,
    pub metadata: BookMetadata,
    #[serde(deserialize_with = "deserialize_string_to_i64")]
    pub checksum: i64,
    pub search_phrase: Option<String>,
    pub state: BookState,
    pub created_on: NaiveDateTime,
    pub updated_on: NaiveDateTime,
    #[serde(skip)]
    pub dir_path: Option<PathBuf>,
}

fn deserialize_string_to_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let s = String::deserialize(deserializer)?;
    s.parse::<i64>().map_err(D::Error::custom)
}

impl BookDetails {
    pub fn new(file_name: String, collection: String, path: &PathBuf, format: BookFormat) -> Self {
        let now = Local::now().naive_local();
        let title = Path::new(&file_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&file_name)
            .replace(['.', '_'], " ");
        let dir_path = path.parent().map(|dir| dir.to_path_buf());

        Self {
            file_name,
            collection,
            title,
            authors: Vec::new(),
            format,
            thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            state: BookState::NewFile,
            created_on: now,
            updated_on: now,
            dir_path,
            ..Default::default()
        }
    }

    pub fn get_full_path(&self) -> PathBuf {
        let file_name = get_book_thumbnail_file_name(&self.file_name);
        if let Some(dir_path) = &self.dir_path {
            dir_path.join(file_name)
        } else if self.collection.is_empty() {
            Path::new(&crate::domain::config::get_book_dir()).join(file_name)
        } else {
            Path::new(&crate::domain::config::get_book_dir())
                .join(safe_book_relative_path(&self.collection, &self.file_name))
        }
    }

    pub fn get_download_path(&self) -> String {
        get_book_download_path(&self.collection, &self.file_name)
    }
}

fn safe_book_relative_path(collection: &str, file_name: &str) -> PathBuf {
    PathBuf::from(get_book_download_path(collection, file_name))
}

impl Serialize for BookDetails {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut field_count = 11;
        if self.description.is_some() {
            field_count += 1;
        }
        if self.publisher.is_some() {
            field_count += 1;
        }
        if self.published_date.is_some() {
            field_count += 1;
        }
        if self.language.is_some() {
            field_count += 1;
        }
        if self.isbn.is_some() {
            field_count += 1;
        }
        if self.page_count.is_some() {
            field_count += 1;
        }
        if !self.metadata.is_empty() {
            field_count += 1;
        }
        if self.search_phrase.is_some() {
            field_count += 1;
        }

        let mut state = serializer.serialize_struct("BookDetails", field_count)?;

        state.serialize_field("fileName", &self.file_name)?;
        state.serialize_field("collection", &self.collection)?;
        state.serialize_field("title", &self.title)?;
        state.serialize_field("authors", &self.authors)?;
        if let Some(ref description) = self.description {
            state.serialize_field("description", description)?;
        }
        if let Some(ref publisher) = self.publisher {
            state.serialize_field("publisher", publisher)?;
        }
        if let Some(ref published_date) = self.published_date {
            state.serialize_field("publishedDate", published_date)?;
        }
        if let Some(ref language) = self.language {
            state.serialize_field("language", language)?;
        }
        if let Some(ref isbn) = self.isbn {
            state.serialize_field("isbn", isbn)?;
        }
        state.serialize_field("format", &self.format)?;
        if let Some(ref page_count) = self.page_count {
            state.serialize_field("pageCount", page_count)?;
        }
        state.serialize_field("thumbnail", &get_book_thumbnail_url(&self.thumbnail))?;
        if !self.metadata.is_empty() {
            state.serialize_field("metadata", &self.metadata)?;
        }
        state.serialize_field("checksum", &self.checksum.to_string())?;
        if let Some(ref search_phrase) = self.search_phrase {
            state.serialize_field("searchPhrase", search_phrase)?;
        }
        state.serialize_field("state", &self.state)?;
        state.serialize_field("createdOn", &self.created_on)?;
        state.serialize_field("updatedOn", &self.updated_on)?;
        state.serialize_field("url", &get_book_url(&self.collection, &self.file_name))?;

        state.end()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::domain::algorithm::{get_book_thumbnail_url, get_book_url};
    use crate::domain::config::BOOK_DIR;
    use chrono::NaiveDate;
    use serde_json::json;
    use std::{env, path::PathBuf};

    fn fixed_time() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 7, 13)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    fn sample_book() -> BookDetails {
        BookDetails {
            file_name: "Clean Code.epub".to_string(),
            collection: "Programming".to_string(),
            title: "Clean Code".to_string(),
            authors: vec!["Robert C. Martin".to_string()],
            format: BookFormat::Epub,
            checksum: 123456789,
            thumbnail: DEFAULT_BOOK_THUMBNAIL.to_string(),
            state: BookState::Ready,
            created_on: fixed_time(),
            updated_on: fixed_time(),
            ..BookDetails::default()
        }
    }

    #[test]
    fn serializes_checksum_urls_and_required_book_fields() {
        std::env::set_var(BOOK_DIR, "/tmp/tvserver-books");
        std::env::set_var(
            crate::domain::config::BOOK_THUMBNAIL_DIR,
            "/tmp/tvserver-book-thumbnails",
        );
        let value = serde_json::to_value(sample_book()).unwrap();

        assert_eq!(value["title"], json!("Clean Code"));
        assert_eq!(value["authors"], json!(["Robert C. Martin"]));
        assert_eq!(value["format"], json!("epub"));
        assert_eq!(value["checksum"], json!("123456789"));
        #[cfg(feature = "webserver")]
        assert_eq!(
            value["url"],
            json!("/api/books/download/Programming/Clean%20Code.epub")
        );
        #[cfg(not(feature = "webserver"))]
        assert_eq!(
            value["url"],
            json!("/tmp/tvserver-books/Programming/Clean Code.epub")
        );
        #[cfg(feature = "webserver")]
        assert_eq!(
            value["thumbnail"],
            json!("/api/book-thumbnails/default-book.jpg")
        );
        #[cfg(not(feature = "webserver"))]
        assert_eq!(
            value["thumbnail"],
            json!("/tmp/tvserver-book-thumbnails/default-book.jpg")
        );
        assert_eq!(value["state"], json!("Ready"));
        assert!(value.get("createdOn").is_some());
        assert!(value.get("updatedOn").is_some());
    }

    #[test]
    fn omits_empty_optional_fields_from_serialized_json() {
        let value = serde_json::to_value(sample_book()).unwrap();

        assert!(value.get("description").is_none());
        assert!(value.get("publisher").is_none());
        assert!(value.get("publishedDate").is_none());
        assert!(value.get("language").is_none());
        assert!(value.get("isbn").is_none());
        assert!(value.get("pageCount").is_none());
        assert!(value.get("metadata").is_none());
        assert!(value.get("searchPhrase").is_none());
    }

    #[test]
    fn deserializes_checksum_string() {
        let book: BookDetails = serde_json::from_value(json!({
            "fileName": "Dune.pdf",
            "collection": "Science Fiction",
            "title": "Dune",
            "authors": ["Frank Herbert"],
            "format": "pdf",
            "thumbnail": "default-book.jpg",
            "checksum": "987654321",
            "state": "Ready",
            "createdOn": "2026-07-13T12:00:00",
            "updatedOn": "2026-07-13T12:00:00"
        }))
        .unwrap();

        assert_eq!(book.checksum, 987654321);
        assert_eq!(book.format, BookFormat::Pdf);
    }

    #[test]
    fn full_and_download_paths_are_relative_to_book_dir() {
        env::set_var(BOOK_DIR, "/tmp/tvserver-books");
        let mut book = sample_book();
        book.file_name = "Dune.pdf".to_string();
        book.collection = "Science Fiction".to_string();

        assert_eq!(
            book.get_full_path(),
            PathBuf::from("/tmp/tvserver-books/Science Fiction/Dune.pdf")
        );
        assert_eq!(book.get_download_path(), "Science Fiction/Dune.pdf");

        book.collection = String::new();
        assert_eq!(
            book.get_full_path(),
            PathBuf::from("/tmp/tvserver-books/Dune.pdf")
        );
        assert_eq!(book.get_download_path(), "Dune.pdf");
    }

    #[test]
    fn full_path_prefers_transient_source_directory() {
        env::set_var(BOOK_DIR, "/tmp/tvserver-books");
        let mut book = sample_book();
        book.dir_path = Some(PathBuf::from("/tmp/downloads"));

        assert_eq!(
            book.get_full_path(),
            PathBuf::from("/tmp/downloads/Clean Code.epub")
        );
    }

    #[test]
    fn path_helpers_keep_collection_and_file_name_relative_to_book_dir() {
        env::set_var(BOOK_DIR, "/tmp/tvserver-books");
        let mut book = sample_book();
        book.collection = "../Escaped/Shelf".to_string();
        book.file_name = "../Secret.pdf".to_string();

        assert_eq!(
            book.get_full_path(),
            PathBuf::from("/tmp/tvserver-books/Escaped/Shelf/Secret.pdf")
        );
        assert_eq!(book.get_download_path(), "Escaped/Shelf/Secret.pdf");

        book.collection = "/var/books".to_string();
        book.file_name = "/tmp/Dune.pdf".to_string();

        assert_eq!(
            book.get_full_path(),
            PathBuf::from("/tmp/tvserver-books/var/books/Dune.pdf")
        );
        assert_eq!(book.get_download_path(), "var/books/Dune.pdf");
    }

    #[test]
    fn default_thumbnail_contract_is_explicit_and_embedded() {
        assert_eq!(DEFAULT_BOOK_THUMBNAIL, "default-book.jpg");
        assert!(is_default_book_thumbnail(DEFAULT_BOOK_THUMBNAIL));
        assert!(!is_default_book_thumbnail("generated-cover.jpg"));
        #[cfg(feature = "webserver")]
        assert_eq!(
            get_book_thumbnail_url(DEFAULT_BOOK_THUMBNAIL),
            "/api/book-thumbnails/default-book.jpg"
        );
        #[cfg(not(feature = "webserver"))]
        {
            env::set_var(BOOK_DIR, "/tmp/tvserver-books");
            env::set_var(
                crate::domain::config::BOOK_THUMBNAIL_DIR,
                "/tmp/tvserver-book-thumbnails",
            );
            assert_eq!(
                get_book_thumbnail_url(DEFAULT_BOOK_THUMBNAIL),
                "/tmp/tvserver-book-thumbnails/default-book.jpg"
            );
        }

        let bytes = default_book_thumbnail_bytes();
        assert!(bytes.starts_with(&[0xff, 0xd8]));
        assert!(bytes.ends_with(&[0xff, 0xd9]));
    }

    #[test]
    fn default_thumbnail_can_be_materialized_for_runtime_serving() {
        let thumbnail_dir = std::env::temp_dir().join(format!(
            "tvserver-book-thumbnail-test-{}",
            std::process::id()
        ));
        if thumbnail_dir.exists() {
            std::fs::remove_dir_all(&thumbnail_dir).unwrap();
        }

        let thumbnail_path = ensure_default_book_thumbnail(&thumbnail_dir).unwrap();

        assert_eq!(thumbnail_path, thumbnail_dir.join(DEFAULT_BOOK_THUMBNAIL));
        assert_eq!(
            std::fs::read(&thumbnail_path).unwrap(),
            default_book_thumbnail_bytes()
        );
        std::fs::remove_dir_all(&thumbnail_dir).unwrap();
    }

    #[test]
    fn stale_default_thumbnail_is_replaced_with_embedded_jpeg() {
        let thumbnail_dir = std::env::temp_dir().join(format!(
            "tvserver-book-thumbnail-stale-test-{}",
            std::process::id()
        ));
        if thumbnail_dir.exists() {
            std::fs::remove_dir_all(&thumbnail_dir).unwrap();
        }
        std::fs::create_dir_all(&thumbnail_dir).unwrap();
        let thumbnail_path = thumbnail_dir.join(DEFAULT_BOOK_THUMBNAIL);
        std::fs::write(&thumbnail_path, b"stale").unwrap();

        ensure_default_book_thumbnail(&thumbnail_dir).unwrap();

        assert_eq!(
            std::fs::read(&thumbnail_path).unwrap(),
            default_book_thumbnail_bytes()
        );
        std::fs::remove_dir_all(&thumbnail_dir).unwrap();
    }

    #[test]
    fn existing_default_thumbnail_is_not_rewritten() {
        let thumbnail_dir = std::env::temp_dir().join(format!(
            "tvserver-book-thumbnail-idempotent-test-{}",
            std::process::id()
        ));
        if thumbnail_dir.exists() {
            std::fs::remove_dir_all(&thumbnail_dir).unwrap();
        }

        let thumbnail_path = ensure_default_book_thumbnail(&thumbnail_dir).unwrap();
        let mut permissions = std::fs::metadata(&thumbnail_path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&thumbnail_path, permissions).unwrap();

        let result = ensure_default_book_thumbnail(&thumbnail_dir);

        let mut permissions = std::fs::metadata(&thumbnail_path).unwrap().permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(&thumbnail_path, permissions).unwrap();
        assert_eq!(result.unwrap(), thumbnail_path);
        assert_eq!(
            std::fs::read(&thumbnail_path).unwrap(),
            default_book_thumbnail_bytes()
        );
        std::fs::remove_dir_all(&thumbnail_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn default_thumbnail_provisioning_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let thumbnail_dir = std::env::temp_dir().join(format!(
            "tvserver-book-thumbnail-symlink-test-{}",
            std::process::id()
        ));
        if thumbnail_dir.exists() {
            std::fs::remove_dir_all(&thumbnail_dir).unwrap();
        }
        std::fs::create_dir_all(&thumbnail_dir).unwrap();
        let outside = thumbnail_dir.with_extension("outside");
        std::fs::write(&outside, b"do not replace").unwrap();
        let thumbnail_path = thumbnail_dir.join(DEFAULT_BOOK_THUMBNAIL);
        symlink(&outside, &thumbnail_path).unwrap();

        ensure_default_book_thumbnail(&thumbnail_dir).unwrap();

        assert_eq!(std::fs::read(&outside).unwrap(), b"do not replace");
        assert!(!std::fs::symlink_metadata(&thumbnail_path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read(&thumbnail_path).unwrap(),
            default_book_thumbnail_bytes()
        );
        std::fs::remove_dir_all(&thumbnail_dir).unwrap();
        std::fs::remove_file(outside).unwrap();
    }

    #[test]
    fn book_url_helpers_build_download_routes() {
        env::set_var(BOOK_DIR, "/tmp/tvserver-books");
        #[cfg(feature = "webserver")]
        assert_eq!(
            get_book_url("Programming", "Clean Code.epub"),
            "/api/books/download/Programming/Clean%20Code.epub"
        );
        #[cfg(feature = "webserver")]
        assert_eq!(get_book_url("", "Dune.pdf"), "/api/books/download/Dune.pdf");
        #[cfg(not(feature = "webserver"))]
        assert_eq!(
            get_book_url("Programming", "Clean Code.epub"),
            "/tmp/tvserver-books/Programming/Clean Code.epub"
        );
        #[cfg(not(feature = "webserver"))]
        assert_eq!(get_book_url("", "Dune.pdf"), "/tmp/tvserver-books/Dune.pdf");
    }

    #[test]
    fn book_url_helpers_keep_paths_relative_to_book_roots() {
        env::set_var(BOOK_DIR, "/tmp/tvserver-books");
        env::set_var(
            crate::domain::config::BOOK_THUMBNAIL_DIR,
            "/tmp/tvserver-book-thumbnails",
        );

        #[cfg(feature = "webserver")]
        assert_eq!(
            get_book_url("../Escaped/Shelf", "../Secret.pdf"),
            "/api/books/download/Escaped/Shelf/Secret.pdf"
        );
        #[cfg(not(feature = "webserver"))]
        assert_eq!(
            get_book_url("../Escaped/Shelf", "../Secret.pdf"),
            "/tmp/tvserver-books/Escaped/Shelf/Secret.pdf"
        );

        #[cfg(feature = "webserver")]
        assert_eq!(
            get_book_thumbnail_url("../default-book.jpg"),
            "/api/book-thumbnails/default-book.jpg"
        );
        #[cfg(not(feature = "webserver"))]
        assert_eq!(
            get_book_thumbnail_url("../default-book.jpg"),
            "/tmp/tvserver-book-thumbnails/default-book.jpg"
        );
    }
}
