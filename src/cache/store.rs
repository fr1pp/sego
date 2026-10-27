use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, FileTimes};
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::{Builder, NamedTempFile, PersistError};

pub const MTIME_INTERVAL: Duration = Duration::from_hours(1);

/// No entry name starts with `~`, since names are hex.
pub const TEMP_PREFIX: &str = "~tmp-";

const TAG: &str =
    "Signature: 8a477f597d28d172789f06886806bc55\n# This directory is a cache created by sego.\n";

/// A hit is a regular file, not a symlink, holding exactly `size` bytes.
/// The path is checked before the open and the handle after it, so
/// anything else swapped in between is refused once opened.
pub fn read_entry(path: &Path, size: u64) -> Option<Vec<u8>> {
    let hit = |metadata: fs::Metadata| metadata.file_type().is_file() && metadata.len() == size;
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
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        Ok(File::from(rustix::fs::open(path, flags, Mode::empty())?))
    }
    #[cfg(not(unix))]
    File::open(path)
}

/// Errors are ignored: a stale time only shortens the entry's life.
pub fn refresh(path: &Path, now: SystemTime) {
    let Ok(modified) = fs::symlink_metadata(path).and_then(|metadata| metadata.modified()) else {
        return;
    };
    let gap = now.duration_since(modified).unwrap_or_else(|error| error.duration());
    if gap > MTIME_INTERVAL {
        let _ = fs::set_times_nofollow(path, FileTimes::new().set_modified(now));
    }
}

pub fn write_entry(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_with_prefix(path, bytes, TEMP_PREFIX)
}

pub fn write_with_prefix(path: &Path, bytes: &[u8], prefix: &str) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("entry has no directory"))?;
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
fn init_root_with(root: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    // Existence is checked before the kind: another creator can rename its
    // root into place between the two checks, and a root never goes away.
    if fs::symlink_metadata(root).is_ok() {
        return if root.is_dir() { Ok(()) } else { Err(ErrorKind::NotADirectory.into()) };
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
    #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
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
            Err(error) if error.kind() == ErrorKind::NotFound && attempts < 4 => {
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
                if cfg!(windows) && error.kind() == ErrorKind::PermissionDenied && attempt < 8 =>
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
mod tests {
    use super::*;

    use std::fs::{self, File, FileTimes};
    use std::io::{self, ErrorKind, Write};
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use tempfile::TempDir;

    use crate::cache::entry_path;

    const NAME: &str = "9c2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6";

    #[test]
    fn an_entry_round_trips() {
        let dir = TempDir::new().unwrap();
        let entry = entry_path(dir.path(), NAME);
        assert_eq!(read_entry(&entry, 5), None);
        write_entry(&entry, b"hello").unwrap();
        assert_eq!(read_entry(&entry, 5).as_deref(), Some(&b"hello"[..]));
        assert_eq!(read_entry(&entry, 6), None);
        assert_eq!(read_entry(&entry, 4), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_entry_is_a_miss_and_is_replaced() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"other").unwrap();
        let entry = entry_path(&dir.path().join("root"), NAME);
        fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &entry).unwrap();
        assert_eq!(read_entry(&entry, 5), None);
        write_entry(&entry, b"hello").unwrap();
        assert!(fs::symlink_metadata(&entry).unwrap().file_type().is_file());
        assert_eq!(read_entry(&entry, 5).as_deref(), Some(&b"hello"[..]));
        assert_eq!(fs::read(&target).unwrap(), b"other");
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_that_is_a_symlink_is_not_opened() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"hello").unwrap();
        let entry = entry_path(&dir.path().join("root"), NAME);
        fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &entry).unwrap();
        assert!(open_entry(&entry).is_err());
        assert_eq!(read_entry(&entry, 5), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_entry_is_a_miss_and_its_open_does_not_wait() {
        let dir = TempDir::new().unwrap();
        let entry = entry_path(&dir.path().join("root"), NAME);
        fs::create_dir_all(entry.parent().unwrap()).unwrap();
        let made = std::process::Command::new("mkfifo").arg(&entry).status().unwrap();
        assert!(made.success());
        let (sent, received) = std::sync::mpsc::channel();
        let path = entry.clone();
        std::thread::spawn(move || {
            let kind = open_entry(&path)
                .and_then(|file| file.metadata())
                .map(|metadata| metadata.file_type().is_file());
            let _ = sent.send((kind.ok(), read_entry(&path, 5)));
        });
        let (kind, read) = received
            .recv_timeout(Duration::from_secs(5))
            .expect("opening the FIFO waited for a writer");
        assert_eq!(kind, Some(false));
        assert_eq!(read, None);
    }

    #[cfg(unix)]
    #[test]
    fn cache_directories_are_owner_only_and_entries_are_owner_read_write() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        let entry = entry_path(&root, NAME);
        init_root(&root).unwrap();
        write_entry(&entry, b"hello").unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&root.join("raw")), 0o700);
        assert_eq!(mode(entry.parent().unwrap()), 0o700);
        assert_eq!(mode(&entry), 0o600);
    }

    #[test]
    fn the_root_is_tagged() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        init_root(&root).unwrap();
        let tag = root.join("CACHEDIR.TAG");
        let first = fs::read_to_string(&tag).unwrap();
        assert!(first.starts_with("Signature: 8a477f597d28d172789f06886806bc55"));
        init_root(&root).unwrap();
        assert_eq!(fs::read_to_string(&tag).unwrap(), first);
    }

    fn names_in(dir: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir)? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    #[test]
    fn a_created_root_is_tagged_and_private() {
        let dir = TempDir::new().unwrap();
        let parent = dir.path().join("a").join("b");
        let root = parent.join("root");
        init_root(&root).unwrap();
        let tag = fs::read_to_string(root.join("CACHEDIR.TAG")).unwrap();
        assert!(tag.starts_with("Signature: 8a477f597d28d172789f06886806bc55"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&root).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        assert_eq!(names_in(&parent).unwrap(), ["root"]);
    }

    #[test]
    fn an_existing_root_is_used_untagged() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(root.join("mine.txt"), b"keep").unwrap();
        init_root(&root).unwrap();
        assert!(!root.join("CACHEDIR.TAG").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&root).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
        assert_eq!(names_in(&root).unwrap(), ["mine.txt"]);
        assert_eq!(fs::read(root.join("mine.txt")).unwrap(), b"keep");
        assert_eq!(names_in(dir.path()).unwrap(), ["root"]);
    }

    #[test]
    fn concurrent_creators_leave_one_tagged_root() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        std::thread::scope(|scope| {
            let creators: Vec<_> = (0..8).map(|_| scope.spawn(|| init_root(&root))).collect();
            for creator in creators {
                creator.join().unwrap().unwrap();
            }
        });
        let tag = fs::read_to_string(root.join("CACHEDIR.TAG")).unwrap();
        assert!(tag.starts_with("Signature: 8a477f597d28d172789f06886806bc55"));
        assert_eq!(names_in(dir.path()).unwrap(), ["root"]);
    }

    #[test]
    fn a_tag_that_fails_to_write_leaves_nothing() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        let error = init_root_with(&root, |_| Err(io::Error::other("disk"))).unwrap_err();
        assert_eq!(error.to_string(), "disk");
        assert_eq!(names_in(dir.path()).unwrap(), Vec::<String>::new());
        init_root(&root).unwrap();
        assert!(root.join("CACHEDIR.TAG").exists());
    }

    #[test]
    fn a_lost_race_uses_the_other_root() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        init_root_with(&root, |file| {
            fs::create_dir(&root)?;
            fs::write(root.join("x"), b"")?;
            file.write_all(b"t")
        })
        .unwrap();
        assert_eq!(names_in(&root).unwrap(), ["x"]);
        assert_eq!(names_in(dir.path()).unwrap(), ["root"]);
    }

    #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
    #[test]
    fn an_empty_root_made_in_the_window_is_kept() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        init_root_with(&root, |file| {
            fs::create_dir(&root)?;
            file.write_all(b"t")
        })
        .unwrap();
        assert_eq!(names_in(&root).unwrap(), Vec::<String>::new());
        assert_eq!(names_in(dir.path()).unwrap(), ["root"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_at_the_root_in_an_unwritable_parent_is_not_a_directory() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = TempDir::new().unwrap();
        if fs::metadata(dir.path()).unwrap().uid() == 0 {
            return;
        }
        let parent = dir.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let root = parent.join("file");
        fs::write(&root, b"").unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o500)).unwrap();
        let result = init_root(&root);
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(result.unwrap_err().kind(), ErrorKind::NotADirectory);
    }

    #[test]
    fn a_root_without_a_parent_is_invalid_input() {
        let error = init_root(Path::new("")).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn concurrent_writes_leave_one_entry() {
        let dir = TempDir::new().unwrap();
        let entry = entry_path(dir.path(), NAME);
        let bytes: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        std::thread::scope(|scope| {
            let writers: Vec<_> =
                (0..8).map(|_| scope.spawn(|| write_entry(&entry, &bytes))).collect();
            for writer in writers {
                writer.join().unwrap().unwrap();
            }
        });
        assert_eq!(read_entry(&entry, 4096), Some(bytes));
        let names: Vec<_> = fs::read_dir(entry.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(!names.iter().any(|n| n.starts_with(TEMP_PREFIX)), "{names:?}");
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn a_future_mtime_is_refreshed() {
        let dir = TempDir::new().unwrap();
        let now = SystemTime::now();
        let dated = |name: &str, modified: SystemTime| {
            let path = dir.path().join(name);
            File::create(&path)
                .unwrap()
                .set_times(FileTimes::new().set_modified(modified))
                .unwrap();
            path
        };
        let modified = |p: &Path| fs::metadata(p).unwrap().modified().unwrap();
        let hours = Duration::from_hours(2);
        let ahead = dated("ahead", now + hours);
        let behind = dated("behind", now - hours);
        let recent = dated("recent", now - Duration::from_mins(10));
        let recent_before = modified(&recent);
        for path in [&ahead, &behind, &recent] {
            refresh(path, now);
        }
        for path in [&ahead, &behind] {
            let m = modified(path);
            let gap = m.duration_since(now).or_else(|_| now.duration_since(m));
            assert!(gap.unwrap() < Duration::from_secs(1), "{path:?}");
        }
        assert_eq!(modified(&recent), recent_before);
        refresh(&dir.path().join("missing"), now);
    }

    #[cfg(unix)]
    #[test]
    fn a_write_into_an_unwritable_root_fails_with_its_kind() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = TempDir::new().unwrap();
        let probe = dir.path().join("probe");
        fs::write(&probe, b"").unwrap();
        if fs::metadata(&probe).unwrap().uid() == 0 {
            return;
        }
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
        let error = write_entry(&entry_path(&root, NAME), b"hello").unwrap_err();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }
}
