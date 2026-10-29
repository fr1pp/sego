use std::cell::Cell;
use std::ffi::OsStr;
use std::fmt::{self, Write as _};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use etcetera::app_strategy::{AppStrategy, AppStrategyArgs, choose_app_strategy};
use ring::digest::{SHA256, digest};

use crate::error::Error;

mod store;
mod trim;

use store::{init_root, read_entry, refresh, write_entry};

/// Where sego keeps the files of raw reads (ADR 0009).
pub struct Cache {
    root: Result<Root, Unavailable>,
    endpoint: String,
    used: Cell<bool>,
}

/// A file at a commit of a repository, as the cache names it.
#[derive(Clone, Copy, Debug)]
pub struct Entry<'a> {
    pub repo: &'a str,
    pub commit: &'a str,
    pub path: &'a str,
}

impl Cache {
    /// The cache for files read from the endpoint at `endpoint_url`; `var` is
    /// the value of `SEGO_CACHE_DIR`.
    pub fn new(var: Option<&OsStr>, endpoint_url: &str) -> Cache {
        Cache { root: root(var), endpoint: endpoint_key(endpoint_url), used: Cell::new(false) }
    }

    /// The file of `size` bytes at `entry`: what the cache holds, when it is
    /// exactly `size` bytes, or else what `download` reads, which is kept
    /// when it is `size` bytes long. A cache that can't be used doesn't fail
    /// the read; the reason comes back beside the bytes.
    pub fn read_through(
        &self,
        entry: Entry<'_>,
        size: u64,
        download: impl FnOnce() -> Result<Vec<u8>, Error>,
    ) -> Result<(Vec<u8>, Option<Unavailable>), Error> {
        let path = match self.entry_path(entry) {
            Ok(Some(path)) => path,
            Ok(None) => return Ok((download()?, None)),
            Err(reason) => return Ok((download()?, Some(reason))),
        };
        if let Some(bytes) = read_entry(&path, size) {
            refresh(&path, SystemTime::now());
            return Ok((bytes, None));
        }
        let bytes = download()?;
        if bytes.len() as u64 != size {
            return Ok((bytes, None));
        }
        let unavailable =
            write_entry(&path, &bytes).err().map(|error| Unavailable::from_io(&error));
        self.used.set(true);
        Ok((bytes, unavailable))
    }

    /// The path of `entry` in a root made ready to use; `None` when the
    /// cache is off.
    fn entry_path(&self, entry: Entry<'_>) -> Result<Option<PathBuf>, Unavailable> {
        let dir = match &self.root {
            Ok(Root::Off) => return Ok(None),
            Ok(Root::Dir(dir)) => dir,
            Err(reason) => return Err(reason.clone()),
        };
        init_root(dir).map_err(|error| Unavailable::from_io(&error))?;
        self.used.set(true);
        Ok(Some(entry_path(dir, &name(&self.endpoint, entry))))
    }

    /// Removes old entries, at most once a day, if this run used the cache.
    pub fn trim(&self) {
        if self.used.get()
            && let Ok(Root::Dir(dir)) = &self.root
        {
            trim::trim(dir, SystemTime::now());
        }
    }
}

/// `url` with a lowercase scheme and host, no `user[:pw]@` and no trailing
/// `/`, so that spellings of one endpoint share a cache.
fn endpoint_key(url: &str) -> String {
    let url = url.trim_end_matches('/');
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let (authority, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    format!("{}://{}{path}", scheme.to_ascii_lowercase(), host_port.to_ascii_lowercase())
}

/// 64 lowercase hex digits. Each part is hashed after its byte length, so
/// that no two entries share a byte stream.
fn name(endpoint: &str, entry: Entry<'_>) -> String {
    let mut bytes = Vec::new();
    for part in [endpoint, entry.repo, entry.commit, entry.path] {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    digest(&SHA256, &bytes).as_ref().iter().fold(String::with_capacity(64), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn entry_path(root: &Path, name: &str) -> PathBuf {
    let (fan, rest) = name.split_at(2);
    root.join("raw").join(fan).join(rest)
}

#[derive(Debug)]
enum Root {
    Off,
    Dir(PathBuf),
}

/// The short reason the cache cannot be used, as printed after `cache unavailable: `.
#[derive(Clone, Debug)]
pub struct Unavailable(String);

impl Unavailable {
    pub fn from_io(error: &io::Error) -> Self {
        Self(match error.kind() {
            ErrorKind::PermissionDenied => "permission denied".to_owned(),
            ErrorKind::ReadOnlyFilesystem => "read-only file system".to_owned(),
            ErrorKind::StorageFull | ErrorKind::QuotaExceeded => {
                "no space left on device".to_owned()
            }
            ErrorKind::AlreadyExists | ErrorKind::NotADirectory => "not a directory".to_owned(),
            kind => kind.to_string(),
        })
    }

    pub fn not_absolute() -> Self {
        Self("SEGO_CACHE_DIR is not an absolute path".to_owned())
    }

    pub fn no_home() -> Self {
        Self("no home directory".to_owned())
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `var` is the value of `SEGO_CACHE_DIR`; unset and empty both mean the default location.
fn root(var: Option<&OsStr>) -> Result<Root, Unavailable> {
    match var {
        Some(value) if value == "off" => Ok(Root::Off),
        Some(value) if !value.is_empty() => {
            let path = PathBuf::from(value);
            if path.is_absolute() { Ok(Root::Dir(path)) } else { Err(Unavailable::not_absolute()) }
        }
        _ => choose_app_strategy(AppStrategyArgs {
            app_name: "sego".to_owned(),
            ..Default::default()
        })
        .map(|strategy| strategy.cache_dir())
        .ok()
        .filter(|dir| dir.is_absolute())
        .map(Root::Dir)
        .ok_or_else(Unavailable::no_home),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn a_known_entry_has_a_known_name() {
        let entry = Entry {
            repo: "github.com/gorilla/mux",
            commit: "98cb6bf42e086f6af920b965c38cacc07402d51b",
            path: "mux.go",
        };
        let name = name("https://sourcegraph.com", entry);
        assert_eq!(name, "9c2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6");
        assert_eq!(
            entry_path(Path::new("/c"), &name),
            Path::new("/c/raw/9c/2f74df4e930a4eb06a7bb20d828db6dc4eed3fa759e256183cf59c00a91fb6")
        );
    }

    #[test]
    fn names_differ_where_entries_or_endpoints_differ() {
        // case only; parts that would run together without length prefixes;
        // endpoint scheme; prefix
        let base = Entry { repo: "r", commit: "c", path: "xt_CONNMARK.h" };
        let names = [
            name("https://x", base),
            name("https://x", Entry { path: "xt_connmark.h", ..base }),
            name("https://x", Entry { repo: "rc", commit: "", ..base }),
            name("http://x", base),
            name("https://x/sg", base),
        ];
        let distinct: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(distinct.len(), names.len());
    }

    #[test]
    fn spellings_of_one_endpoint_share_a_key() {
        assert_eq!(endpoint_key("HTTPS://Sourcegraph.COM/"), "https://sourcegraph.com");
        assert_eq!(endpoint_key("https://user:pw@host:3080/sg/"), "https://host:3080/sg");
        assert_ne!(endpoint_key("http://x"), endpoint_key("https://x"));
        assert_ne!(endpoint_key("https://x/a"), endpoint_key("https://x"));
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
                assert!(d.ends_with("sego") || d.ends_with(Path::new("sego").join("cache")));
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

    const FILE: &[u8] = b"1\n2\n3\n";

    const SOURCEGRAPH: &str = "https://sourcegraph.com";

    const BIG: Entry<'static> = Entry { repo: "example.com/x/y", commit: "c", path: "big.c" };

    fn cache_at(dir: &Path) -> Cache {
        Cache::new(Some(dir.as_os_str()), SOURCEGRAPH)
    }

    /// Reads `BIG` of `size` bytes through `cache` from a download of `FILE`,
    /// and counts the downloads.
    fn read(cache: &Cache, size: u64, downloads: &Cell<u32>) -> (Vec<u8>, Option<String>) {
        let download = || {
            downloads.set(downloads.get() + 1);
            Ok(FILE.to_vec())
        };
        let (bytes, unavailable) =
            cache.read_through(BIG, size, download).unwrap_or_else(|e| panic!("{e}"));
        (bytes, unavailable.map(|reason| reason.to_string()))
    }

    fn entries(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(entries(&path));
            } else {
                found.push(path);
            }
        }
        found
    }

    #[test]
    fn a_hit_needs_the_exact_size_and_a_miss_downloads_once() {
        let dir = TempDir::new().unwrap();
        let cache = cache_at(dir.path());
        let downloads = Cell::new(0);
        assert_eq!(read(&cache, FILE.len() as u64, &downloads), (FILE.to_vec(), None));
        assert_eq!(read(&cache, FILE.len() as u64, &downloads), (FILE.to_vec(), None));
        assert_eq!(downloads.get(), 1);
    }

    #[test]
    fn a_download_of_the_wrong_length_answers_and_is_not_stored() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("c");
        let downloads = Cell::new(0);
        assert_eq!(read(&cache_at(&root), 2_000_000, &downloads), (FILE.to_vec(), None));
        assert!(root.join("CACHEDIR.TAG").exists());
        assert_eq!(entries(&root.join("raw")), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_failed_download_fails_the_read_and_stores_nothing() {
        let dir = TempDir::new().unwrap();
        let cache = cache_at(dir.path());
        let failed = cache.read_through(BIG, 3, || Err(Error::network("down")));
        assert_eq!(failed.err().map(|e| e.to_string()).as_deref(), Some("down"));
        assert_eq!(entries(&dir.path().join("raw")), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_cached_file_of_the_wrong_length_is_replaced() {
        let dir = TempDir::new().unwrap();
        let cache = cache_at(dir.path());
        let downloads = Cell::new(0);
        read(&cache, FILE.len() as u64, &downloads);
        let stored = entries(&dir.path().join("raw"));
        fs::write(&stored[0], b"short").unwrap();
        assert_eq!(read(&cache, FILE.len() as u64, &downloads), (FILE.to_vec(), None));
        assert_eq!(downloads.get(), 2);
        assert_eq!(fs::read(&stored[0]).unwrap(), FILE);
    }

    #[test]
    fn a_root_that_is_a_file_reads_through_with_its_reason() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, b"").unwrap();
        let downloads = Cell::new(0);
        let read = read(&cache_at(&file), FILE.len() as u64, &downloads);
        assert_eq!(read, (FILE.to_vec(), Some("not a directory".to_owned())));
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_root_reads_through_with_one_reason() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = TempDir::new().unwrap();
        if fs::metadata(dir.path()).unwrap().uid() == 0 {
            return;
        }
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
        let downloads = Cell::new(0);
        let read = read(&cache_at(&root), FILE.len() as u64, &downloads);
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(read, (FILE.to_vec(), Some("permission denied".to_owned())));
    }

    #[test]
    fn a_cache_that_is_off_reads_through_every_time() {
        let cache = Cache::new(Some(OsStr::new("off")), SOURCEGRAPH);
        let downloads = Cell::new(0);
        for _ in 0..2 {
            assert_eq!(read(&cache, FILE.len() as u64, &downloads), (FILE.to_vec(), None));
        }
        assert_eq!(downloads.get(), 2);
    }
}
