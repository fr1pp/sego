use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::store::{MTIME_INTERVAL, TEMP_PREFIX, write_with_prefix};

pub const TRIM_INTERVAL: Duration = Duration::from_hours(24);
pub const TRIM_LIMIT: Duration = Duration::from_hours(5 * 24);

/// The root may be a user's own directory, so the root's sweep matches only
/// the prefix of `trim.txt`'s temporary files.
const STAMP_TEMP_PREFIX: &str = "~tmp-trim-";

/// `stamp` is the contents of `trim.txt`: decimal Unix seconds. A stamp
/// slightly in the future is tolerated as clock skew.
pub fn due(stamp: Option<&str>, now: SystemTime) -> bool {
    let Some(secs) = stamp.and_then(|text| text.trim().parse::<u64>().ok())
    else {
        return true;
    };
    let Some(stamped) = UNIX_EPOCH.checked_add(Duration::from_secs(secs))
    else {
        return true;
    };
    match now.duration_since(stamped) {
        Ok(age) => age >= TRIM_INTERVAL,
        Err(ahead) => ahead.duration() > MTIME_INTERVAL,
    }
}

/// Best effort: every error is ignored and left for the next trim.
pub fn trim(root: &Path, now: SystemTime) {
    let stamp = fs::read_to_string(root.join("trim.txt")).ok();
    if !due(stamp.as_deref(), now) {
        return;
    }
    if let Ok(since_epoch) = now.duration_since(UNIX_EPOCH) {
        let _ = write_with_prefix(
            &root.join("trim.txt"),
            since_epoch.as_secs().to_string().as_bytes(),
            STAMP_TEMP_PREFIX,
        );
    }
    let temp_cutoff = now.checked_sub(MTIME_INTERVAL);
    if let Ok(files) = fs::read_dir(root) {
        for file in files.flatten() {
            trim_file(&file, None, STAMP_TEMP_PREFIX, temp_cutoff);
        }
    }
    let raw = root.join("raw");
    if !fs::symlink_metadata(&raw).is_ok_and(|meta| meta.is_dir()) {
        return;
    }
    let Ok(shards) = fs::read_dir(&raw) else {
        return;
    };
    let entry_cutoff = now.checked_sub(TRIM_LIMIT + MTIME_INTERVAL);
    for shard in shards.flatten() {
        if !shard.file_type().is_ok_and(|kind| kind.is_dir())
            || !is_hex(&shard.file_name(), 2)
        {
            continue;
        }
        let shard = shard.path();
        if let Ok(files) = fs::read_dir(&shard) {
            for file in files.flatten() {
                trim_file(&file, entry_cutoff, TEMP_PREFIX, temp_cutoff);
            }
        }
        let _ = fs::remove_dir(&shard);
    }
}

fn trim_file(
    file: &fs::DirEntry,
    entry_cutoff: Option<SystemTime>,
    temp_prefix: &str,
    temp_cutoff: Option<SystemTime>,
) {
    if !file.file_type().is_ok_and(|kind| kind.is_file()) {
        return;
    }
    let name = file.file_name();
    let cutoff = if is_hex(&name, 62) {
        entry_cutoff
    } else if name.to_string_lossy().starts_with(temp_prefix) {
        temp_cutoff
    } else {
        return;
    };
    let Some(cutoff) = cutoff else {
        return;
    };
    if file
        .metadata()
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified < cutoff)
    {
        let _ = fs::remove_file(file.path());
    }
}

/// Only sego's own names are touched, since `SEGO_CACHE_DIR` may be a
/// directory that already holds other files.
fn is_hex(name: &OsStr, len: usize) -> bool {
    name.as_encoded_bytes().len() == len
        && name
            .as_encoded_bytes()
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
