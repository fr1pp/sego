use std::ffi::OsStr;
use std::path::Path;

use super::{Root, Unavailable, root};

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
