use crate::domain::{
    algorithm::file_integrity::{seal_reader, FileSeal},
    models::VideoDetails,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use cap_fs_ext::{
    DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsMaybeDirExt,
};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::{fs, io::AsyncWriteExt};

use crate::domain::traits::{FileStore, Filer, PrivateSnapshot, StagedFile, StoreObject};

const PRIVATE_SNAPSHOT_DIRECTORY: &str = ".tvserver-book-snapshots";

trait MoveFileSystem: Send + Sync {
    fn hard_link_staged_to_destination(
        &self,
        source_dir: &Dir,
        staged_source: &Path,
        destination_dir: &Dir,
        destination: &Path,
    ) -> io::Result<()>;
}

struct CapabilityMoveFileSystem;

impl MoveFileSystem for CapabilityMoveFileSystem {
    fn hard_link_staged_to_destination(
        &self,
        source_dir: &Dir,
        staged_source: &Path,
        destination_dir: &Dir,
        destination: &Path,
    ) -> io::Result<()> {
        source_dir.hard_link(staged_source, destination_dir, destination)
    }
}

#[derive(Clone)]
pub struct FileSystemStore {
    root: String,
    root_dir: Arc<OnceLock<Dir>>,
    move_filesystem: Arc<dyn MoveFileSystem>,
    private_snapshots: Arc<Mutex<HashMap<u128, PrivateSnapshotAuthority>>>,
}

#[derive(Clone)]
struct PrivateSnapshotAuthority {
    directory: Arc<Dir>,
    name: PathBuf,
    device: u64,
    inode: u64,
}

impl FileSystemStore {
    pub fn new(root: &str) -> Self {
        Self::new_with_move_filesystem(root, Arc::new(CapabilityMoveFileSystem))
    }

    fn new_with_move_filesystem(root: &str, move_filesystem: Arc<dyn MoveFileSystem>) -> Self {
        let store = Self {
            root: root.to_string(),
            root_dir: Arc::new(OnceLock::new()),
            move_filesystem,
            private_snapshots: Arc::new(Mutex::new(HashMap::new())),
        };
        let _ = store.open_root();
        store
    }

    fn get_real_path(&self, path: &str) -> Result<PathBuf, anyhow::Error> {
        let requested = Path::new(path);
        let root = Path::new(&self.root);
        let resolved = if requested.is_absolute() || requested.starts_with(root) {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };

        // Normalize path components to prevent traversal (e.g. "../../etc/passwd")
        let mut normalized = PathBuf::new();
        for component in resolved.components() {
            match component {
                std::path::Component::ParentDir => { normalized.pop(); }
                _ => normalized.push(component),
            }
        }

        let root_normalized = {
            let mut r = PathBuf::new();
            for component in Path::new(&self.root).components() {
                match component {
                    std::path::Component::ParentDir => { r.pop(); }
                    _ => r.push(component),
                }
            }
            r
        };

        if !normalized.starts_with(&root_normalized) {
            return Err(anyhow!("path escapes root directory: {}", path));
        }

        Ok(normalized)
    }

    fn open_root(&self) -> Result<&Dir> {
        if self.root_dir.get().is_none() {
            let root_path = self.normalized_root()?;
            let anchor = root_path.ancestors().last().ok_or_else(|| {
                anyhow!("filesystem root has no volume anchor: {}", root_path.display())
            })?;
            let relative_root = root_path.strip_prefix(anchor)?;
            let mut pending = Vec::new();
            append_relative_components(&mut pending, relative_root)?;
            let mut pending = VecDeque::from(pending);
            let mut resolved = Vec::new();
            let mut symlink_hops = 0;
            let mut root = Dir::open_ambient_dir(anchor, ambient_authority())?;
            while let Some(component) = pending.pop_front() {
                let component = Path::new(&component);
                match root.create_dir(component) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                let expected = root.symlink_metadata(component)?;
                if expected.file_type().is_symlink() {
                    let target = root.read_link(component)?;
                    let observed = root.symlink_metadata(component)?;
                    if !observed.file_type().is_symlink()
                        || observed.dev() != expected.dev()
                        || observed.ino() != expected.ino()
                    {
                        return Err(anyhow!(
                            "filesystem root symlink changed while it was read: {}",
                            component.display()
                        ));
                    }
                    symlink_hops += 1;
                    if symlink_hops > 40 {
                        return Err(anyhow!(
                            "too many symlinks while opening filesystem root: {}",
                            root_path.display()
                        ));
                    }

                    let mut redirected = if target.is_absolute() {
                        let relative_target = target.strip_prefix(anchor).map_err(|_| {
                            anyhow!(
                                "filesystem root symlink changes volume: {}",
                                target.display()
                            )
                        })?;
                        let mut components = Vec::new();
                        append_relative_components(&mut components, relative_target)?;
                        components
                    } else {
                        let mut components = resolved.clone();
                        append_relative_components(&mut components, &target)?;
                        components
                    };
                    redirected.extend(pending);
                    pending = VecDeque::from(redirected);
                    resolved.clear();
                    root = Dir::open_ambient_dir(anchor, ambient_authority())?;
                    continue;
                }
                if !expected.is_dir() {
                    return Err(anyhow!(
                        "filesystem root component is not a directory: {}",
                        component.display()
                    ));
                }
                let mut options = OpenOptions::new();
                options
                    .read(true)
                    .maybe_dir(true)
                    .follow(FollowSymlinks::Yes);
                let child = root.open_with(component, &options)?;
                let opened = child.metadata()?;
                if !opened.is_dir()
                    || opened.dev() != expected.dev()
                    || opened.ino() != expected.ino()
                {
                    return Err(anyhow!(
                        "filesystem root component changed while it was opened: {}",
                        component.display()
                    ));
                }
                root = Dir::from_std_file(child.into_std());
                resolved.push(component.as_os_str().to_os_string());
            }
            let _ = self.root_dir.set(root);
        }
        self.root_dir
            .get()
            .ok_or_else(|| anyhow!("failed to retain filesystem root capability"))
    }

    fn normalized_root(&self) -> Result<PathBuf> {
        Ok(std::path::absolute(self.get_real_path("")?)?)
    }

    fn rooted_relative_path(&self, path: &Path) -> Result<PathBuf> {
        let path = path.to_str().ok_or_else(|| {
            anyhow!("filesystem path is not valid UTF-8: {}", path.display())
        })?;
        let resolved = std::path::absolute(self.get_real_path(path)?)?;
        let root = self.normalized_root()?;
        resolved
            .strip_prefix(&root)
            .map(Path::to_path_buf)
            .map_err(|_| anyhow!("path escapes root directory: {}", resolved.display()))
    }

    fn source_path(&self, path: &str) -> Result<PathBuf> {
        let direct = PathBuf::from(path);
        match std::fs::symlink_metadata(&direct) {
            Ok(_) => Ok(std::path::absolute(direct)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(std::path::absolute(self.get_real_path(path)?)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn private_snapshot_name(&self, snapshot: &PrivateSnapshot) -> Result<PathBuf> {
        let root = self.normalized_root()?;
        let relative = snapshot
            .path
            .strip_prefix(&root)
            .map_err(|_| anyhow!("private snapshot is outside the store root"))?;
        let mut components = relative.components();
        match (components.next(), components.next(), components.next()) {
            (
                Some(Component::Normal(directory)),
                Some(Component::Normal(name)),
                None,
            ) if directory == PRIVATE_SNAPSHOT_DIRECTORY => Ok(PathBuf::from(name)),
            _ => Err(anyhow!(
                "private snapshot must have a safe single file name immediately beneath the snapshot directory"
            )),
        }
    }

    fn private_snapshot_authority(
        &self,
        snapshot: &PrivateSnapshot,
    ) -> Result<PrivateSnapshotAuthority> {
        let token_name = self.private_snapshot_name(snapshot)?;
        let snapshots = self
            .private_snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let authority = snapshots
            .get(&snapshot.id)
            .ok_or_else(|| anyhow!("private snapshot token is no longer active"))?;
        if authority.name != token_name
            || authority.device != snapshot.device
            || authority.inode != snapshot.inode
        {
            return Err(anyhow!("private snapshot token identity does not match its authority"));
        }
        Ok(authority.clone())
    }

    fn validate_visible_private_snapshot_directory(&self) -> Result<()> {
        match self
            .open_root()?
            .symlink_metadata(PRIVATE_SNAPSHOT_DIRECTORY)
        {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anyhow!(
                    "private book snapshot directory must be a directory and not a symlink"
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    fn forget_private_snapshot(&self, snapshot: &PrivateSnapshot) {
        self.private_snapshots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&snapshot.id);
    }
}

fn append_relative_components(components: &mut Vec<OsString>, path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if components.pop().is_none() {
                    return Err(anyhow!(
                        "filesystem root symlink escapes its volume: {}",
                        path.display()
                    ));
                }
            }
            Component::Normal(component) => components.push(component.to_os_string()),
            Component::Prefix(_) | Component::RootDir => {
                return Err(anyhow!(
                    "expected a relative filesystem root component: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn is_cross_device(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::CrossesDevices
}

fn unique_staging_name(prefix: &str) -> String {
    format!(".{prefix}-{:032x}", rand::random::<u128>())
}

fn ensure_safe_single_name(path: &Path) -> io::Result<()> {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("expected a safe single file name: {}", path.display()),
        )),
    }
}

#[cfg(any(target_os = "android", target_os = "linux", target_vendor = "apple"))]
fn rename_no_replace_relative(
    source_directory: &Dir,
    source_name: &Path,
    destination_directory: &Dir,
    destination_name: &Path,
) -> io::Result<()> {
    use rustix::fs::{renameat_with, RenameFlags};

    ensure_safe_single_name(source_name)?;
    ensure_safe_single_name(destination_name)?;
    Ok(renameat_with(
        source_directory,
        source_name,
        destination_directory,
        destination_name,
        RenameFlags::NOREPLACE,
    )?)
}

#[cfg(windows)]
fn rename_no_replace_relative(
    source_directory: &Dir,
    source_name: &Path,
    destination_directory: &Dir,
    destination_name: &Path,
) -> io::Result<()> {
    use cap_std::fs::OpenOptionsExt;
    use std::mem::{offset_of, size_of};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO, DELETE,
        },
    };

    ensure_safe_single_name(source_name)?;
    ensure_safe_single_name(destination_name)?;

    let mut source_options = OpenOptions::new();
    source_options
        .access_mode(DELETE)
        .follow(FollowSymlinks::No);
    let source = source_directory.open_with(source_name, &source_options)?;
    let destination_name: Vec<u16> = destination_name.as_os_str().encode_wide().collect();
    let header_size = offset_of!(FILE_RENAME_INFO, FileName);
    let buffer_size = header_size
        .checked_add(destination_name.len().checked_mul(size_of::<u16>()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long")
        })?)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long"))?;
    let word_count = buffer_size.div_ceil(size_of::<usize>());
    let mut buffer = vec![0usize; word_count];
    let rename_info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();

    // SAFETY: `buffer` is usize-aligned and sized for the fixed FILE_RENAME_INFO header plus
    // every UTF-16 destination-name byte. SetFileInformationByHandle only borrows it for the call.
    unsafe {
        (*rename_info).Anonymous.ReplaceIfExists = 0;
        (*rename_info).RootDirectory = destination_directory.as_raw_handle() as HANDLE;
        (*rename_info).FileNameLength = destination_name
            .len()
            .checked_mul(size_of::<u16>())
            .and_then(|length| u32::try_from(length).ok())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long")
            })?;
        std::ptr::copy_nonoverlapping(
            destination_name.as_ptr(),
            std::ptr::addr_of_mut!((*rename_info).FileName).cast::<u16>(),
            destination_name.len(),
        );
        if SetFileInformationByHandle(
            source.as_raw_handle() as HANDLE,
            FileRenameInfo,
            rename_info.cast(),
            u32::try_from(buffer_size).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination name is too long")
            })?,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_vendor = "apple",
    windows
)))]
fn rename_no_replace_relative(
    _source_directory: &Dir,
    _source_name: &Path,
    _destination_directory: &Dir,
    _destination_name: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic relative rename without replacement is unavailable on this platform",
    ))
}

fn ensure_cap_regular_file(dir: &Dir, path: &Path, description: &str) -> Result<()> {
    let metadata = dir.symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!(
            "{description} must be a regular file and not a symlink: {}",
            path.display()
        ));
    }
    Ok(())
}

fn ensure_snapshot_identity(
    metadata: &cap_std::fs::Metadata,
    device: u64,
    inode: u64,
) -> Result<()> {
    if !metadata.is_file() || metadata.dev() != device || metadata.ino() != inode {
        return Err(anyhow!(
            "private snapshot identity changed after it was created"
        ));
    }
    Ok(())
}

fn private_snapshot_identity_names(authority: &PrivateSnapshotAuthority) -> Result<Vec<PathBuf>> {
    let mut owned_names = Vec::new();
    let entries = authority.directory.entries().map_err(|error| {
        anyhow!(
            "failed to enumerate the retained private snapshot directory: {error}"
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            anyhow!("failed to enumerate a retained private snapshot entry: {error}")
        })?;
        let name = PathBuf::from(entry.file_name());
        match authority.directory.symlink_metadata(&name) {
            Ok(metadata)
                if metadata.is_file()
                    && metadata.dev() == authority.device
                    && metadata.ino() == authority.inode =>
            {
                owned_names.push(name);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(anyhow!(
                    "failed to inspect private snapshot cleanup candidate {}: {error}",
                    name.display()
                ));
            }
        }
    }

    Ok(owned_names)
}

struct PrivateSnapshotCleanupDirectory {
    directory: Dir,
}

fn remove_empty_directory_identity(directory: &Dir, device: u64, inode: u64) -> Result<()> {
    for entry in directory.entries()? {
        let entry = entry?;
        let name = PathBuf::from(entry.file_name());
        let metadata = match directory.symlink_metadata(&name) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_dir() || metadata.dev() != device || metadata.ino() != inode {
            continue;
        }
        let opened = match directory.open_dir_nofollow(&name) {
            Ok(opened) => opened,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let opened_metadata = opened.dir_metadata()?;
        if !opened_metadata.is_dir()
            || opened_metadata.dev() != device
            || opened_metadata.ino() != inode
        {
            continue;
        }
        opened.remove_open_dir()?;
    }
    Ok(())
}

fn create_private_snapshot_cleanup_directory_with_hook<F>(
    authority: &PrivateSnapshotAuthority,
    after_create: F,
) -> Result<(PathBuf, PrivateSnapshotCleanupDirectory)>
where
    F: FnOnce(&Dir, &Path),
{
    loop {
        let name = PathBuf::from(unique_staging_name("tvserver-snapshot-cleanup"));
        match authority.directory.create_dir(&name) {
            Ok(()) => {
                let created_metadata = authority.directory.symlink_metadata(&name)?;
                if !created_metadata.is_dir() || created_metadata.file_type().is_symlink() {
                    return Err(anyhow!(
                        "created private snapshot cleanup quarantine is not a real directory"
                    ));
                }
                let device = created_metadata.dev();
                let inode = created_metadata.ino();
                after_create(&authority.directory, &name);
                let opened = authority
                    .directory
                    .open_dir_nofollow(&name)
                    .and_then(|directory| {
                        let opened_metadata = directory.dir_metadata()?;
                        if !opened_metadata.is_dir()
                            || opened_metadata.dev() != device
                            || opened_metadata.ino() != inode
                        {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "private snapshot cleanup quarantine identity changed while opening",
                            ));
                        }
                        Ok(directory)
                    });
                match opened {
                    Ok(directory) => {
                        return Ok((
                            name,
                            PrivateSnapshotCleanupDirectory { directory },
                        ));
                    }
                    Err(open_error) => {
                        let cleanup_result =
                            remove_empty_directory_identity(&authority.directory, device, inode);
                        return match cleanup_result {
                            Ok(()) => Err(anyhow!(
                                "failed to bind private snapshot cleanup quarantine {} to its created identity: {open_error}",
                                name.display()
                            )),
                            Err(cleanup_error) => Err(anyhow!(
                                "failed to bind private snapshot cleanup quarantine {} to its created identity: {open_error}; additionally failed to remove the created quarantine identity: {cleanup_error}",
                                name.display()
                            )),
                        };
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(anyhow!(
                    "failed to create a private snapshot cleanup quarantine: {error}"
                ));
            }
        }
    }
}

fn create_private_snapshot_cleanup_directory(
    authority: &PrivateSnapshotAuthority,
) -> Result<(PathBuf, PrivateSnapshotCleanupDirectory)> {
    create_private_snapshot_cleanup_directory_with_hook(authority, |_, _| {})
}

fn remove_private_snapshot_cleanup_directory_if_empty(
    quarantine: PrivateSnapshotCleanupDirectory,
) -> Result<()> {
    if quarantine.directory.entries()?.next().transpose()?.is_some() {
        return Err(anyhow!(
            "private snapshot cleanup quarantine is not empty; its unowned entries were preserved"
        ));
    }
    quarantine.directory.remove_open_dir()?;
    Ok(())
}

fn quarantine_private_snapshot_candidate(
    authority: &PrivateSnapshotAuthority,
    name: &Path,
) -> Result<()> {
    let (quarantine_name, quarantine) = create_private_snapshot_cleanup_directory(authority)?;
    let quarantined_name = PathBuf::from(unique_staging_name("tvserver-snapshot-candidate"));
    let result = match rename_no_replace_relative(
        &authority.directory,
        name,
        &quarantine.directory,
        &quarantined_name,
    ) {
        Ok(()) => match quarantine.directory.symlink_metadata(&quarantined_name) {
            Ok(metadata)
                if metadata.is_file()
                    && metadata.dev() == authority.device
                    && metadata.ino() == authority.inode =>
            {
                quarantine
                    .directory
                    .remove_file(&quarantined_name)
                    .map_err(|error| {
                        anyhow!(
                            "failed to remove quarantined owned private snapshot entry {}: {error}",
                            name.display()
                        )
                    })
            }
            Ok(_) => match rename_no_replace_relative(
                &quarantine.directory,
                &quarantined_name,
                &authority.directory,
                name,
            ) {
                Ok(()) => Ok(()),
                Err(error) => Err(anyhow!(
                    "private snapshot cleanup candidate {} changed identity; it remains preserved beneath quarantine {} because it could not be restored without replacement: {error}",
                    name.display(),
                    quarantine_name.display()
                )),
            },
            Err(error) => Err(anyhow!(
                "failed to inspect quarantined private snapshot cleanup candidate {}: {error}",
                name.display()
            )),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow!(
            "failed to quarantine private snapshot cleanup candidate {}: {error}",
            name.display()
        )),
    };

    if let Err(remove_error) = remove_private_snapshot_cleanup_directory_if_empty(quarantine) {
        return Err(anyhow!(
            "{}; additionally failed to remove private snapshot cleanup quarantine {}: {remove_error}",
            result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_else(|| "private snapshot cleanup candidate handling succeeded".to_string()),
            quarantine_name.display()
        ));
    }

    result
}

fn remove_private_snapshot_identity(authority: &PrivateSnapshotAuthority) -> Result<()> {
    const MAX_CLEANUP_PASSES: usize = 16;

    for _ in 0..MAX_CLEANUP_PASSES {
        let owned_names = private_snapshot_identity_names(authority)?;
        if owned_names.is_empty() {
            return Ok(());
        }

        for name in owned_names {
            quarantine_private_snapshot_candidate(authority, &name)?;
        }
    }

    let remaining = private_snapshot_identity_names(authority)?;
    Err(anyhow!(
        "private snapshot cleanup did not converge; {} owned directory entries remain",
        remaining.len()
    ))
}

fn restore_staged_file(dir: &Dir, staged: &Path, original: &Path) -> Result<()> {
    ensure_cap_regular_file(dir, staged, "staged file")?;
    dir.hard_link(staged, dir, original).map_err(|error| {
        anyhow!(
            "failed to restore {} to {} without replacing an existing file: {error}",
            staged.display(),
            original.display()
        )
    })?;
    if let Err(error) = dir.remove_file(staged) {
        tracing::warn!(
            "Restored staged file to {} but could not remove {}: {}",
            original.display(),
            staged.display(),
            error
        );
    }
    Ok(())
}

fn staged_relative_path(root_path: &Path, absolute_path: &Path) -> Option<PathBuf> {
    absolute_path.strip_prefix(root_path).ok().map(Path::to_path_buf)
}

fn remove_staged_source_after_publication(
    source_dir: &Dir,
    staged_source: &Path,
    destination: &Path,
) {
    if let Err(error) = source_dir.remove_file(staged_source) {
        tracing::warn!(
            "Published file at {} but could not remove staged source {}: {}",
            destination.display(),
            staged_source.display(),
            error
        );
    }
}

fn copy_staged_file(
    source_dir: &Dir,
    staged_source: &Path,
    destination_dir: &Dir,
    destination: &Path,
) -> Result<()> {
    let mut source_options = OpenOptions::new();
    source_options.read(true).follow(FollowSymlinks::No);
    let mut source = source_dir.open_with(staged_source, &source_options)?;
    let source_metadata = source.metadata()?;
    if !source_metadata.is_file() {
        return Err(anyhow!("source must be a regular file"));
    }

    let destination_parent = destination.parent().unwrap_or_else(|| Path::new(""));
    let temporary = destination_parent.join(unique_staging_name("tvserver-copy"));
    let mut destination_options = OpenOptions::new();
    destination_options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut copied = destination_dir.open_with(&temporary, &destination_options)?;

    let result = (|| -> Result<()> {
        io::copy(&mut source, &mut copied)?;
        destination_dir.set_permissions(&temporary, source_metadata.permissions())?;
        copied.sync_all()?;
        drop(copied);
        drop(source);
        destination_dir.rename(&temporary, destination_dir, destination)?;
        if let Err(error) = source_dir.remove_file(staged_source) {
            tracing::warn!(
                "Published cross-device copy at {} but could not remove staged source {}: {}",
                destination.display(),
                staged_source.display(),
                error
            );
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = destination_dir.remove_file(&temporary);
    }
    result
}

fn copy_staged_file_no_replace(
    source_dir: &Dir,
    staged_source: &Path,
    destination_dir: &Dir,
    destination: &Path,
) -> Result<()> {
    let mut source_options = OpenOptions::new();
    source_options.read(true).follow(FollowSymlinks::No);
    let mut source = source_dir.open_with(staged_source, &source_options)?;
    let source_metadata = source.metadata()?;
    if !source_metadata.is_file() {
        return Err(anyhow!("source must be a regular file"));
    }

    let destination_parent = destination.parent().unwrap_or_else(|| Path::new(""));
    let temporary = destination_parent.join(unique_staging_name("tvserver-copy"));
    let mut destination_options = OpenOptions::new();
    destination_options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut copied = destination_dir.open_with(&temporary, &destination_options)?;

    let result = (|| -> Result<()> {
        io::copy(&mut source, &mut copied)?;
        destination_dir.set_permissions(&temporary, source_metadata.permissions())?;
        copied.sync_all()?;
        drop(copied);
        drop(source);
        destination_dir
            .hard_link(&temporary, destination_dir, destination)
            .map_err(|error| {
                anyhow!(
                    "failed to publish {} without replacing an existing file: {error}",
                    destination.display()
                )
            })?;
        ensure_cap_regular_file(destination_dir, destination, "destination file")?;
        if let Err(error) = destination_dir.remove_file(&temporary) {
            tracing::warn!(
                "Published cross-device copy at {} but could not remove temporary file {}: {}",
                destination.display(),
                temporary.display(),
                error
            );
        }
        remove_staged_source_after_publication(source_dir, staged_source, destination);
        Ok(())
    })();
    if result.is_err() {
        let _ = destination_dir.remove_file(&temporary);
    }
    result
}

fn copy_and_seal<R: Read, W: Write>(source: &mut R, destination: &mut W) -> io::Result<FileSeal> {
    struct CopyReader<'a, R, W> {
        source: &'a mut R,
        destination: &'a mut W,
    }

    impl<R: Read, W: Write> Read for CopyReader<'_, R, W> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = self.source.read(buffer)?;
            self.destination.write_all(&buffer[..read])?;
            Ok(read)
        }
    }

    seal_reader(&mut CopyReader {
        source,
        destination,
    })
}

fn copy_private_snapshot_no_replace_verified(
    snapshot_dir: &Dir,
    snapshot_name: &Path,
    snapshot_device: u64,
    snapshot_inode: u64,
    root_dir: &Dir,
    destination: &Path,
    expected_seal: &FileSeal,
) -> Result<()> {
    let mut source_options = OpenOptions::new();
    source_options.read(true).follow(FollowSymlinks::No);
    let mut source = snapshot_dir.open_with(snapshot_name, &source_options)?;
    let source_metadata = source.metadata()?;
    ensure_snapshot_identity(&source_metadata, snapshot_device, snapshot_inode)?;

    let destination_parent = destination.parent().unwrap_or_else(|| Path::new(""));
    let temporary = destination_parent.join(unique_staging_name("tvserver-verified-copy"));
    let mut destination_options = OpenOptions::new();
    destination_options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut copied = root_dir.open_with(&temporary, &destination_options)?;

    let result = (|| -> Result<()> {
        let actual_seal = copy_and_seal(&mut source, &mut copied)?;
        root_dir.set_permissions(&temporary, source_metadata.permissions())?;
        copied.sync_all()?;
        let copied_metadata = copied.metadata()?;
        if actual_seal != *expected_seal {
            return Err(anyhow!(
                "private snapshot integrity check failed: expected {expected_seal:?}, copied {actual_seal:?}"
            ));
        }
        drop(copied);
        drop(source);
        root_dir
            .hard_link(&temporary, root_dir, destination)
            .map_err(|error| {
                anyhow!(
                    "failed to publish {} without replacing an existing file: {error}",
                    destination.display()
                )
            })?;
        if let Err(error) = ensure_cap_regular_file(root_dir, destination, "destination file") {
            let _ = root_dir.remove_file(destination);
            return Err(error);
        }
        let published_metadata = root_dir.symlink_metadata(destination)?;
        if published_metadata.dev() != copied_metadata.dev()
            || published_metadata.ino() != copied_metadata.ino()
        {
            return Err(anyhow!(
                "published destination identity changed before snapshot cleanup"
            ));
        }
        if let Err(error) = snapshot_dir.remove_file(snapshot_name) {
            let rollback = root_dir.remove_file(destination);
            return Err(match rollback {
                Ok(()) => anyhow!(
                    "failed to remove the original private snapshot after publication: {error}"
                ),
                Err(rollback_error) => anyhow!(
                    "failed to remove the original private snapshot after publication: {error}; additionally failed to roll back destination: {rollback_error}"
                ),
            });
        }
        if let Err(error) = root_dir.remove_file(&temporary) {
            tracing::warn!(
                "Published verified copy at {} but could not remove temporary file {}: {}",
                destination.display(),
                temporary.display(),
                error
            );
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = root_dir.remove_file(&temporary);
    }
    result
}

pub struct FileStoreObject {
    file: PathBuf,
}

impl FileStoreObject {
    pub fn new(file: &Path) -> Self {
        Self {
            file: PathBuf::from(file),
        }
    }

    async fn read_struct_from_json_file<T: DeserializeOwned>(
        file_path: &Path,
    ) -> anyhow::Result<T> {
        // Read the file content
        let file_content = fs::read(file_path).await?;

        // Deserialize the JSON content into the target struct
        let deserialized_struct = serde_json::from_slice(&file_content)?;
        Ok(deserialized_struct)
    }

    async fn write_struct_to_json_file<T: Serialize>(data: &T, file_path: &Path) -> Result<()> {
        // Serialize the struct to a JSON string with indentation.
        let json_string = serde_json::to_string_pretty(data)?;

        // Open the file in write mode or create it if it doesn't exist.
        let mut file = fs::File::create(file_path).await?;

        // Write the JSON string to the file.
        file.write_all(json_string.as_bytes()).await?;

        // Flush and close the file.
        file.flush().await?;

        Ok(())
    }
}

#[async_trait]
impl Filer for FileStoreObject {
    fn is_dir(&self) -> bool {
        self.file.is_dir()
    }

    async fn get_metadata(&self) -> Result<VideoDetails> {
        let data_file = self.file.with_extension("json");
        if !data_file.exists() && self.file.exists() {
            return Ok(VideoDetails{..Default::default()})
        }

        Self::read_struct_from_json_file(&data_file).await
    }

    async fn save_metadata(&self, details: VideoDetails) -> Result<()> {
        let data_file = self.file.with_extension("json");

        Self::write_struct_to_json_file(&details, &data_file).await
    }
}

fn directory_entry_name(name: OsString) -> Result<String> {
    name.into_string()
        .map_err(|name| anyhow!("directory entry name is not valid UTF-8: {name:?}"))
}

#[async_trait]
impl FileStore for FileSystemStore {
    async fn create_folder(&self, path: &Path) -> Result<()> {
        let store = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let relative = store.rooted_relative_path(&path)?;
            store.open_root()?.create_dir_all(relative)?;
            Result::<()>::Ok(())
        })
        .await??;
        Ok(())
    }

    async fn list_folder(&self, _path: &str) -> Result<(Vec<String>, Vec<String>)> {
        let path = self.get_real_path(_path)?;
        let mut directories: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();

        if !path.is_dir() {
            return Err(anyhow!("{} is not a directory", path.to_string_lossy()));
        }

        let mut read_dir = fs::read_dir(path).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let name = directory_entry_name(entry.file_name())?;
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                directories.push(name);
            } else if file_type.is_file() {
                files.push(name);
            }
        }

        directories.sort();
        files.sort();

        Ok((directories, files))
    }

    async fn ensure_path_exists(&self, _path: &str) -> Result<()> {
        let path = self.get_real_path(_path)?;
        if !path.exists() {
            fs::create_dir_all(path).await?;
        } else if !path.is_dir() {
            return Err(anyhow!(
                "a file already exists with that name: {}",
                path.to_string_lossy()
            ));
        }
        Ok(())
    }

    async fn rename(&self, _old_path: &str, _new_path: &str) -> Result<()> {
        let store = self.clone();
        let old_path = _old_path.to_string();
        let new_path = PathBuf::from(_new_path);
        tokio::task::spawn_blocking(move || {
            let old_path = store.source_path(&old_path)?;
            let destination = store.rooted_relative_path(&new_path)?;
            let destination_dir = store.open_root()?;
            let root_path = store.normalized_root()?;
            let internal_source = old_path.strip_prefix(&root_path).ok().map(Path::to_path_buf);
            let ambient_source;
            let (source_dir, source_path) = if let Some(relative) = internal_source {
                (destination_dir, relative)
            } else {
                let old_parent = old_path.parent().ok_or_else(|| {
                    anyhow!("source path has no parent: {}", old_path.display())
                })?;
                let old_name = old_path.file_name().ok_or_else(|| {
                    anyhow!("source path has no file name: {}", old_path.display())
                })?;
                ambient_source = Dir::open_ambient_dir(old_parent, ambient_authority())?;
                (&ambient_source, PathBuf::from(old_name))
            };
            ensure_cap_regular_file(source_dir, &source_path, "source file")?;

            let staged_source = PathBuf::from(unique_staging_name("tvserver-move"));
            source_dir.rename(&source_path, source_dir, &staged_source)?;

            let move_result = (|| -> Result<()> {
                ensure_cap_regular_file(source_dir, &staged_source, "source file")?;
                match source_dir.rename(&staged_source, destination_dir, &destination) {
                    Ok(()) => ensure_cap_regular_file(
                        destination_dir,
                        &destination,
                        "destination file",
                    ),
                    Err(error) if is_cross_device(&error) => copy_staged_file(
                        source_dir,
                        &staged_source,
                        destination_dir,
                        &destination,
                    ),
                    Err(error) => Err(error.into()),
                }
            })();

            if let Err(error) = move_result {
                if source_dir.symlink_metadata(&staged_source).is_ok() {
                    restore_staged_file(source_dir, &staged_source, &source_path).map_err(
                        |restore_error| {
                            anyhow!(
                                "{error}; additionally failed to restore source {}: {restore_error}",
                                old_path.display()
                            )
                        },
                    )?;
                }
                return Err(error);
            }
            Ok(())
        })
        .await?
    }

    async fn rename_no_replace(&self, _old_path: &str, _new_path: &str) -> Result<()> {
        let staged = self.stage_no_follow(_old_path).await?;
        self.publish_staged_no_replace(&staged, _new_path).await
    }

    async fn stage_no_follow(&self, source: &str) -> Result<StagedFile> {
        let store = self.clone();
        let source = source.to_string();
        tokio::task::spawn_blocking(move || {
            let original_path = store.source_path(&source)?;
            let root_path = store.normalized_root()?;
            let root_dir = store.open_root()?;
            let internal_source = staged_relative_path(&root_path, &original_path);
            let ambient_source;
            let (source_dir, source_path) = if let Some(relative) = internal_source {
                (root_dir, relative)
            } else {
                let original_parent = original_path.parent().ok_or_else(|| {
                    anyhow!("source path has no parent: {}", original_path.display())
                })?;
                let original_name = original_path.file_name().ok_or_else(|| {
                    anyhow!("source path has no file name: {}", original_path.display())
                })?;
                ambient_source = Dir::open_ambient_dir(original_parent, ambient_authority())?;
                (&ambient_source, PathBuf::from(original_name))
            };
            ensure_cap_regular_file(source_dir, &source_path, "source file")?;

            let staged_source = source_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(unique_staging_name("tvserver-ingest"));
            source_dir.rename(&source_path, source_dir, &staged_source)?;
            if let Err(error) = ensure_cap_regular_file(source_dir, &staged_source, "staged source") {
                if source_dir.symlink_metadata(&staged_source).is_ok() {
                    if let Err(restore_error) = source_dir
                        .hard_link(&staged_source, source_dir, &source_path)
                        .and_then(|_| source_dir.remove_file(&staged_source))
                    {
                        return Err(anyhow!(
                            "{error}; additionally failed to restore rejected staged source {}: {restore_error}",
                            original_path.display()
                        ));
                    }
                }
                return Err(error);
            }

            let staged_path = if original_path.starts_with(&root_path) {
                root_path.join(&staged_source)
            } else {
                original_path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .join(staged_source)
            };
            Ok(StagedFile {
                original_path,
                staged_path,
            })
        })
        .await?
    }

    async fn create_private_snapshot(&self, staged: &StagedFile) -> Result<PrivateSnapshot> {
        let store = self.clone();
        let staged = staged.clone();
        tokio::task::spawn_blocking(move || {
            if staged.staged_path.parent() != staged.original_path.parent() {
                return Err(anyhow!("staged source must remain beside its original path"));
            }
            let root_path = store.normalized_root()?;
            let root_dir = store.open_root()?;
            let internal_staged = staged_relative_path(&root_path, &staged.staged_path);
            let ambient_source;
            let (source_dir, staged_source) = if let Some(relative) = internal_staged {
                (root_dir, relative)
            } else {
                let parent = staged.staged_path.parent().ok_or_else(|| {
                    anyhow!("staged source has no parent: {}", staged.staged_path.display())
                })?;
                let staged_name = staged.staged_path.file_name().ok_or_else(|| {
                    anyhow!("staged source has no file name: {}", staged.staged_path.display())
                })?;
                ambient_source = Dir::open_ambient_dir(parent, ambient_authority())?;
                (&ambient_source, PathBuf::from(staged_name))
            };
            ensure_cap_regular_file(source_dir, &staged_source, "staged source")?;

            match root_dir.create_dir(PRIVATE_SNAPSHOT_DIRECTORY) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            let snapshot_directory = Path::new(PRIVATE_SNAPSHOT_DIRECTORY);
            let directory_metadata = root_dir.symlink_metadata(snapshot_directory)?;
            if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
                return Err(anyhow!(
                    "private book snapshot directory must be a directory and not a symlink"
                ));
            }

            let snapshot_dir = root_dir.open_dir_nofollow(snapshot_directory)?;
            let opened_directory_metadata = snapshot_dir.dir_metadata()?;
            if opened_directory_metadata.dev() != directory_metadata.dev()
                || opened_directory_metadata.ino() != directory_metadata.ino()
            {
                return Err(anyhow!(
                    "private book snapshot directory changed while it was opened"
                ));
            }

            let snapshot_name = PathBuf::from(unique_staging_name("snapshot"));
            let copy_result = (|| -> Result<(u64, u64)> {
                let mut source_options = OpenOptions::new();
                source_options.read(true).follow(FollowSymlinks::No);
                let mut source = source_dir.open_with(&staged_source, &source_options)?;
                if !source.metadata()?.is_file() {
                    return Err(anyhow!("opened staged source is not a regular file"));
                }

                let mut destination_options = OpenOptions::new();
                destination_options
                    .write(true)
                    .create_new(true)
                    .follow(FollowSymlinks::No);
                let mut snapshot = snapshot_dir.open_with(&snapshot_name, &destination_options)?;
                io::copy(&mut source, &mut snapshot)?;
                snapshot.flush()?;
                snapshot.sync_all()?;
                let metadata = snapshot.metadata()?;
                if !metadata.is_file() {
                    return Err(anyhow!("private snapshot must be a regular file"));
                }
                Ok((metadata.dev(), metadata.ino()))
            })();
            let (device, inode) = match copy_result {
                Ok(identity) => identity,
                Err(error) => {
                    let _ = snapshot_dir.remove_file(&snapshot_name);
                    return Err(error);
                }
            };

            let mut snapshots = store
                .private_snapshots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let id = loop {
                let id = rand::random::<u128>();
                if !snapshots.contains_key(&id) {
                    break id;
                }
            };
            snapshots.insert(
                id,
                PrivateSnapshotAuthority {
                    directory: Arc::new(snapshot_dir),
                    name: snapshot_name.clone(),
                    device,
                    inode,
                },
            );

            Ok(PrivateSnapshot::new(
                root_path.join(snapshot_directory).join(snapshot_name),
                id,
                device,
                inode,
            ))
        })
        .await?
    }

    async fn seal_private_snapshot(&self, snapshot: &PrivateSnapshot) -> Result<FileSeal> {
        let store = self.clone();
        let snapshot = snapshot.clone();
        tokio::task::spawn_blocking(move || {
            store.validate_visible_private_snapshot_directory()?;
            let authority = store.private_snapshot_authority(&snapshot)?;
            let mut source_options = OpenOptions::new();
            source_options.read(true).follow(FollowSymlinks::No);
            let mut source = authority.directory.open_with(&authority.name, &source_options)?;
            ensure_snapshot_identity(&source.metadata()?, authority.device, authority.inode)?;
            Ok(seal_reader(&mut source)?)
        })
        .await?
    }

    async fn publish_private_snapshot_no_replace(
        &self,
        snapshot: &PrivateSnapshot,
        destination: &str,
        expected_seal: &FileSeal,
    ) -> Result<()> {
        let store = self.clone();
        let snapshot = snapshot.clone();
        let destination = PathBuf::from(destination);
        let expected_seal = expected_seal.clone();
        tokio::task::spawn_blocking(move || {
            store.validate_visible_private_snapshot_directory()?;
            let authority = store.private_snapshot_authority(&snapshot)?;
            let destination = store.rooted_relative_path(&destination)?;
            let root_dir = store.open_root()?;
            let result = copy_private_snapshot_no_replace_verified(
                &authority.directory,
                &authority.name,
                authority.device,
                authority.inode,
                root_dir,
                &destination,
                &expected_seal,
            );
            if result.is_ok() {
                store.forget_private_snapshot(&snapshot);
            }
            result
        })
        .await?
    }

    async fn remove_private_snapshot(&self, snapshot: &PrivateSnapshot) -> Result<()> {
        let store = self.clone();
        let snapshot = snapshot.clone();
        tokio::task::spawn_blocking(move || {
            let authority = store.private_snapshot_authority(&snapshot)?;
            let result = remove_private_snapshot_identity(&authority);
            store.forget_private_snapshot(&snapshot);
            result
        })
        .await?
    }

    async fn regular_file_exists_no_follow(&self, path: &Path) -> Result<bool> {
        let store = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let relative = store.rooted_relative_path(&path)?;
            match store.open_root()?.symlink_metadata(&relative) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
                    anyhow!(
                        "canonical file must be a regular file and not a symlink: {}",
                        path.display()
                    ),
                ),
                Ok(_) => Ok(true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error.into()),
            }
        })
        .await?
    }

    async fn remove_regular_no_follow(&self, path: &Path) -> Result<()> {
        let store = self.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let absolute = std::path::absolute(&path)?;
            let root_path = store.normalized_root()?;
            if let Some(relative) = staged_relative_path(&root_path, &absolute) {
                let root = store.open_root()?;
                return match root.symlink_metadata(&relative) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
                        anyhow!("cleanup target must be a regular file and not a symlink"),
                    ),
                    Ok(_) => Ok(root.remove_file(relative)?),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                };
            }

            let parent = absolute.parent().ok_or_else(|| {
                anyhow!("cleanup target has no parent: {}", absolute.display())
            })?;
            let name = absolute.file_name().ok_or_else(|| {
                anyhow!("cleanup target has no file name: {}", absolute.display())
            })?;
            let directory = Dir::open_ambient_dir(parent, ambient_authority())?;
            match directory.symlink_metadata(name) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
                    anyhow!("cleanup target must be a regular file and not a symlink"),
                ),
                Ok(_) => Ok(directory.remove_file(name)?),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
        .await?
    }

    async fn discard_staged(&self, staged: &StagedFile) -> Result<()> {
        let store = self.clone();
        let staged = staged.clone();
        tokio::task::spawn_blocking(move || {
            if staged.staged_path.parent() != staged.original_path.parent() {
                return Err(anyhow!("staged source must remain beside its original path"));
            }
            let root_path = store.normalized_root()?;
            if let Some(relative) = staged_relative_path(&root_path, &staged.staged_path) {
                let root = store.open_root()?;
                ensure_cap_regular_file(root, &relative, "staged source")?;
                return Ok(root.remove_file(relative)?);
            }
            let parent = staged.staged_path.parent().ok_or_else(|| {
                anyhow!("staged source has no parent: {}", staged.staged_path.display())
            })?;
            let name = staged.staged_path.file_name().ok_or_else(|| {
                anyhow!("staged source has no file name: {}", staged.staged_path.display())
            })?;
            let source_dir = Dir::open_ambient_dir(parent, ambient_authority())?;
            ensure_cap_regular_file(&source_dir, Path::new(name), "staged source")?;
            Ok(source_dir.remove_file(name)?)
        })
        .await?
    }

    async fn publish_staged_no_replace(
        &self,
        staged: &StagedFile,
        destination: &str,
    ) -> Result<()> {
        let store = self.clone();
        let staged = staged.clone();
        let destination = PathBuf::from(destination);
        tokio::task::spawn_blocking(move || {
            if staged.staged_path.parent() != staged.original_path.parent() {
                return Err(anyhow!("staged source must remain beside its original path"));
            }
            let destination = store.rooted_relative_path(&destination)?;
            let destination_dir = store.open_root()?;
            let root_path = store.normalized_root()?;
            let internal_staged = staged_relative_path(&root_path, &staged.staged_path);
            let ambient_source;
            let (source_dir, staged_source, original_source) = if let Some(relative) = internal_staged {
                let original = staged_relative_path(&root_path, &staged.original_path)
                    .ok_or_else(|| anyhow!("staged and original paths cross the store root"))?;
                (destination_dir, relative, original)
            } else {
                let parent = staged.staged_path.parent().ok_or_else(|| {
                    anyhow!("staged source has no parent: {}", staged.staged_path.display())
                })?;
                let staged_name = staged.staged_path.file_name().ok_or_else(|| {
                    anyhow!("staged source has no file name: {}", staged.staged_path.display())
                })?;
                let original_name = staged.original_path.file_name().ok_or_else(|| {
                    anyhow!("original source has no file name: {}", staged.original_path.display())
                })?;
                ambient_source = Dir::open_ambient_dir(parent, ambient_authority())?;
                (
                    &ambient_source,
                    PathBuf::from(staged_name),
                    PathBuf::from(original_name),
                )
            };

            ensure_cap_regular_file(source_dir, &staged_source, "staged source")?;
            let move_result = match store.move_filesystem.hard_link_staged_to_destination(
                source_dir,
                &staged_source,
                destination_dir,
                &destination,
            ) {
                Ok(()) => ensure_cap_regular_file(
                    destination_dir,
                    &destination,
                    "destination file",
                )
                .map(|_| {
                    remove_staged_source_after_publication(
                        source_dir,
                        &staged_source,
                        &destination,
                    );
                }),
                Err(error) if is_cross_device(&error) => copy_staged_file_no_replace(
                    source_dir,
                    &staged_source,
                    destination_dir,
                    &destination,
                ),
                Err(error) => Err(anyhow!(
                    "failed to publish {} without replacing an existing file: {error}",
                    destination.display()
                )),
            };

            if let Err(error) = move_result {
                if source_dir.symlink_metadata(&staged_source).is_ok() {
                    restore_staged_file(source_dir, &staged_source, &original_source).map_err(
                        |restore_error| {
                            anyhow!(
                                "{error}; additionally failed to restore source {}: {restore_error}",
                                staged.original_path.display()
                            )
                        },
                    )?;
                }
                return Err(error);
            }
            Ok(())
        })
        .await?
    }

    async fn restore_staged(&self, staged: &StagedFile) -> Result<()> {
        let store = self.clone();
        let staged = staged.clone();
        tokio::task::spawn_blocking(move || {
            if staged.staged_path.parent() != staged.original_path.parent() {
                return Err(anyhow!("staged source must remain beside its original path"));
            }
            let root_path = store.normalized_root()?;
            if let Some(staged_relative) = staged_relative_path(&root_path, &staged.staged_path) {
                let original_relative = staged_relative_path(&root_path, &staged.original_path)
                    .ok_or_else(|| anyhow!("staged and original paths cross the store root"))?;
                return restore_staged_file(
                    store.open_root()?,
                    &staged_relative,
                    &original_relative,
                );
            }
            let parent = staged.staged_path.parent().ok_or_else(|| {
                anyhow!("staged source has no parent: {}", staged.staged_path.display())
            })?;
            let staged_name = staged.staged_path.file_name().ok_or_else(|| {
                anyhow!("staged source has no file name: {}", staged.staged_path.display())
            })?;
            let original_name = staged.original_path.file_name().ok_or_else(|| {
                anyhow!("original source has no file name: {}", staged.original_path.display())
            })?;
            let source_dir = Dir::open_ambient_dir(parent, ambient_authority())?;
            restore_staged_file(
                &source_dir,
                Path::new(staged_name),
                Path::new(original_name),
            )
        })
        .await?
    }

    async fn restore(&self, staged_path: &str, original_path: &str) -> Result<()> {
        let store = self.clone();
        let staged_path = PathBuf::from(staged_path);
        let original_path = PathBuf::from(original_path);
        tokio::task::spawn_blocking(move || {
            let staged_path = store.rooted_relative_path(&staged_path)?;
            let original_path = store.rooted_relative_path(&original_path)?;
            restore_staged_file(store.open_root()?, &staged_path, &original_path)
        })
        .await?
    }

    async fn get(&self, path: &str) -> anyhow::Result<StoreObject> {
        let obj = FileStoreObject::new(&self.get_real_path(path)?);
        Ok(Arc::new(obj))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let store = self.clone();
        let path = PathBuf::from(path);
        tokio::task::spawn_blocking(move || {
            let relative = store.rooted_relative_path(&path)?;
            let root = store.open_root()?;
            match root.symlink_metadata(&relative) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
                    anyhow!("file must be a regular file and not a symlink: {}", path.display()),
                ),
                Ok(_) => {
                    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
                    let staged = parent.join(unique_staging_name("tvserver-delete"));
                    root.rename(&relative, root, &staged)?;
                    if let Err(error) = ensure_cap_regular_file(root, &staged, "staged file") {
                        return Err(anyhow!(
                            "{error}; unsafe replacement was quarantined at {}",
                            staged.display()
                        ));
                    }
                    Ok(root.remove_file(staged)?)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        })
        .await?
    }

    async fn remove_empty_dir(&self, path: &Path) -> Result<()> {
        fs::remove_dir(path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::path::PathBuf;

    const TEST_DIR: &str = "tests/fixtures/media_dir";

    struct ForcedCrossDevice;

    impl MoveFileSystem for ForcedCrossDevice {
        fn hard_link_staged_to_destination(
            &self,
            _source_dir: &Dir,
            _staged_source: &Path,
            _destination_dir: &Dir,
            _destination: &Path,
        ) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::CrossesDevices))
        }
    }

    #[cfg(unix)]
    async fn swapped_private_snapshot_directory(
        test_name: &str,
    ) -> (
        PathBuf,
        PathBuf,
        FileSystemStore,
        PrivateSnapshot,
        FileSeal,
        PathBuf,
    ) {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-{test_name}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"legitimate snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let seal = store.seal_private_snapshot(&snapshot).await.unwrap();
        let snapshot_name = snapshot.path.file_name().unwrap().to_owned();
        let snapshot_directory = root.join(PRIVATE_SNAPSHOT_DIRECTORY);
        std::fs::rename(
            &snapshot_directory,
            root.join("original-private-snapshots"),
        )
        .unwrap();
        let decoy_directory = root.join("decoy");
        std::fs::create_dir(&decoy_directory).unwrap();
        let decoy = decoy_directory.join(snapshot_name);
        std::fs::write(&decoy, b"legitimate snapshot bytes").unwrap();
        symlink("decoy", &snapshot_directory).unwrap();

        (base, root, store, snapshot, seal, decoy)
    }

    #[cfg(unix)]
    async fn replaced_private_snapshot_directory(
        test_name: &str,
    ) -> (
        PathBuf,
        PathBuf,
        FileSystemStore,
        PrivateSnapshot,
        FileSeal,
        PathBuf,
        PathBuf,
    ) {
        let base = std::env::temp_dir().join(format!(
            "tvserver-{test_name}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"legitimate snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let seal = store.seal_private_snapshot(&snapshot).await.unwrap();
        let snapshot_name = snapshot.path.file_name().unwrap().to_owned();
        let snapshot_directory = root.join(PRIVATE_SNAPSHOT_DIRECTORY);
        let original_directory = root.join("original-private-snapshots");
        std::fs::rename(&snapshot_directory, &original_directory).unwrap();
        std::fs::create_dir(&snapshot_directory).unwrap();
        let decoy = snapshot_directory.join(&snapshot_name);
        std::fs::write(&decoy, b"decoy bytes must be preserved").unwrap();
        let original = original_directory.join(snapshot_name);

        (base, root, store, snapshot, seal, original, decoy)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_sealing_uses_creation_directory_after_real_directory_replacement() {
        let (base, _root, store, snapshot, expected, original, decoy) =
            replaced_private_snapshot_directory("snapshot-dir-seal-real").await;

        let actual = store.seal_private_snapshot(&snapshot).await.unwrap();

        assert_eq!(actual, expected);
        assert_eq!(std::fs::read(&original).unwrap(), b"legitimate snapshot bytes");
        assert_eq!(std::fs::read(&decoy).unwrap(), b"decoy bytes must be preserved");
        store.remove_private_snapshot(&snapshot).await.unwrap();
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_publication_uses_and_cleans_original_after_real_directory_replacement() {
        let (base, root, store, snapshot, seal, original, decoy) =
            replaced_private_snapshot_directory("snapshot-dir-publish-real").await;
        let destination = root.join("Dune.epub");

        store
            .publish_private_snapshot_no_replace(
                &snapshot,
                destination.to_str().unwrap(),
                &seal,
            )
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"legitimate snapshot bytes");
        assert!(!original.exists());
        assert_eq!(std::fs::read(&decoy).unwrap(), b"decoy bytes must be preserved");
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_removal_cleans_original_after_real_directory_replacement() {
        let (base, _root, store, snapshot, _seal, original, decoy) =
            replaced_private_snapshot_directory("snapshot-dir-remove-real").await;

        store.remove_private_snapshot(&snapshot).await.unwrap();

        assert!(!original.exists());
        assert_eq!(std::fs::read(&decoy).unwrap(), b"decoy bytes must be preserved");
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_removal_finds_owned_inode_after_basename_replacement() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-snapshot-name-replacement-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"owned snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let snapshot_directory = snapshot.path.parent().unwrap().to_path_buf();
        let owned_metadata = std::fs::metadata(&snapshot.path).unwrap();
        let renamed_owned = snapshot_directory.join("renamed-owned-snapshot");
        let second_owned_link = snapshot_directory.join("second-owned-link");
        std::fs::rename(&snapshot.path, &renamed_owned).unwrap();
        std::fs::hard_link(&renamed_owned, &second_owned_link).unwrap();
        std::fs::write(&snapshot.path, b"decoy must be preserved").unwrap();

        store.remove_private_snapshot(&snapshot).await.unwrap();

        assert_eq!(
            std::fs::read(&snapshot.path).unwrap(),
            b"decoy must be preserved"
        );
        assert!(std::fs::read_dir(&snapshot_directory).unwrap().all(|entry| {
            let metadata = entry.unwrap().metadata().unwrap();
            cap_fs_ext::MetadataExt::dev(&metadata)
                != cap_fs_ext::MetadataExt::dev(&owned_metadata)
                || cap_fs_ext::MetadataExt::ino(&metadata)
                    != cap_fs_ext::MetadataExt::ino(&owned_metadata)
        }));
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_quarantine_preserves_decoy_swapped_after_identity_scan() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-snapshot-cleanup-race-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"owned snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let authority = store.private_snapshot_authority(&snapshot).unwrap();
        let scanned_name = private_snapshot_identity_names(&authority).unwrap().remove(0);
        let renamed_owned = snapshot
            .path
            .parent()
            .unwrap()
            .join("owned-moved-after-scan");

        std::fs::rename(&snapshot.path, &renamed_owned).unwrap();
        std::fs::write(&snapshot.path, b"late decoy must be preserved").unwrap();
        quarantine_private_snapshot_candidate(&authority, &scanned_name).unwrap();

        assert_eq!(
            std::fs::read(&snapshot.path).unwrap(),
            b"late decoy must be preserved"
        );
        assert_eq!(
            std::fs::read(&renamed_owned).unwrap(),
            b"owned snapshot bytes"
        );

        store.remove_private_snapshot(&snapshot).await.unwrap();

        assert_eq!(
            std::fs::read(&snapshot.path).unwrap(),
            b"late decoy must be preserved"
        );
        assert!(!renamed_owned.exists());
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn private_snapshot_quarantine_never_replaces_a_destination_collision() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-snapshot-quarantine-collision-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"owned snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let authority = store.private_snapshot_authority(&snapshot).unwrap();
        let (_quarantine_name, quarantine) =
            create_private_snapshot_cleanup_directory(&authority).unwrap();
        quarantine
            .directory
            .write("candidate", b"unowned collision")
            .unwrap();

        let result = rename_no_replace_relative(
            &authority.directory,
            &authority.name,
            &quarantine.directory,
            Path::new("candidate"),
        );

        assert!(result.is_err());
        assert_eq!(
            authority.directory.read(&authority.name).unwrap(),
            b"owned snapshot bytes"
        );
        assert_eq!(
            quarantine.directory.read("candidate").unwrap(),
            b"unowned collision"
        );
        quarantine.directory.remove_file("candidate").unwrap();
        remove_private_snapshot_cleanup_directory_if_empty(quarantine).unwrap();
        store.remove_private_snapshot(&snapshot).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn private_snapshot_quarantine_rejects_created_directory_replacement_without_orphaning() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-snapshot-quarantine-directory-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"owned snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let authority = store.private_snapshot_authority(&snapshot).unwrap();
        let renamed_created = PathBuf::from("renamed-created-quarantine");
        let mut created_name = None;

        let result = create_private_snapshot_cleanup_directory_with_hook(
            &authority,
            |directory, name| {
                created_name = Some(name.to_path_buf());
                directory.rename(name, directory, &renamed_created).unwrap();
                directory.create_dir(name).unwrap();
                directory
                    .write(name.join("decoy-marker"), b"decoy directory")
                    .unwrap();
            },
        );

        let created_name = created_name.unwrap();
        assert!(result.is_err());
        assert!(authority
            .directory
            .symlink_metadata(&renamed_created)
            .is_err());
        assert_eq!(
            authority
                .directory
                .read(created_name.join("decoy-marker"))
                .unwrap(),
            b"decoy directory"
        );
        authority
            .directory
            .remove_file(created_name.join("decoy-marker"))
            .unwrap();
        authority.directory.remove_dir(&created_name).unwrap();
        store.remove_private_snapshot(&snapshot).await.unwrap();
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_quarantine_rejects_created_directory_symlink_without_redirection() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-snapshot-quarantine-symlink-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"owned snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let authority = store.private_snapshot_authority(&snapshot).unwrap();
        let snapshot_directory = snapshot.path.parent().unwrap().to_path_buf();
        let decoy_target = snapshot_directory.join("quarantine-decoy-target");
        std::fs::create_dir(&decoy_target).unwrap();
        std::fs::write(decoy_target.join("marker"), b"untouched decoy target").unwrap();
        let renamed_created = snapshot_directory.join("renamed-created-quarantine");
        let mut created_path = None;

        let result = create_private_snapshot_cleanup_directory_with_hook(
            &authority,
            |_directory, name| {
                let path = snapshot_directory.join(name);
                created_path = Some(path.clone());
                std::fs::rename(&path, &renamed_created).unwrap();
                symlink(&decoy_target, &path).unwrap();
            },
        );

        let created_path = created_path.unwrap();
        assert!(result.is_err());
        assert!(!renamed_created.exists());
        assert!(created_path
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read(decoy_target.join("marker")).unwrap(),
            b"untouched decoy target"
        );
        std::fs::remove_file(&created_path).unwrap();
        std::fs::remove_dir_all(&decoy_target).unwrap();
        store.remove_private_snapshot(&snapshot).await.unwrap();
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn private_snapshot_empty_quarantine_is_removed_by_consuming_its_open_handle() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-snapshot-quarantine-open-handle-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"owned snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let authority = store.private_snapshot_authority(&snapshot).unwrap();
        let (quarantine_name, quarantine) =
            create_private_snapshot_cleanup_directory(&authority).unwrap();
        let quarantine_path = snapshot.path.parent().unwrap().join(quarantine_name);

        assert!(quarantine_path.is_dir());
        remove_private_snapshot_cleanup_directory_if_empty(quarantine).unwrap();
        assert!(!quarantine_path.exists());

        store.remove_private_snapshot(&snapshot).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_cleanup_error_retires_registry_token() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!(
            "tvserver-snapshot-cleanup-error-token-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"owned snapshot bytes").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let snapshot_directory = snapshot.path.parent().unwrap();
        let original_permissions = std::fs::metadata(snapshot_directory).unwrap().permissions();
        std::fs::set_permissions(snapshot_directory, std::fs::Permissions::from_mode(0o500))
            .unwrap();

        let result = store.remove_private_snapshot(&snapshot).await;

        std::fs::set_permissions(snapshot_directory, original_permissions).unwrap();
        assert!(result.is_err());
        assert!(snapshot.path.exists());
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_directory_symlink_rejects_sealing_and_preserves_decoy() {
        let (base, _root, store, snapshot, _seal, decoy) =
            swapped_private_snapshot_directory("snapshot-dir-seal-symlink").await;

        let result = store.seal_private_snapshot(&snapshot).await;
        let decoy_bytes = std::fs::read(&decoy).ok();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_err());
        assert_eq!(decoy_bytes.as_deref(), Some(b"legitimate snapshot bytes".as_slice()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_directory_symlink_rejects_publication_and_preserves_decoy() {
        let (base, root, store, snapshot, seal, decoy) =
            swapped_private_snapshot_directory("snapshot-dir-publish-symlink").await;
        let destination = root.join("Dune.epub");

        let result = store
            .publish_private_snapshot_no_replace(
                &snapshot,
                destination.to_str().unwrap(),
                &seal,
            )
            .await;
        let destination_exists = destination.exists();
        let decoy_bytes = std::fs::read(&decoy).ok();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_err());
        assert!(!destination_exists);
        assert_eq!(decoy_bytes.as_deref(), Some(b"legitimate snapshot bytes".as_slice()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_directory_symlink_does_not_block_retained_cleanup() {
        let (base, root, store, snapshot, _seal, decoy) =
            swapped_private_snapshot_directory("snapshot-dir-remove-symlink").await;
        let original = root
            .join("original-private-snapshots")
            .join(snapshot.path.file_name().unwrap());

        store.remove_private_snapshot(&snapshot).await.unwrap();

        assert!(!original.exists());
        assert_eq!(std::fs::read(&decoy).unwrap(), b"legitimate snapshot bytes");
        assert!(root
            .join(PRIVATE_SNAPSHOT_DIRECTORY)
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn stage_no_follow_moves_one_regular_identity_to_an_unguessable_sibling() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-stage-regular-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"stable book").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();

        assert_eq!(staged.original_path, source);
        assert!(!source.exists());
        assert_eq!(std::fs::read(&staged.staged_path).unwrap(), b"stable book");
        assert_eq!(staged.staged_path.parent(), source.parent());
        assert!(staged
            .staged_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".tvserver-ingest-"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stage_no_follow_rejects_a_source_symlink_without_reading_its_target() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-stage-symlink-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let target = base.join("secret.epub");
        let source = base.join("Dune.epub");
        std::fs::write(&target, b"must never be processed").unwrap();
        symlink(&target, &source).unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store.stage_no_follow(source.to_str().unwrap()).await;

        assert!(result.is_err());
        assert!(source.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"must never be processed");
        assert!(std::fs::read_dir(&base)
            .unwrap()
            .all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".tvserver-ingest-")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_has_a_new_inode_and_ignores_retained_source_fd_writes() {
        use std::io::{Seek, SeekFrom, Write};

        let base = std::env::temp_dir().join(format!(
            "tvserver-private-snapshot-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"snapshot bytes").unwrap();
        let mut downloader_fd = std::fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();

        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        downloader_fd.seek(SeekFrom::Start(0)).unwrap();
        downloader_fd.write_all(b"changed through").unwrap();
        downloader_fd.flush().unwrap();

        assert_ne!(
            std::os::unix::fs::MetadataExt::ino(
                &std::fs::metadata(&staged.staged_path).unwrap(),
            ),
            std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&snapshot.path).unwrap())
        );
        assert_eq!(std::fs::read(&snapshot.path).unwrap(), b"snapshot bytes");
        assert!(snapshot.path.starts_with(&root));
        assert!(snapshot
            .path
            .parent()
            .unwrap()
            .file_name()
            .is_some_and(|name| name == ".tvserver-book-snapshots"));

        store.remove_private_snapshot(&snapshot).await.unwrap();
        store.restore_staged(&staged).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn private_snapshot_rejects_staged_symlink_without_reading_target() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-private-snapshot-symlink-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        let target = base.join("secret.epub");
        std::fs::write(&source, b"original").unwrap();
        std::fs::write(&target, b"must never be copied").unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        std::fs::remove_file(&staged.staged_path).unwrap();
        symlink(&target, &staged.staged_path).unwrap();

        let result = store.create_private_snapshot(&staged).await;

        assert!(result.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"must never be copied");
        let snapshot_dir = root.join(".tvserver-book-snapshots");
        assert!(!snapshot_dir.exists() || std::fs::read_dir(snapshot_dir).unwrap().next().is_none());
        assert!(store.private_snapshots.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn verified_snapshot_publication_rejects_post_seal_mutation() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-verified-snapshot-mutation-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"sealed bytes").unwrap();
        let destination = root.join("Dune.epub");
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let seal = store.seal_private_snapshot(&snapshot).await.unwrap();
        std::fs::write(&snapshot.path, b"mutated byte").unwrap();

        let error = store
            .publish_private_snapshot_no_replace(
                &snapshot,
                destination.to_str().unwrap(),
                &seal,
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("integrity"), "{error:#}");
        assert!(!destination.exists());
        assert_eq!(std::fs::read(&snapshot.path).unwrap(), b"mutated byte");
        store.remove_private_snapshot(&snapshot).await.unwrap();
        store.restore_staged(&staged).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn verified_snapshot_publication_uses_a_distinct_destination_inode() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-verified-snapshot-inode-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        std::fs::write(&source, b"sealed bytes").unwrap();
        let destination = root.join("Dune.epub");
        let store = FileSystemStore::new(root.to_str().unwrap());
        let staged = store.stage_no_follow(source.to_str().unwrap()).await.unwrap();
        let snapshot = store.create_private_snapshot(&staged).await.unwrap();
        let seal = store.seal_private_snapshot(&snapshot).await.unwrap();
        let snapshot_inode = std::os::unix::fs::MetadataExt::ino(
            &std::fs::metadata(&snapshot.path).unwrap(),
        );

        store
            .publish_private_snapshot_no_replace(
                &snapshot,
                destination.to_str().unwrap(),
                &seal,
            )
            .await
            .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"sealed bytes");
        assert!(!snapshot.path.exists());
        assert_ne!(
            snapshot_inode,
            std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&destination).unwrap())
        );
        store.restore_staged(&staged).await.unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remove_regular_no_follow_handles_ambient_files_and_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-remove-ambient-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("covers");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let regular = outside.join("generated.jpg");
        let target = outside.join("target.jpg");
        let link = outside.join("link.jpg");
        std::fs::write(&regular, b"generated").unwrap();
        std::fs::write(&target, b"preserve").unwrap();
        symlink(&target, &link).unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        store.remove_regular_no_follow(&regular).await.unwrap();
        let link_result = store.remove_regular_no_follow(&link).await;

        assert!(!regular.exists());
        assert!(link_result.is_err());
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"preserve");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn rename_no_replace_exdev_seam_publishes_and_removes_source_end_to_end() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-exdev-publish-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        let destination = root.join("Dune.epub");
        std::fs::write(&source, b"cross-device bytes").unwrap();
        let store = FileSystemStore::new_with_move_filesystem(
            root.to_str().unwrap(),
            Arc::new(ForcedCrossDevice),
        );

        store
            .rename_no_replace(source.to_str().unwrap(), destination.to_str().unwrap())
            .await
            .unwrap();

        assert!(!source.exists());
        assert_eq!(std::fs::read(&destination).unwrap(), b"cross-device bytes");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn rename_no_replace_exdev_seam_restores_source_on_collision_and_cleans_temp() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-exdev-collision-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let source = base.join("Dune.epub");
        let destination = root.join("Dune.epub");
        std::fs::write(&source, b"incoming").unwrap();
        std::fs::write(&destination, b"existing").unwrap();
        let store = FileSystemStore::new_with_move_filesystem(
            root.to_str().unwrap(),
            Arc::new(ForcedCrossDevice),
        );

        let result = store
            .rename_no_replace(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;

        assert!(result.is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"incoming");
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn test_list_directory() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        let results = store.list_folder("").await?;

        assert_eq!(
            results.0,
            vec![".thumbnails", "TV", "collection1", "collection2"]
        );
        assert_eq!(
            results.1,
            vec!["empty.mp4", "test.jpg", "test.mp4", "test.png", "test.py"]
        );

        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn directory_entry_name_rejects_non_utf8_names() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let result = directory_entry_name(OsString::from_vec(vec![b'b', 0xff]));

        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_folder_skips_symlinks_to_files_and_directories() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-list-symlinks-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("real.pdf"), b"book").unwrap();
        std::fs::create_dir(root.join("real-directory")).unwrap();
        std::fs::write(outside.join("outside.pdf"), b"outside").unwrap();
        symlink(&outside, root.join("linked-directory")).unwrap();
        symlink(outside.join("outside.pdf"), root.join("linked-file.pdf")).unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let listing = store.list_folder("").await.unwrap();

        assert_eq!(listing.0, vec!["real-directory"]);
        assert_eq!(listing.1, vec!["real.pdf"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn test_list_directory_that_does_not_exists() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        if let Ok(_) = store.list_folder("not here").await {
            panic!("{}", "expected call to fail");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_ensure_directory_exists() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        let mut path = PathBuf::from(TEST_DIR);

        path.push("TV");
        path.push("does not exist");

        if path.exists() {
            fs::remove_dir_all(&path).await?;
        }

        assert!(!path.exists());

        store.ensure_path_exists("TV/does not exist").await?;

        assert!(path.exists());

        fs::remove_dir_all(&path).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_rename() -> Result<()> {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);

        let mut path = PathBuf::from(TEST_DIR);

        path.push("collection1");

        let results = store.list_folder("collection1").await?;

        assert!(results.1.len() > 0);

        let existing = results.1.first().unwrap();

        let new_name = String::from("new file name.mp4");

        assert!(!results.1.contains(&new_name));

        let mut source_path = path.clone();
        source_path.push(existing);

        let mut dest_path = path.clone();
        dest_path.push(&new_name);

        store
            .rename(source_path.to_str().unwrap(), dest_path.to_str().unwrap())
            .await?;

        let results = store.list_folder(path.to_str().unwrap()).await?;

        assert!(results.1.contains(&new_name));

        store
            .rename(dest_path.to_str().unwrap(), source_path.to_str().unwrap())
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn rename_no_replace_preserves_existing_destination_and_source() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-no-replace-existing-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        fs::create_dir_all(&root).await.unwrap();
        let source = base.join("incoming.epub");
        let destination = root.join("book.epub");
        fs::write(&source, b"incoming bytes").await.unwrap();
        fs::write(&destination, b"existing bytes").await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store
            .rename_no_replace(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;
        let source_contents = fs::read(&source).await.unwrap();
        let destination_contents = fs::read(&destination).await.unwrap();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert_eq!(source_contents, b"incoming bytes");
        assert_eq!(destination_contents, b"existing bytes");
    }

    #[tokio::test]
    async fn concurrent_rename_no_replace_calls_have_exactly_one_winner() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-no-replace-concurrent-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        fs::create_dir_all(&root).await.unwrap();
        let first = base.join("first.epub");
        let second = base.join("second.epub");
        let destination = root.join("book.epub");
        fs::write(&first, b"first bytes").await.unwrap();
        fs::write(&second, b"second bytes").await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let (first_result, second_result) = tokio::join!(
            store.rename_no_replace(first.to_str().unwrap(), destination.to_str().unwrap()),
            store.rename_no_replace(second.to_str().unwrap(), destination.to_str().unwrap())
        );
        let destination_contents = fs::read(&destination).await.unwrap();

        match (first_result, second_result) {
            (Ok(()), Err(_)) => {
                assert!(!first.exists());
                assert_eq!(fs::read(&second).await.unwrap(), b"second bytes");
                assert_eq!(destination_contents, b"first bytes");
            }
            (Err(_), Ok(())) => {
                assert_eq!(fs::read(&first).await.unwrap(), b"first bytes");
                assert!(!second.exists());
                assert_eq!(destination_contents, b"second bytes");
            }
            results => panic!("expected exactly one no-replace winner, got {results:?}"),
        }
        let _ = fs::remove_dir_all(&base).await;
    }

    #[tokio::test]
    async fn case_equivalent_no_replace_destinations_have_one_winner_when_supported() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-no-replace-case-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        fs::create_dir_all(&root).await.unwrap();
        let probe = root.join("CaseProbe");
        fs::write(&probe, b"probe").await.unwrap();
        let is_case_insensitive = root.join("caseprobe").exists();
        fs::remove_file(&probe).await.unwrap();
        if !is_case_insensitive {
            let _ = fs::remove_dir_all(&base).await;
            return;
        }

        let first = base.join("first.epub");
        let second = base.join("second.epub");
        let first_destination = root.join("Book.epub");
        let second_destination = root.join("book.epub");
        fs::write(&first, b"first bytes").await.unwrap();
        fs::write(&second, b"second bytes").await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let (first_result, second_result) = tokio::join!(
            store.rename_no_replace(first.to_str().unwrap(), first_destination.to_str().unwrap()),
            store.rename_no_replace(second.to_str().unwrap(), second_destination.to_str().unwrap())
        );

        assert!(matches!(
            (first_result, second_result),
            (Ok(()), Err(_)) | (Err(_), Ok(()))
        ));
        let _ = fs::remove_dir_all(&base).await;
    }

    #[tokio::test]
    async fn rename_still_replaces_an_existing_destination() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-rename-replace-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        fs::create_dir_all(&root).await.unwrap();
        let source = base.join("incoming.mp4");
        let destination = root.join("video.mp4");
        fs::write(&source, b"incoming video").await.unwrap();
        fs::write(&destination, b"old video").await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        store
            .rename(source.to_str().unwrap(), destination.to_str().unwrap())
            .await
            .unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(&destination).await.unwrap(), b"incoming video");
        let _ = fs::remove_dir_all(&base).await;
    }

    #[test]
    fn test_path_traversal_rejected() {
        let store = FileSystemStore::new(TEST_DIR);
        assert!(store.get_real_path("../../etc/passwd").is_err());
        assert!(store.get_real_path("subdir/../../../etc/shadow").is_err());
    }

    #[test]
    fn test_valid_paths_accepted() {
        let store = FileSystemStore::new(TEST_DIR);
        assert!(store.get_real_path("collection1/file.mp4").is_ok());
        assert!(store.get_real_path("").is_ok());
        // Relative parent within root should be fine
        assert!(store.get_real_path("collection1/../collection2").is_ok());
    }

    #[tokio::test]
    async fn test_delete_path_traversal() {
        let store: &dyn FileStore = &FileSystemStore::new(TEST_DIR);
        let result = store.delete("../../etc/passwd").await;
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn delete_rejects_dangling_symlink_without_removing_it() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-delete-link-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        fs::create_dir_all(&root).await.unwrap();
        let link = root.join("Dune.epub");
        symlink(base.join("missing.epub"), &link).unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store.delete(link.to_str().unwrap()).await;
        let link_exists = link.symlink_metadata().is_ok();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(link_exists);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rename_does_not_follow_destination_parent_symlink() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-rename-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        symlink(&outside, root.join("Fiction")).unwrap();
        let source = base.join("Dune.epub");
        fs::write(&source, b"book").await.unwrap();
        let destination = root.join("Fiction/Dune.epub");
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store
            .rename(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;
        let source_exists = source.exists();
        let outside_exists = outside.join("Dune.epub").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(source_exists);
        assert!(!outside_exists);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rename_stays_anchored_when_store_root_is_replaced() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-root-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let original_root = base.join("original-root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        fs::rename(&root, &original_root).await.unwrap();
        symlink(&outside, &root).unwrap();
        let source = base.join("Dune.epub");
        fs::write(&source, b"book").await.unwrap();
        let destination = root.join("Dune.epub");

        let result = store
            .rename(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;
        let outside_exists = outside.join("Dune.epub").exists();
        let preserved = result.is_err() && source.exists()
            || result.is_ok() && original_root.join("Dune.epub").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(!outside_exists);
        assert!(preserved);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rename_uses_root_capability_for_internal_source() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-source-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let original_collection = base.join("original-fiction");
        let outside = base.join("outside");
        fs::create_dir_all(root.join("Fiction")).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        fs::write(root.join("Fiction/Dune.epub"), b"inside")
            .await
            .unwrap();
        fs::write(outside.join("Dune.epub"), b"outside")
            .await
            .unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        fs::rename(root.join("Fiction"), &original_collection)
            .await
            .unwrap();
        symlink(&outside, root.join("Fiction")).unwrap();
        let source = root.join("Fiction/Dune.epub");
        let destination = root.join("quarantine.epub");

        let result = store
            .rename(source.to_str().unwrap(), destination.to_str().unwrap())
            .await;
        let outside_contents = fs::read(outside.join("Dune.epub")).await.ok();
        let quarantined_contents = fs::read(&destination).await.ok();
        let original_contents = fs::read(original_collection.join("Dune.epub")).await.ok();
        let _ = fs::remove_dir_all(&base).await;

        assert_eq!(outside_contents.as_deref(), Some(b"outside".as_slice()));
        assert!(
            result.is_err() && original_contents.as_deref() == Some(b"inside".as_slice())
                || result.is_ok()
                    && quarantined_contents.as_deref() == Some(b"inside".as_slice())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn create_folder_does_not_follow_parent_symlink() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-create-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).await.unwrap();
        fs::create_dir(&outside).await.unwrap();
        symlink(&outside, root.join("Fiction")).unwrap();
        let destination = root.join("Fiction/Classics");
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store.create_folder(&destination).await;
        let outside_exists = outside.join("Classics").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(!outside_exists);
    }

    #[tokio::test]
    async fn create_folder_creates_missing_store_root() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-create-root-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let destination = root.join("Fiction");
        let store = FileSystemStore::new(root.to_str().unwrap());

        let result = store.create_folder(&destination).await;
        let destination_exists = destination.is_dir();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_ok(), "{result:?}");
        assert!(destination_exists);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn create_folder_rejects_symlink_installed_at_missing_root() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-missing-root-swap-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&outside).await.unwrap();
        let store = FileSystemStore::new(root.to_str().unwrap());
        if root.exists() {
            fs::remove_dir(&root).await.unwrap();
        }
        symlink(&outside, &root).unwrap();
        let destination = root.join("Fiction");

        let result = store.create_folder(&destination).await;
        let outside_exists = outside.join("Fiction").exists();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_err());
        assert!(!outside_exists);
    }

    #[tokio::test]
    async fn create_folder_accepts_normalized_store_root() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-normalized-root-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(base.join("parent")).await.unwrap();
        let configured_root = base.join("parent/../root");
        let destination = base.join("root/Fiction");
        let store = FileSystemStore::new(configured_root.to_str().unwrap());

        let result = store.create_folder(&destination).await;
        let destination_exists = destination.is_dir();
        let _ = fs::remove_dir_all(&base).await;

        assert!(result.is_ok(), "{result:?}");
        assert!(destination_exists);
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_copy_replaces_destination_symlink_without_following_it() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-copy-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir(&destination_root).unwrap();
        std::fs::write(source_root.join("staged.epub"), b"book").unwrap();
        let outside = base.join("outside.epub");
        std::fs::write(&outside, b"outside").unwrap();
        symlink(&outside, destination_root.join("Dune.epub")).unwrap();
        let source_dir = Dir::open_ambient_dir(&source_root, ambient_authority()).unwrap();
        let destination_dir =
            Dir::open_ambient_dir(&destination_root, ambient_authority()).unwrap();

        let result = copy_staged_file(
            &source_dir,
            Path::new("staged.epub"),
            &destination_dir,
            Path::new("Dune.epub"),
        );
        let outside_contents = std::fs::read(&outside).unwrap();
        let destination_contents = std::fs::read(destination_root.join("Dune.epub")).unwrap();
        let source_exists = source_root.join("staged.epub").exists();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_ok());
        assert_eq!(outside_contents, b"outside");
        assert_eq!(destination_contents, b"book");
        assert!(!source_exists);
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_copy_is_committed_after_destination_publication() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-copy-commit-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir(&destination_root).unwrap();
        std::fs::write(source_root.join("staged.epub"), b"book").unwrap();
        let source_dir = Dir::open_ambient_dir(&source_root, ambient_authority()).unwrap();
        let destination_dir =
            Dir::open_ambient_dir(&destination_root, ambient_authority()).unwrap();
        std::fs::set_permissions(&source_root, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = copy_staged_file(
            &source_dir,
            Path::new("staged.epub"),
            &destination_dir,
            Path::new("Dune.epub"),
        );
        std::fs::set_permissions(&source_root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let destination_contents = std::fs::read(destination_root.join("Dune.epub")).unwrap();
        let source_exists = source_root.join("staged.epub").exists();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_ok());
        assert_eq!(destination_contents, b"book");
        assert!(source_exists);
    }

    #[test]
    fn cross_device_no_replace_copy_preserves_collision_and_cleans_temporary_file() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-no-replace-copy-collision-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir(&destination_root).unwrap();
        std::fs::write(source_root.join("staged.epub"), b"incoming").unwrap();
        std::fs::write(destination_root.join("Dune.epub"), b"existing").unwrap();
        let source_dir = Dir::open_ambient_dir(&source_root, ambient_authority()).unwrap();
        let destination_dir =
            Dir::open_ambient_dir(&destination_root, ambient_authority()).unwrap();

        let result = copy_staged_file_no_replace(
            &source_dir,
            Path::new("staged.epub"),
            &destination_dir,
            Path::new("Dune.epub"),
        );
        let destination_entries = std::fs::read_dir(&destination_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();

        assert!(result.is_err());
        assert_eq!(std::fs::read(source_root.join("staged.epub")).unwrap(), b"incoming");
        assert_eq!(
            std::fs::read(destination_root.join("Dune.epub")).unwrap(),
            b"existing"
        );
        assert_eq!(destination_entries, [OsString::from("Dune.epub")]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn cross_device_no_replace_copy_is_committed_after_destination_publication() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!(
            "tvserver-no-replace-copy-commit-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let source_root = base.join("source");
        let destination_root = base.join("destination");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir(&destination_root).unwrap();
        std::fs::write(source_root.join("staged.epub"), b"book").unwrap();
        let source_dir = Dir::open_ambient_dir(&source_root, ambient_authority()).unwrap();
        let destination_dir =
            Dir::open_ambient_dir(&destination_root, ambient_authority()).unwrap();
        std::fs::set_permissions(&source_root, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = copy_staged_file_no_replace(
            &source_dir,
            Path::new("staged.epub"),
            &destination_dir,
            Path::new("Dune.epub"),
        );
        std::fs::set_permissions(&source_root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let destination_contents = std::fs::read(destination_root.join("Dune.epub")).unwrap();
        let source_exists = source_root.join("staged.epub").exists();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_ok());
        assert_eq!(destination_contents, b"book");
        assert!(source_exists);
    }

    #[tokio::test]
    async fn restore_does_not_overwrite_replacement() {
        let base = std::env::temp_dir().join(format!(
            "tvserver-rooted-restore-collision-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("staged.epub"), b"original").unwrap();
        std::fs::write(base.join("Dune.epub"), b"replacement").unwrap();
        let store = FileSystemStore::new(base.to_str().unwrap());

        let result = store
            .restore(
                base.join("staged.epub").to_str().unwrap(),
                base.join("Dune.epub").to_str().unwrap(),
            )
            .await;
        let original = std::fs::read(base.join("staged.epub")).unwrap();
        let replacement = std::fs::read(base.join("Dune.epub")).unwrap();
        let _ = std::fs::remove_dir_all(&base);

        assert!(result.is_err());
        assert_eq!(original, b"original");
        assert_eq!(replacement, b"replacement");
    }

    // TODO,rename ensure_path_exists -> ensure_directory_exists and test when there is a file
    // with the same name as the desired directory. Test rename with non-existent destination
    // directory (should be created) and non-existent source, which should fail.
}
