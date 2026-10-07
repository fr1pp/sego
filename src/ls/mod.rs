use std::io;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::address::{Address, pinned, rev_field};
use crate::error::Error;
use crate::escape::one_line;
use crate::output::text::header;
use crate::output::{Answer, Notes};
use crate::quote::shell_word;
use crate::submodule::{self, Submodule};
use crate::upstream::{
    Candidate, GRAPHQL_PATH, Located, Plan, Transport, Var, View, build,
    classify, locate,
};

const TEXT_ROWS: usize = 200;

const COMMIT_TEMPLATE: &str = "\
tree: tree(path: $path) { entries(recursive: $recursive) { path isDirectory submodule { url commit } } }
      file: blob(path: $path) { totalLines byteSize binary }
      parent: tree(path: $parent) { entries { path submodule { url commit } } }";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Dir,
    File,
    Submodule,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Dir => "dir",
            Kind::File => "file",
            Kind::Submodule => "submodule",
        }
    }
}

#[derive(Serialize)]
pub struct Entry {
    path: String,
    kind: Kind,
    submodule_url: Option<String>,
    submodule_target: Option<String>,
    submodule_commit: Option<String>,
    #[serde(skip)]
    target_shown: Option<String>,
}

impl Entry {
    fn new(path: &str, is_dir: bool, module: Option<Submodule>) -> Entry {
        let kind = match (&module, is_dir) {
            (_, true) => Kind::Dir,
            (Some(_), false) => Kind::Submodule,
            (None, false) => Kind::File,
        };
        let target_shown = module.as_ref().map(Submodule::shown);
        let (submodule_url, submodule_target, submodule_commit) = match module
        {
            Some(module) => {
                (Some(module.url), module.target, Some(module.commit))
            }
            None => (None, None, None),
        };
        Entry {
            path: path.to_owned(),
            kind,
            submodule_url,
            submodule_target,
            submodule_commit,
            target_shown,
        }
    }

    fn row(&self) -> String {
        let mut row =
            format!("{:<9}  {}", self.kind.word(), shell_word(&self.path));
        if matches!(self.kind, Kind::Dir) {
            row.push('/');
        }
        if let Some(target) = &self.target_shown {
            row.push_str("  ");
            row.push_str(&one_line(target));
        }
        row.push('\n');
        row
    }
}

#[derive(Serialize)]
pub struct Listed {
    repo: String,
    rev: Option<String>,
    commit: String,
    path: Option<String>,
    recursive: bool,
    entries: Vec<Entry>,
}

impl Listed {
    fn fields(&self) -> Vec<String> {
        let total = self.entries.len();
        let count = |n: usize, one: &str, many: &str| {
            format!("{n} {}", if n == 1 { one } else { many })
        };
        let mut fields = vec![if total > TEXT_ROWS {
            format!("{TEXT_ROWS} of {total} entries shown")
        } else {
            count(total, "entry", "entries")
        }];
        let of = |kind: fn(&Kind) -> bool| {
            self.entries.iter().filter(|e| kind(&e.kind)).count()
        };
        for (n, one, many) in [
            (of(|k| matches!(k, Kind::Dir)), "dir", "dirs"),
            (of(|k| matches!(k, Kind::File)), "file", "files"),
            (of(|k| matches!(k, Kind::Submodule)), "submodule", "submodules"),
        ] {
            if n > 0 {
                fields.push(count(n, one, many));
            }
        }
        fields.extend(rev_field(self.rev.as_ref()));
        fields
    }
}

impl Answer for Listed {
    fn write_json(&self, out: &mut dyn io::Write) -> io::Result<()> {
        crate::output::json::write(self, out)
    }

    fn text(&self) -> String {
        let address = pinned(&self.repo, &self.commit, self.path.as_deref());
        let mut out = header(&address, &self.fields());
        for entry in self.entries.iter().take(TEXT_ROWS) {
            out.push_str(&entry.row());
        }
        out
    }
}

fn plan(recursive: bool) -> Plan {
    Plan {
        operation: "ListTree",
        commit_template: COMMIT_TEMPLATE,
        vars: Box::new(move |candidate: &Candidate| {
            let path = candidate.path.clone().unwrap_or_default();
            let parent =
                path.rsplit_once('/').map_or("", |(parent, _)| parent);
            vec![
                Var { name: "path", ty: "String!", value: json!(path) },
                Var {
                    name: "recursive",
                    ty: "Boolean!",
                    value: json!(recursive),
                },
                Var { name: "parent", ty: "String!", value: json!(parent) },
            ]
        }),
    }
}

pub fn run(
    address: &Address,
    recursive: bool,
    transport: &dyn Transport,
    deadline: Instant,
    _notes: &mut Notes,
) -> Result<Listed, Error> {
    let request = build(&plan(recursive), address);
    let reply = transport.post(GRAPHQL_PATH, &request.body(), deadline)?;
    let envelope = classify(reply)?;
    let located = locate(address, &envelope)?;
    let view = View::new(&envelope, &located.at);
    let entries = entries_of(&view, &located)?;
    let path = located.path.filter(|path| !path.is_empty());
    Ok(Listed {
        repo: located.repo,
        rev: located.rev,
        commit: located.commit,
        path,
        recursive,
        entries,
    })
}

fn listing_failed() -> Error {
    Error::failed("the directory listing")
}

fn entries_of(
    view: &View<'_>,
    located: &Located,
) -> Result<Vec<Entry>, Error> {
    let path = located.path.as_deref().filter(|path| !path.is_empty());
    if let Some(path) = path
        && let Some(module) = submodule_at(view, located, path)
    {
        return Ok(vec![Entry::new(path, false, Some(module))]);
    }
    let tree_failed = view.errored_beneath(&["tree"]);
    if let Some(tree) = view.get(&["tree"]) {
        if tree_failed {
            return Err(listing_failed());
        }
        return directory(tree, &located.repo);
    }
    let Some(path) = path else {
        return Err(listing_failed());
    };
    let file_failed = view.errored_beneath(&["file"]);
    match view.get(&["file"]) {
        Some(file) if tree_failed && !file_failed => {
            Err(file_path(file, located, path))
        }
        None if !tree_failed && !file_failed => Err(Error::missing(format!(
            "{path} not found in {}",
            pinned(&located.repo, &located.commit, None)
        ))),
        _ => Err(listing_failed()),
    }
}

fn submodule_at(
    view: &View<'_>,
    located: &Located,
    path: &str,
) -> Option<Submodule> {
    let entries = view.get(&["parent"])?.get("entries")?.as_array()?;
    submodule::find(entries, path, &located.repo)
}

fn directory(tree: &Value, repo: &str) -> Result<Vec<Entry>, Error> {
    let list = tree
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(listing_failed)?;
    let mut entries = Vec::with_capacity(list.len());
    for entry in list {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(listing_failed)?;
        let is_dir = entry
            .get("isDirectory")
            .and_then(Value::as_bool)
            .ok_or_else(listing_failed)?;
        let module = submodule::from_entry(entry, repo);
        entries.push(Entry::new(path, is_dir, module));
    }
    entries.sort_by(|a, b| a.path.split('/').cmp(b.path.split('/')));
    Ok(entries)
}

/// The error for a path that names a file, from its `file` details.
fn file_path(file: &Value, located: &Located, path: &str) -> Error {
    let details = (
        file.get("byteSize").and_then(Value::as_u64),
        file.get("binary").and_then(Value::as_bool),
        file.get("totalLines").and_then(Value::as_u64),
    );
    let (Some(bytes), Some(binary), Some(lines)) = details else {
        return listing_failed();
    };
    let address = pinned(&located.repo, &located.commit, None);
    let what = if binary {
        format!("binary, {bytes} bytes")
    } else {
        let lines = if bytes == 0 { 0 } else { lines };
        let unit = if lines == 1 { "line" } else { "lines" };
        format!("{lines} {unit}")
    };
    Error::missing(format!("{path} is a file at {address} ({what})"))
}
