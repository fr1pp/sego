use std::ffi::OsStr;
use std::fmt;
use std::io::{self, ErrorKind};
use std::path::PathBuf;

use etcetera::app_strategy::{
    AppStrategy, AppStrategyArgs, choose_app_strategy,
};

#[derive(Debug)]
pub enum Root {
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
            ErrorKind::ReadOnlyFilesystem => {
                "read-only file system".to_owned()
            }
            ErrorKind::StorageFull | ErrorKind::QuotaExceeded => {
                "no space left on device".to_owned()
            }
            ErrorKind::AlreadyExists | ErrorKind::NotADirectory => {
                "not a directory".to_owned()
            }
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
pub fn root(var: Option<&OsStr>) -> Result<Root, Unavailable> {
    match var {
        Some(value) if value == "off" => Ok(Root::Off),
        Some(value) if !value.is_empty() => {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                Ok(Root::Dir(path))
            } else {
                Err(Unavailable::not_absolute())
            }
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
