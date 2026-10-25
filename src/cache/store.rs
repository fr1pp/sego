use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, FileTimes};
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::{Builder, NamedTempFile, PersistError};

pub const MTIME_INTERVAL: Duration = Duration::from_hours(1);

/// No entry name starts with `~`, since names are hex.
pub const TEMP_PREFIX: &str = "~tmp-";

const TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n# This directory is a cache created by sego.\n";

/// A hit is a regular file, not a symlink, holding exactly `size` bytes.
/// The path is checked before the open and the handle after it, so
/// anything else swapped in between is refused once opened.
pub fn read_entry(path: &Path, size: u64) -> Option<Vec<u8>> {
    let hit = |metadata: fs::Metadata| {
        metadata.file_type().is_file() && metadata.len() == size
    };
    if !hit(fs::symlink_metadata(path).ok()?) {
        return None;
    }
    let file = open_entry(path).ok()?;
    if !hit(file.metadata().ok()?) {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(size.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 == size).then_some(bytes)
}

/// On Unix the open refuses a symlink, so one swapped in after the check
/// isn't followed, and doesn't wait for a FIFO's writer. Elsewhere it
/// follows a symlink.
fn open_entry(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use rustix::fs::{Mode, OFlags};
        let flags = OFlags::RDONLY
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC;
        Ok(File::from(rustix::fs::open(path, flags, Mode::empty())?))
    }
    #[cfg(not(unix))]
    File::open(path)
}

/// Errors are ignored: a stale time only shortens the entry's life.
pub(super) fn refresh(path: &Path, now: SystemTime) {
    let Ok(modified) =
        fs::symlink_metadata(path).and_then(|metadata| metadata.modified())
    else {
        return;
    };
    let gap =
        now.duration_since(modified).unwrap_or_else(|error| error.duration());
    if gap > MTIME_INTERVAL {
        let _ =
            fs::set_times_nofollow(path, FileTimes::new().set_modified(now));
    }
}

pub fn write_entry(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_with_prefix(path, bytes, TEMP_PREFIX)
}

pub(super) fn write_with_prefix(
    path: &Path,
    bytes: &[u8],
    prefix: &str,
) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("entry has no directory"))?;
    let mut temp = new_temp(dir, prefix)?;
    temp.as_file_mut().write_all(bytes)?;
    persist(temp, path)
}

pub fn init_root(root: &Path) -> io::Result<()> {
    init_root_with(root, |file| file.write_all(TAG.as_bytes()))
}

/// A root that already exists is used as it is. A missing one is made
/// tagged in a temporary directory beside it and renamed into place, so it
/// never exists untagged.
fn init_root_with(
    root: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    // Existence is checked before the kind: another creator can rename its
    // root into place between the two checks, and a root never goes away.
    if fs::symlink_metadata(root).is_ok() {
        return if root.is_dir() {
            Ok(())
        } else {
            Err(ErrorKind::NotADirectory.into())
        };
    }
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
        return Err(ErrorKind::InvalidInput.into());
    };
    make_dir(parent)?;
    let mut prefix = OsString::from(TEMP_PREFIX);
    prefix.push(name);
    prefix.push("-");
    let mut builder = Builder::new();
    builder.prefix(&prefix);
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    let temp = builder.tempdir_in(parent)?;
    write(&mut File::create_new(temp.path().join("CACHEDIR.TAG"))?)?;
    match rename_new(temp.path(), root) {
        Err(_) if root.is_dir() => Ok(()),
        result => result,
    }
}

/// A rename that fails when `to` exists, so an empty directory another
/// program made there in the meantime is kept. Where the system or the file
/// system can't refuse, a plain rename replaces an empty directory.
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_vendor = "apple"
    ))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        use rustix::io::Errno;
        match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
            Err(Errno::INVAL | Errno::NOSYS | Errno::NOTSUP) => {}
            result => return result.map_err(io::Error::from),
        }
    }
    fs::rename(from, to)
}

fn make_dir(dir: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// The file comes first and the directory only on `NotFound`, because a
/// trimmer may remove the directory whenever it is empty.
fn new_temp(dir: &Path, prefix: &str) -> io::Result<NamedTempFile> {
    let mut attempts = 0;
    loop {
        match Builder::new().prefix(prefix).rand_bytes(12).tempfile_in(dir) {
            Err(error)
                if error.kind() == ErrorKind::NotFound && attempts < 4 =>
            {
                attempts += 1;
                make_dir(dir)?;
            }
            result => return result,
        }
    }
}

/// On Windows, replacing a file another process holds open fails with
/// `PermissionDenied`, so that is retried for about 600 ms in all.
fn persist(mut temp: NamedTempFile, path: &Path) -> io::Result<()> {
    let mut delay = Duration::from_millis(5);
    let mut attempt = 1;
    loop {
        match temp.persist(path) {
            Ok(_) => return Ok(()),
            Err(PersistError { error, file })
                if cfg!(windows)
                    && error.kind() == ErrorKind::PermissionDenied
                    && attempt < 8 =>
            {
                temp = file;
                std::thread::sleep(delay);
                delay *= 2;
                attempt += 1;
            }
            Err(PersistError { error, .. }) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
