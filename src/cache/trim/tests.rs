use std::fs::{self, File, FileTimes};
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use super::{due, trim};

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
fn a_trim_is_due_for_a_missing_old_or_far_future_stamp() {
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
