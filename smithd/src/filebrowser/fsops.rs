//! Filesystem primitives for the remote browser.
//!
//! smithd runs as root, so there is deliberately no path allowlist here: an
//! operator holding `commands:files` can already read anything via `FreeForm`.
//! The guards below exist for a different reason — keeping the daemon alive.
//! Opening a FIFO blocks forever, `/dev/zero` never ends, and `/proc/kcore`
//! claims to be 128 TiB. Each of those would wedge or balloon a transfer, so
//! they are refused by shape rather than by name.

use crate::utils::schema::{DirEntryInfo, FileKind, FileOpError};
use nix::libc;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// Largest file the browser will transfer. Chosen so the transport can actually
/// satisfy it: at a typical device uplink a larger file takes hours, and the
/// transfer has no resume. Exactly matches the api's `DefaultBodyLimit` on the
/// smith router, so a file that passes here is never rejected on arrival.
pub const MAX_DOWNLOAD_BYTES: u64 = 512_000_000;

/// Entries returned for a single directory before `truncated` is set. Bounds
/// both the daemon's memory and the size of a relayed listing.
pub const MAX_LIST_ENTRIES: usize = 5000;

/// A file resolved, validated and held open. Holding the descriptor is what
/// makes the later transfer free of a time-of-check/time-of-use race: the bytes
/// streamed are the bytes that were validated, even if the path is replaced.
#[derive(Debug)]
pub struct OpenedFile {
    pub file: File,
    pub name: String,
    pub size: u64,
}

pub fn map_io_error(err: &io::Error) -> FileOpError {
    match err.raw_os_error() {
        Some(libc::ENOENT) => FileOpError::NotFound,
        Some(libc::EACCES) | Some(libc::EPERM) => FileOpError::PermissionDenied,
        Some(libc::ENOTDIR) => FileOpError::NotADirectory,
        Some(libc::EISDIR) => FileOpError::NotRegularFile,
        Some(libc::EMFILE) | Some(libc::ENFILE) => FileOpError::TooManyOpenFiles,
        // O_NOFOLLOW on a symlink reports ELOOP. Treated as "not a regular
        // file" because that is what the operator needs to know.
        Some(libc::ELOOP) => FileOpError::NotRegularFile,
        _ => FileOpError::Io,
    }
}

fn kind_of(metadata: &std::fs::Metadata) -> FileKind {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        FileKind::Symlink
    } else if file_type.is_dir() {
        FileKind::Dir
    } else if file_type.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    }
}

/// Canonicalize so the caller's breadcrumb reflects where it actually landed
/// after resolving symlinks, and so a path is reported one consistent way.
fn resolve(path: &str) -> Result<PathBuf, FileOpError> {
    if path.contains('\0') {
        return Err(FileOpError::NotFound);
    }
    if !Path::new(path).is_absolute() {
        return Err(FileOpError::NotFound);
    }
    std::fs::canonicalize(path).map_err(|e| map_io_error(&e))
}

/// List a directory using `lstat` only. Nothing here opens a file, so a FIFO or
/// device node sitting in the directory cannot block the caller.
pub fn list_dir(path: &str) -> Result<(String, Vec<DirEntryInfo>, bool), FileOpError> {
    let canonical = resolve(path)?;

    let metadata = std::fs::symlink_metadata(&canonical).map_err(|e| map_io_error(&e))?;
    if !metadata.is_dir() {
        return Err(FileOpError::NotADirectory);
    }

    let reader = std::fs::read_dir(&canonical).map_err(|e| map_io_error(&e))?;

    let mut entries = Vec::new();
    let mut truncated = false;

    for entry in reader {
        if entries.len() >= MAX_LIST_ENTRIES {
            truncated = true;
            break;
        }

        // A single unreadable entry must not fail the whole listing — a
        // directory being mutated underneath us is normal.
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                tracing::debug!("Skipping unreadable directory entry: {e}");
                continue;
            }
        };

        // `DirEntry::metadata` does not traverse symlinks, which is what we
        // want: report the link itself, not its target.
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(e) => {
                tracing::debug!("Skipping entry with unreadable metadata: {e}");
                continue;
            }
        };

        let raw_name = entry.file_name();
        // Linux filenames are bytes. A lossy conversion would not round-trip
        // back to the same file, so the entry is shown but marked unreachable
        // rather than silently omitted.
        let (name, reachable) = match raw_name.to_str() {
            Some(name) => (name.to_string(), true),
            None => (raw_name.to_string_lossy().into_owned(), false),
        };

        let kind = kind_of(&metadata);
        let symlink_target = if kind == FileKind::Symlink {
            std::fs::read_link(entry.path())
                .ok()
                .map(|target| target.to_string_lossy().into_owned())
        } else {
            None
        };

        entries.push(DirEntryInfo {
            name,
            kind,
            size: metadata.size(),
            mtime: Some(metadata.mtime()),
            mode: metadata.mode() & 0o7777,
            uid: metadata.uid(),
            gid: metadata.gid(),
            symlink_target,
            reachable,
        });
    }

    entries.sort_by(
        |a, b| match (a.kind == FileKind::Dir, b.kind == FileKind::Dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        },
    );

    Ok((canonical.to_string_lossy().into_owned(), entries, truncated))
}

/// Open a file for transfer, refusing anything that could hang or never end.
pub fn open_file(path: &str) -> Result<OpenedFile, FileOpError> {
    let canonical = resolve(path)?;

    let file = OpenOptions::new()
        .read(true)
        // O_NONBLOCK is a no-op on regular files, but makes open() on a FIFO
        // with no writer return immediately instead of blocking forever.
        // O_NOFOLLOW stops a final-component symlink swap between the
        // canonicalize above and this open.
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(&canonical)
        .map_err(|e| map_io_error(&e))?;

    // fstat on the descriptor we hold, not a second lookup of the path.
    let metadata = file.metadata().map_err(|e| map_io_error(&e))?;

    if !metadata.is_file() {
        return Err(FileOpError::NotRegularFile);
    }

    let size = metadata.len();
    if size > MAX_DOWNLOAD_BYTES {
        return Err(FileOpError::TooLarge);
    }

    let name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());

    Ok(OpenedFile { file, name, size })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lists_a_directory_with_dirs_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "hello").unwrap();
        std::fs::create_dir(dir.path().join("a_dir")).unwrap();
        std::fs::create_dir(dir.path().join("z_dir")).unwrap();

        let (path, entries, truncated) = list_dir(dir.path().to_str().unwrap()).unwrap();

        assert!(!truncated);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "a_dir");
        assert_eq!(entries[0].kind, FileKind::Dir);
        assert_eq!(entries[1].name, "z_dir");
        assert_eq!(entries[2].name, "b.txt");
        assert_eq!(entries[2].kind, FileKind::File);
        assert_eq!(entries[2].size, 5);
        // The reported path is canonical, which on macOS differs from the
        // tempdir path (/var vs /private/var).
        assert!(path.ends_with(dir.path().file_name().unwrap().to_str().unwrap()));
    }

    #[test]
    fn reports_symlinks_without_following_them() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, "data").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("link")).unwrap();

        let (_, entries, _) = list_dir(dir.path().to_str().unwrap()).unwrap();

        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert_eq!(link.kind, FileKind::Symlink);
        assert!(link.symlink_target.is_some());
    }

    #[test]
    fn lists_a_directory_containing_a_fifo_without_hanging() {
        // The wedge test: listing must never open anything.
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) }, 0);

        let (_, entries, _) = list_dir(dir.path().to_str().unwrap()).unwrap();

        let pipe = entries.iter().find(|e| e.name == "pipe").unwrap();
        assert_eq!(pipe.kind, FileKind::Other);
    }

    #[test]
    fn truncates_a_very_large_directory() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_LIST_ENTRIES + 10) {
            std::fs::write(dir.path().join(format!("f{i}")), "").unwrap();
        }

        let (_, entries, truncated) = list_dir(dir.path().to_str().unwrap()).unwrap();

        assert!(truncated);
        assert_eq!(entries.len(), MAX_LIST_ENTRIES);
    }

    #[test]
    fn rejects_listing_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();

        assert_eq!(
            list_dir(file.to_str().unwrap()).unwrap_err(),
            FileOpError::NotADirectory
        );
    }

    #[test]
    fn rejects_relative_and_missing_paths() {
        assert_eq!(
            list_dir("relative/path").unwrap_err(),
            FileOpError::NotFound
        );
        assert_eq!(
            list_dir("/definitely/not/here/at/all").unwrap_err(),
            FileOpError::NotFound
        );
        assert_eq!(list_dir("/tmp/\0evil").unwrap_err(), FileOpError::NotFound);
    }

    #[test]
    fn opens_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();

        let opened = open_file(path.to_str().unwrap()).unwrap();

        assert_eq!(opened.name, "a.txt");
        assert_eq!(opened.size, 11);
    }

    #[test]
    fn refuses_to_open_a_fifo() {
        // Without O_NONBLOCK this open would block forever with no writer,
        // taking the session's task with it.
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o644) }, 0);

        assert_eq!(
            open_file(fifo.to_str().unwrap()).unwrap_err(),
            FileOpError::NotRegularFile
        );
    }

    #[test]
    fn refuses_to_open_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let err = open_file(dir.path().to_str().unwrap()).unwrap_err();
        // Linux reports EISDIR from open(O_RDONLY) on a directory; macOS
        // succeeds and is caught by the S_ISREG check. Both land here.
        assert_eq!(err, FileOpError::NotRegularFile);
    }

    #[test]
    fn refuses_a_dangling_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("nope"), &link).unwrap();

        assert_eq!(
            open_file(link.to_str().unwrap()).unwrap_err(),
            FileOpError::NotFound
        );
    }

    #[test]
    fn follows_a_symlink_to_a_regular_file() {
        // canonicalize resolves the link, then O_NOFOLLOW applies to the
        // resolved path — so a legitimate symlink still works.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, "payload").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let opened = open_file(link.to_str().unwrap()).unwrap();

        assert_eq!(opened.size, 7);
        assert_eq!(opened.name, "target.txt");
    }

    #[test]
    fn refuses_a_file_over_the_size_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let f = std::fs::File::create(&path).unwrap();
        // Sparse: sets st_size without writing 512 MiB to the test machine.
        f.set_len(MAX_DOWNLOAD_BYTES + 1).unwrap();
        drop(f);

        assert_eq!(
            open_file(path.to_str().unwrap()).unwrap_err(),
            FileOpError::TooLarge
        );
    }

    #[test]
    fn held_descriptor_survives_the_path_being_replaced() {
        // The point of holding the fd: what gets streamed is what was
        // validated, even if the path is swapped afterwards.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "original").unwrap();

        let opened = open_file(path.to_str().unwrap()).unwrap();

        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "replaced-with-something-longer").unwrap();

        let mut contents = String::new();
        use std::io::Read;
        let mut file = opened.file;
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "original");
    }
}
