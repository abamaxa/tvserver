use std::{
    ffi::OsString,
    future::{ready, Future, Ready},
    io,
    path::{Component, Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::SystemTime,
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, OpenOptions};
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tower_http::services::fs::{Backend, File, Metadata};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StaticFilePolicy {
    BookDownload,
    BookThumbnail,
}

impl StaticFilePolicy {
    fn accepts(self, components: &[OsString]) -> bool {
        let Some(extension) = components
            .last()
            .and_then(|component| Path::new(component).extension())
            .and_then(|extension| extension.to_str())
        else {
            return false;
        };

        match self {
            Self::BookDownload => {
                extension.eq_ignore_ascii_case("epub") || extension.eq_ignore_ascii_case("pdf")
            }
            Self::BookThumbnail => components.len() == 1 && extension.eq_ignore_ascii_case("jpg"),
        }
    }
}

fn not_found(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, message)
}

fn hidden_path_error(_: io::Error) -> io::Error {
    not_found("static file not found")
}

fn normal_components(path: &Path, policy: StaticFilePolicy) -> io::Result<Vec<OsString>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir if components.is_empty() => {}
            Component::Normal(component) => components.push(component.to_os_string()),
            Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_)
            | Component::RootDir => {
                return Err(not_found("static file path must be relative"));
            }
        }
    }

    if components.is_empty() || !policy.accepts(&components) {
        return Err(not_found("static file path rejected by policy"));
    }
    Ok(components)
}

#[derive(Clone, Debug)]
pub(super) struct CapabilityBackend {
    root: Arc<Dir>,
    policy: StaticFilePolicy,
}

impl CapabilityBackend {
    pub(super) fn new(root: Arc<Dir>, policy: StaticFilePolicy) -> Self {
        Self { root, policy }
    }
}

#[derive(Clone, Debug)]
pub(super) struct CapabilityMetadata {
    len: u64,
    modified: SystemTime,
}

impl Metadata for CapabilityMetadata {
    fn is_dir(&self) -> bool {
        false
    }

    fn modified(&self) -> io::Result<SystemTime> {
        Ok(self.modified)
    }

    fn len(&self) -> u64 {
        self.len
    }
}

#[derive(Debug)]
pub(super) struct CapabilityFile {
    file: tokio::fs::File,
    metadata: CapabilityMetadata,
}

impl AsyncRead for CapabilityFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_read(cx, buffer)
    }
}

impl AsyncSeek for CapabilityFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.file).start_seek(position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Pin::new(&mut self.file).poll_complete(cx)
    }
}

impl File for CapabilityFile {
    type Metadata = CapabilityMetadata;
    type MetadataFuture<'a> = Ready<io::Result<Self::Metadata>>;

    fn metadata(&self) -> Self::MetadataFuture<'_> {
        ready(Ok(self.metadata.clone()))
    }
}

fn open_regular_file(
    root: &Dir,
    components: &[OsString],
) -> io::Result<(cap_std::fs::File, CapabilityMetadata)> {
    let mut current = root.try_clone()?;
    for component in &components[..components.len() - 1] {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .maybe_dir(true)
            .follow(FollowSymlinks::No);
        let child = current
            .open_with(Path::new(component), &options)
            .map_err(hidden_path_error)?;
        if !child.metadata().map_err(hidden_path_error)?.is_dir() {
            return Err(not_found("static file parent is not a directory"));
        }
        current = Dir::from_std_file(child.into_std());
    }

    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = current
        .open_with(
            Path::new(components.last().expect("validated non-empty components")),
            &options,
        )
        .map_err(hidden_path_error)?;
    let metadata = file.metadata().map_err(hidden_path_error)?;
    if !metadata.is_file() {
        return Err(not_found("static file is not a regular file"));
    }
    Ok((
        file,
        CapabilityMetadata {
            len: metadata.len(),
            modified: metadata.modified()?.into_std(),
        },
    ))
}

impl Backend for CapabilityBackend {
    type File = CapabilityFile;
    type Metadata = CapabilityMetadata;
    type OpenFuture = Pin<Box<dyn Future<Output = io::Result<Self::File>> + Send>>;
    type MetadataFuture = Pin<Box<dyn Future<Output = io::Result<Self::Metadata>> + Send>>;

    fn open(&self, path: PathBuf) -> Self::OpenFuture {
        let root = self.root.clone();
        let policy = self.policy;
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let components = normal_components(&path, policy)?;
                let (file, metadata) = open_regular_file(&root, &components)?;
                Ok(CapabilityFile {
                    file: tokio::fs::File::from_std(file.into_std()),
                    metadata,
                })
            })
            .await
            .map_err(io::Error::other)?
        })
    }

    fn metadata(&self, path: PathBuf) -> Self::MetadataFuture {
        let root = self.root.clone();
        let policy = self.policy;
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let components = normal_components(&path, policy)?;
                let (_, metadata) = open_regular_file(&root, &components)?;
                Ok(metadata)
            })
            .await
            .map_err(io::Error::other)?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    struct TestRoot(PathBuf);

    #[cfg(unix)]
    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "tvserver-capability-file-service-{}-{}",
                std::process::id(),
                rand::random::<u64>()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn backend(&self) -> CapabilityBackend {
            let root = Dir::open_ambient_dir(&self.0, cap_std::ambient_authority()).unwrap();
            CapabilityBackend::new(Arc::new(root), StaticFilePolicy::BookDownload)
        }
    }

    #[cfg(unix)]
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn book_download_policy_accepts_nested_epub_and_pdf_paths() {
        assert_eq!(
            normal_components(Path::new("./Fiction/Dune.EPUB"), StaticFilePolicy::BookDownload)
                .unwrap(),
            [OsString::from("Fiction"), OsString::from("Dune.EPUB")]
        );
        assert!(normal_components(
            Path::new("./Reference/manual.PDF"),
            StaticFilePolicy::BookDownload
        )
        .is_ok());
    }

    #[test]
    fn static_policies_reject_wrong_extensions_and_thumbnail_nesting() {
        for path in ["./notes.txt", "./book.epub.exe", "./collection"] {
            assert!(normal_components(Path::new(path), StaticFilePolicy::BookDownload).is_err());
        }
        assert!(
            normal_components(Path::new("./cover.JPG"), StaticFilePolicy::BookThumbnail).is_ok()
        );
        for path in ["./nested/cover.jpg", "./cover.png"] {
            assert!(normal_components(Path::new(path), StaticFilePolicy::BookThumbnail).is_err());
        }
    }

    #[test]
    fn static_policies_reject_non_relative_components() {
        for path in ["../secret.epub", "/secret.epub"] {
            assert!(normal_components(Path::new(path), StaticFilePolicy::BookDownload).is_err());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn backend_rejects_symlinked_parent_with_not_found() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new();
        let outside = root.0.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("Dune.epub"), b"book").unwrap();
        symlink(&outside, root.0.join("Fiction")).unwrap();
        let backend = root.backend();
        let path = PathBuf::from("Fiction/Dune.epub");

        assert_eq!(
            backend.open(path.clone()).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            backend.metadata(path).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn backend_rejects_final_symlink_with_not_found() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new();
        let target = root.0.join("target.epub");
        std::fs::write(&target, b"book").unwrap();
        symlink(&target, root.0.join("Dune.epub")).unwrap();
        let backend = root.backend();
        let path = PathBuf::from("Dune.epub");

        assert_eq!(
            backend.open(path.clone()).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            backend.metadata(path).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
