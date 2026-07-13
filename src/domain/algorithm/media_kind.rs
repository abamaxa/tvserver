use std::path::Path;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "flv", "wmv", "webm", "m4v", "mpg", "mpeg", "3gp", "3g2", "ts",
    "vob", "m2ts", "mts", "f4v", "f4p", "f4a", "f4b", "ogv", "ogg", "drc", "gif", "gifv", "mng",
    "qt", "yuv", "rm", "rmvb", "asf", "amv", "m4p", "mp2", "mpe", "mpv", "m2v", "svi", "mxf",
    "roq", "nsv",
];

const BOOK_EXTENSIONS: &[&str] = &["pdf", "epub"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Book,
    Unsupported,
}

pub fn classify_media_kind(path: impl AsRef<Path>) -> MediaKind {
    let path = path.as_ref();
    if is_hidden(path) || is_temporary_video(path) {
        return MediaKind::Unsupported;
    }

    let Some(extension) = normalized_extension(path) else {
        return MediaKind::Unsupported;
    };

    if BOOK_EXTENSIONS.contains(&extension.as_str()) {
        MediaKind::Book
    } else if is_video_extension(&extension) {
        MediaKind::Video
    } else {
        MediaKind::Unsupported
    }
}

pub(crate) fn is_video_extension(extension: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

fn is_temporary_video(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".tmp.mp4"))
}

#[cfg(test)]
mod tests {
    use super::{classify_media_kind, MediaKind};

    #[test]
    fn classifies_pdf_and_epub_as_books_case_insensitively() {
        assert_eq!(classify_media_kind("novel.pdf"), MediaKind::Book);
        assert_eq!(classify_media_kind("NOVEL.PDF"), MediaKind::Book);
        assert_eq!(classify_media_kind("novel.epub"), MediaKind::Book);
        assert_eq!(classify_media_kind("NOVEL.EPUB"), MediaKind::Book);
    }

    #[test]
    fn classifies_existing_video_extensions_as_video() {
        for file_name in ["clip.mp4", "clip.MKV", "clip.avi", "clip.webm", "clip.mov"] {
            assert_eq!(classify_media_kind(file_name), MediaKind::Video);
        }
    }

    #[test]
    fn classifies_hidden_and_unrelated_files_as_unsupported() {
        for file_name in [".hidden.mp4", "cover.jpg", "notes.txt", "README", ".epub"] {
            assert_eq!(classify_media_kind(file_name), MediaKind::Unsupported);
        }
    }
}
