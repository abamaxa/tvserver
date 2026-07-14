use crate::domain::models::BookDetails;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum BookEventType {
    BookEventAdded = 0,
    BookEventChanged = 1,
    BookEventDeleted = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookEvent {
    #[serde(rename = "type")]
    pub event_type: BookEventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book: Option<BookDetails>,
    pub checksum: String,
}

impl BookEvent {
    pub fn new_book_added_event(book: BookDetails) -> Self {
        Self {
            event_type: BookEventType::BookEventAdded,
            checksum: book.checksum.to_string(),
            book: Some(book),
        }
    }

    pub fn new_book_changed_event(book: BookDetails) -> Self {
        Self {
            event_type: BookEventType::BookEventChanged,
            checksum: book.checksum.to_string(),
            book: Some(book),
        }
    }

    pub fn new_book_deleted_event(checksum: i64) -> Self {
        Self {
            event_type: BookEventType::BookEventDeleted,
            book: None,
            checksum: checksum.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{BookDetails, BookFormat};

    fn sample_book(checksum: i64) -> BookDetails {
        BookDetails {
            file_name: "Dune.epub".to_string(),
            collection: "Science Fiction".to_string(),
            title: "Dune".to_string(),
            authors: vec!["Frank Herbert".to_string()],
            format: BookFormat::Epub,
            checksum,
            ..BookDetails::default()
        }
    }

    #[test]
    fn added_event_carries_string_checksum_and_book_payload() {
        let event = BookEvent::new_book_added_event(sample_book(123));

        assert_eq!(event.event_type, BookEventType::BookEventAdded);
        assert_eq!(event.checksum, "123");
        assert_eq!(event.book.unwrap().title, "Dune");
    }

    #[test]
    fn changed_event_carries_string_checksum_and_book_payload() {
        let event = BookEvent::new_book_changed_event(sample_book(456));

        assert_eq!(event.event_type, BookEventType::BookEventChanged);
        assert_eq!(event.checksum, "456");
        assert_eq!(event.book.unwrap().title, "Dune");
    }

    #[test]
    fn deleted_event_carries_string_checksum_without_book_payload() {
        let event = BookEvent::new_book_deleted_event(789);

        assert_eq!(event.event_type, BookEventType::BookEventDeleted);
        assert_eq!(event.checksum, "789");
        assert!(event.book.is_none());
    }
}
