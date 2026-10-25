mod key;
mod root;
mod store;
mod trim;

use std::cell::Cell;
use std::ffi::OsStr;
use std::path::Path;
use std::time::SystemTime;

pub use key::*;
pub use root::*;
pub use store::*;
pub use trim::*;

/// The raw-file cache for one command.
pub struct Cache {
    root: Result<Root, Unavailable>,
    endpoint: String,
    used: Cell<bool>,
}

impl Cache {
    /// `var` is the value of `SEGO_CACHE_DIR`; `endpoint` is
    /// `Endpoint::cache_key`.
    pub fn new(var: Option<&OsStr>, endpoint: String) -> Cache {
        Cache { root: root(var), endpoint, used: Cell::new(false) }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The cache directory, `None` when the cache is off.
    fn dir(&self) -> Result<Option<&Path>, Unavailable> {
        match &self.root {
            Ok(Root::Off) => Ok(None),
            Ok(Root::Dir(dir)) => Ok(Some(dir)),
            Err(reason) => Err(reason.clone()),
        }
    }

    /// The entry's bytes when it holds exactly `size` of them.
    pub fn get(
        &self,
        key: &Key<'_>,
        size: u64,
        now: SystemTime,
    ) -> Result<Option<Vec<u8>>, Unavailable> {
        let Some(dir) = self.dir()? else {
            return Ok(None);
        };
        init_root(dir).map_err(|error| Unavailable::from_io(&error))?;
        self.used.set(true);
        let path = entry_path(dir, &key.name());
        let bytes = read_entry(&path, size);
        if bytes.is_some() {
            refresh(&path, now);
        }
        Ok(bytes)
    }

    pub fn put(&self, key: &Key<'_>, bytes: &[u8]) -> Result<(), Unavailable> {
        let Some(dir) = self.dir()? else {
            return Ok(());
        };
        write_entry(&entry_path(dir, &key.name()), bytes)
            .map_err(|error| Unavailable::from_io(&error))?;
        self.used.set(true);
        Ok(())
    }

    pub fn trim_if_used(&self, now: SystemTime) {
        if self.used.get()
            && let Ok(Root::Dir(dir)) = &self.root
        {
            trim(dir, now);
        }
    }
}
