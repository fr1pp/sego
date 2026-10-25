use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sego::{Reply, TransportError};
use tempfile::TempDir;

use crate::envelope::{
    SHOW_BIG, json_reply, over_the_content_limit, under_the_content_limit,
};
use crate::transport::{Ran, Server, reply, sego};

const FILE: &[u8] = b"1\n2\n3\n4\n5\n";

type Script = Vec<Result<Reply, TransportError>>;

/// The replies to reading `big.c` through the raw route, whose size the
/// response gives as `byte_size`.
fn raw_read(byte_size: usize) -> Script {
    vec![
        Ok(json_reply(&over_the_content_limit(byte_size))),
        Ok(reply(200, "text/plain", FILE)),
    ]
}

fn show_big(cache_dir: &OsStr, script: Script) -> (Ran, Server) {
    let server = Server::new(script);
    (sego(&SHOW_BIG, cache_dir, &server), server)
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn a_second_read_of_a_large_file_uses_the_raw_file_cache() {
    let dir = TempDir::new().unwrap();
    let (first, server) =
        show_big(dir.path().as_os_str(), raw_read(FILE.len()));
    assert_eq!(first.stderr, "");
    assert_eq!(first.code, ExitCode::SUCCESS);
    assert_eq!(server.methods(), ["POST", "GET"]);
    assert!(dir.path().join("trim.txt").exists());

    let (second, server) = show_big(
        dir.path().as_os_str(),
        vec![Ok(json_reply(&over_the_content_limit(FILE.len())))],
    );
    assert_eq!(second.stderr, "");
    assert_eq!(second.code, ExitCode::SUCCESS);
    assert_eq!(server.methods(), ["POST"]);
    assert_eq!(second.stdout, first.stdout);
    assert!(second.stdout.ends_with("5:5\n"), "{}", second.stdout);
}

#[test]
fn a_raw_read_of_the_wrong_length_answers_and_is_not_cached() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("c");
    let (ran, server) = show_big(root.as_os_str(), raw_read(2_000_000));
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(ran.stderr, "");
    assert_eq!(server.methods(), ["POST", "GET"]);
    assert!(root.join("CACHEDIR.TAG").exists());
    assert_eq!(files_under(&root.join("raw")), Vec::<PathBuf>::new());
}

#[test]
fn an_existing_cache_dir_is_used_untagged() {
    let dir = TempDir::new().unwrap();
    let (first, server) =
        show_big(dir.path().as_os_str(), raw_read(FILE.len()));
    assert_eq!(first.stderr, "");
    assert_eq!(server.methods(), ["POST", "GET"]);

    let (second, server) = show_big(
        dir.path().as_os_str(),
        vec![Ok(json_reply(&over_the_content_limit(FILE.len())))],
    );
    assert_eq!(second.stderr, "");
    assert_eq!(server.methods(), ["POST"]);
    assert!(!dir.path().join("CACHEDIR.TAG").exists());
}

#[test]
fn a_cached_file_of_the_wrong_length_is_replaced() {
    let dir = TempDir::new().unwrap();
    let (_, server) = show_big(dir.path().as_os_str(), raw_read(FILE.len()));
    assert_eq!(server.methods(), ["POST", "GET"]);
    let entries = files_under(&dir.path().join("raw"));
    assert_eq!(entries.len(), 1);
    fs::write(&entries[0], b"short").unwrap();

    let (ran, server) = show_big(dir.path().as_os_str(), raw_read(FILE.len()));
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(ran.stderr, "");
    assert_eq!(server.methods(), ["POST", "GET"]);
    assert_eq!(files_under(&dir.path().join("raw")), entries);
    assert_eq!(fs::read(&entries[0]).unwrap(), FILE);
}

#[test]
fn a_cache_dir_that_is_a_file_is_noted() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("file");
    fs::write(&file, b"").unwrap();
    let (ran, server) = show_big(file.as_os_str(), raw_read(FILE.len()));
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(ran.stderr, "sego: note: cache unavailable: not a directory\n");
    assert_eq!(server.methods(), ["POST", "GET"]);
}

#[test]
fn a_show_under_the_content_limit_leaves_the_raw_file_cache_untouched() {
    let dir = TempDir::new().unwrap();
    let server = Server::new(vec![Ok(json_reply(&under_the_content_limit()))]);
    let ran = sego(
        &["show", "example.com/x/y@main:a.go"],
        dir.path().as_os_str(),
        &server,
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(ran.stderr, "");
    assert_eq!(server.methods(), ["POST"]);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn an_unwritable_cache_dir_is_noted_once_and_the_raw_read_answers() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = TempDir::new().unwrap();
    if fs::metadata(dir.path()).unwrap().uid() == 0 {
        return;
    }
    let root = dir.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
    let (ran, server) = show_big(root.as_os_str(), raw_read(FILE.len()));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(
        ran.stderr,
        "sego: note: cache unavailable: permission denied\n"
    );
    assert!(ran.stdout.ends_with("5:5\n"), "{}", ran.stdout);
    assert_eq!(server.methods(), ["POST", "GET"]);
}

#[test]
fn a_cache_dir_set_to_off_reads_through_every_time() {
    for _ in 0..2 {
        let (ran, server) = show_big(OsStr::new("off"), raw_read(FILE.len()));
        assert_eq!(ran.code, ExitCode::SUCCESS);
        assert_eq!(ran.stderr, "");
        assert_eq!(server.methods(), ["POST", "GET"]);
    }
}
