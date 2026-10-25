use std::ffi::OsStr;
use std::fs::{self, File, FileTimes};
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use sego::Endpoint;
use sego::internal::cache::{
    Key, Root, TEMP_PREFIX, Unavailable, due, entry_path, init_root,
    init_root_with, open_entry, read_entry, refresh, root, trim, write_entry,
};
use tempfile::TempDir;

#[test]
fn a_known_key_has_a_known_name() {
    let key = Key {
        endpoint: "https://sourcegraph.com",
        repo: "github.com/gorilla/mux",
        commit: "98cb6bf42e086f6af920b965c38cacc07402d51b",
        path: "mux.go",
    };
    assert_eq!(
        key.name(),
        "9c2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6"
    );
    assert_eq!(
        entry_path(Path::new("/c"), &key.name()),
        Path::new(
            "/c/raw/9c/2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6"
        )
    );
}

#[test]
fn names_differ_where_keys_differ() {
    // case only; parts that would run together without length prefixes;
    // endpoint scheme; prefix
    let base = Key {
        endpoint: "https://x",
        repo: "r",
        commit: "c",
        path: "xt_CONNMARK.h",
    };
    let names = [
        base.name(),
        Key { path: "xt_connmark.h", ..base }.name(),
        Key { repo: "rc", commit: "", ..base }.name(),
        Key { endpoint: "http://x", ..base }.name(),
        Key { endpoint: "https://x/sg", ..base }.name(),
    ];
    let distinct: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(distinct.len(), names.len());
}

#[test]
fn endpoint_cache_keys_are_normalised() {
    let key =
        |s: &str| Endpoint::from_env(Some(OsStr::new(s))).unwrap().cache_key();
    assert_eq!(key("HTTPS://Sourcegraph.COM/"), "https://sourcegraph.com");
    assert_eq!(key("https://user:pw@host:3080/sg/"), "https://host:3080/sg");
    assert_ne!(key("http://x"), key("https://x"));
    assert_ne!(key("https://x/a"), key("https://x"));
}

#[test]
fn the_root_follows_sego_cache_dir() {
    assert!(matches!(root(Some(OsStr::new("off"))), Ok(Root::Off)));
    assert!(
        matches!(root(Some(OsStr::new("/abs/c"))), Ok(Root::Dir(d)) if d == Path::new("/abs/c"))
    );
    assert_eq!(
        root(Some(OsStr::new("rel/c"))).err().unwrap().to_string(),
        "SEGO_CACHE_DIR is not an absolute path"
    );
    // empty counts as unset: etcetera decides, and its answer is absolute and ends in sego
    match root(Some(OsStr::new(""))) {
        Ok(Root::Dir(d)) => {
            assert!(d.is_absolute());
            assert!(
                d.ends_with("sego")
                    || d.ends_with(Path::new("sego").join("cache"))
            );
        }
        Ok(Root::Off) => panic!("empty is not off"),
        Err(u) => assert_eq!(u.to_string(), "no home directory"),
    }
}

#[test]
fn reasons_come_from_the_error_kind() {
    use std::io::{Error, ErrorKind::*};
    let reason = |k| Unavailable::from_io(&Error::from(k)).to_string();
    assert_eq!(reason(PermissionDenied), "permission denied");
    assert_eq!(reason(ReadOnlyFilesystem), "read-only file system");
    assert_eq!(reason(StorageFull), "no space left on device");
    assert_eq!(reason(QuotaExceeded), "no space left on device");
    assert_eq!(reason(NotFound), NotFound.to_string());
    assert_eq!(reason(AlreadyExists), "not a directory");
    assert_eq!(reason(NotADirectory), "not a directory");
}

const NAME: &str =
    "9c2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6";

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
    let made =
        std::process::Command::new("mkfifo").arg(&entry).status().unwrap();
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
fn modes_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("root");
    let entry = entry_path(&root, NAME);
    init_root(&root).unwrap();
    write_entry(&entry, b"hello").unwrap();
    let mode =
        |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
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
        let creators: Vec<_> =
            (0..8).map(|_| scope.spawn(|| init_root(&root))).collect();
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
    let error =
        init_root_with(&root, |_| Err(io::Error::other("disk"))).unwrap_err();
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

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple"
))]
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
        let writers: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| write_entry(&entry, &bytes)))
            .collect();
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

fn date(path: &Path, modified: SystemTime) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    File::create(path)?.set_times(FileTimes::new().set_modified(modified))
}

fn stamp(time: SystemTime) -> String {
    let since_epoch = time.duration_since(SystemTime::UNIX_EPOCH);
    since_epoch.map_or(0, |elapsed| elapsed.as_secs()).to_string()
}

#[test]
fn stamp_rules() {
    let now = SystemTime::now();
    let hours = Duration::from_hours;
    assert!(due(None, now));
    assert!(due(Some("garbage"), now));
    assert!(!due(Some(&stamp(now - hours(23))), now));
    assert!(due(Some(&stamp(now - hours(25))), now));
    assert!(!due(Some(&stamp(now + Duration::from_mins(30))), now));
    assert!(due(Some(&stamp(now + hours(2))), now));
}

fn hex62(digit: char) -> String {
    digit.to_string().repeat(62)
}

#[test]
fn trim_deletes_what_is_old() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let now = SystemTime::now();
    let old = Duration::from_hours(5 * 24 + 2);
    date(&root.join(format!("raw/aa/{}", hex62('0'))), now - old).unwrap();
    date(
        &root.join(format!("raw/aa/{}", hex62('2'))),
        now - Duration::from_hours(24),
    )
    .unwrap();
    date(&root.join("raw/bb/~tmp-x"), now - Duration::from_hours(2)).unwrap();
    date(&root.join("raw/bb/~tmp-y"), now - Duration::from_mins(10)).unwrap();
    date(&root.join(format!("raw/cc/{}", hex62('1'))), now - old).unwrap();
    trim(root, now);
    assert!(!root.join(format!("raw/aa/{}", hex62('0'))).exists());
    assert!(root.join(format!("raw/aa/{}", hex62('2'))).exists());
    assert!(!root.join("raw/bb/~tmp-x").exists());
    assert!(root.join("raw/bb/~tmp-y").exists());
    assert!(!root.join(format!("raw/cc/{}", hex62('1'))).exists());
    assert!(!root.join("raw/cc").exists());
    assert!(root.join("raw/aa").exists());
    assert_eq!(fs::read_to_string(root.join("trim.txt")).unwrap(), stamp(now));
}

#[test]
fn a_recent_stamp_skips_the_trim() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let now = SystemTime::now();
    let entry = root.join(format!("raw/aa/{}", hex62('0')));
    date(&entry, now - Duration::from_hours(6 * 24)).unwrap();
    let before = stamp(now - Duration::from_hours(1));
    fs::write(root.join("trim.txt"), &before).unwrap();
    trim(root, now);
    assert!(entry.exists());
    assert_eq!(fs::read_to_string(root.join("trim.txt")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn trim_never_follows_links() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    let now = SystemTime::now();
    let outside_file = outside.join(hex62('5'));
    date(&outside_file, now - Duration::from_hours(6 * 24)).unwrap();
    fs::create_dir_all(root.join("raw")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("raw/zz")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("raw/aa")).unwrap();
    fs::create_dir_all(root.join("raw/bb")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("raw/bb/dir")).unwrap();
    trim(&root, now);
    assert!(outside_file.exists());
    assert!(outside.exists());
}

#[test]
fn trim_only_touches_sego_names() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let now = SystemTime::now();
    let old = now - Duration::from_hours(6 * 24);
    let foreign = [
        "raw/2024-trip/IMG_0001.CR2".to_string(),
        "raw/aa/notes.txt".to_string(),
        "raw/aa/".to_string() + &hex62('A'),
        "raw/aa/".to_string() + &hex62('0')[1..],
        "raw/aa/0".to_string() + &hex62('0'),
        "raw/aa/".to_string() + &hex62('g'),
    ];
    for path in &foreign {
        date(&root.join(path), old).unwrap();
    }
    let ours = root.join("raw/aa").join(hex62('3'));
    date(&ours, old).unwrap();
    trim(root, now);
    for path in &foreign {
        assert!(root.join(path).exists(), "{path}");
    }
    assert!(!ours.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn trim_leaves_an_uppercase_shard_alone() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let now = SystemTime::now();
    let foreign = root.join("raw/AA").join(hex62('0'));
    date(&foreign, now - Duration::from_hours(6 * 24)).unwrap();
    trim(root, now);
    assert!(foreign.exists());
}

#[test]
fn trim_sweeps_stale_temp_files_in_the_root() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let now = SystemTime::now();
    let old = now - Duration::from_hours(2);
    date(&root.join("~tmp-trim-stale"), old).unwrap();
    date(&root.join("~tmp-trim-fresh"), now - Duration::from_mins(10))
        .unwrap();
    date(&root.join("~tmp-x"), old).unwrap();
    date(&root.join("notes.txt"), old).unwrap();
    date(&root.join(hex62('6')), now - Duration::from_hours(6 * 24)).unwrap();
    date(&root.join("~tmp-dir/inner"), old).unwrap();
    trim(root, now);
    assert!(!root.join("~tmp-trim-stale").exists());
    assert!(root.join("~tmp-trim-fresh").exists());
    assert!(root.join("~tmp-x").exists());
    assert!(root.join("notes.txt").exists());
    assert!(root.join(hex62('6')).exists());
    assert!(root.join("~tmp-dir/inner").exists());
}

#[test]
fn an_entry_survives_the_extra_hour() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let now = SystemTime::now();
    let days = Duration::from_hours(5 * 24);
    let kept = root.join(format!("raw/aa/{}", hex62('7')));
    let gone = root.join(format!("raw/aa/{}", hex62('8')));
    date(&kept, now - (days + Duration::from_mins(30))).unwrap();
    date(&gone, now - (days + Duration::from_mins(90))).unwrap();
    trim(root, now);
    assert!(kept.exists());
    assert!(!gone.exists());
}

#[cfg(unix)]
#[test]
fn trim_does_not_follow_a_linked_raw() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    let now = SystemTime::now();
    let outside_file = outside.join("aa").join(hex62('4'));
    date(&outside_file, now - Duration::from_hours(6 * 24)).unwrap();
    fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("raw")).unwrap();
    trim(&root, now);
    assert!(outside_file.exists());
}
