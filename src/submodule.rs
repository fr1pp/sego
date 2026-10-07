use serde::Serialize;
use serde_json::Value;

use crate::address::{short, submodule_target};

#[derive(Serialize)]
pub struct Submodule {
    pub url: String,
    pub target: Option<String>,
    pub commit: String,
}

/// `entries` is the `parent: tree(path:) { entries { path submodule { url
/// commit } } }` list.
pub fn find(entries: &[Value], path: &str, repo: &str) -> Option<Submodule> {
    let entry = entries.iter().find(|entry| {
        entry.get("path").and_then(Value::as_str) == Some(path)
    })?;
    from_entry(entry, repo)
}

/// One entry's submodule, or `None` when its `submodule` is null. `find` is
/// built on it, and `ls` calls it once per entry rather than `find` per path.
pub fn from_entry(entry: &Value, repo: &str) -> Option<Submodule> {
    let module = entry.get("submodule").filter(|value| !value.is_null())?;
    let field = |name: &str| {
        module.get(name).and_then(Value::as_str).unwrap_or_default().to_owned()
    };
    let (url, commit) = (field("url"), field("commit"));
    let target = submodule_target(&url, repo)
        .map(|target| with_commit(&target, &commit));
    Some(Submodule { url, target, commit })
}

impl Submodule {
    /// The target with its 12 hex digits, else the stored URL, else `?`.
    pub fn shown(&self) -> String {
        match &self.target {
            Some(target) => target.clone(),
            None if self.url.is_empty() => "?".to_owned(),
            None => self.url.clone(),
        }
    }
}

fn with_commit(name: &str, commit: &str) -> String {
    if commit.is_empty() {
        name.to_owned()
    } else {
        format!("{name}@{}", short(commit))
    }
}
