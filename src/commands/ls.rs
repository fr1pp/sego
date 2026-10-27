use std::fmt;

use serde::Serialize;

use crate::address::{Repo, Target, pinned, rev_field};
use crate::error::Error;
use crate::escape::one_line;
use crate::escape::shell_word;
use crate::output::{count, header, submodule_target, submodule_text};
use crate::sourcegraph::{Client, Details, Resolved, Submodule, Tree};

const TEXT_ROWS: usize = 200;

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
    fn new(path: &str, is_dir: bool, module: Option<Submodule>, repo: &str) -> Entry {
        let kind = match (&module, is_dir) {
            (_, true) => Kind::Dir,
            (Some(_), false) => Kind::Submodule,
            (None, false) => Kind::File,
        };
        let (submodule_url, submodule_target, submodule_commit, target_shown) = match module {
            Some(module) => {
                let target = submodule_target(&module.url, &module.commit, repo);
                let shown = submodule_text(target.as_deref(), &module.url);
                (Some(module.url), target, Some(module.commit), Some(shown))
            }
            None => (None, None, None, None),
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
        let mut row = format!("{:<9}  {}", self.kind.word(), shell_word(&self.path));
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
pub struct Answer {
    repo: String,
    rev: Option<String>,
    commit: String,
    path: Option<String>,
    recursive: bool,
    entries: Vec<Entry>,
}

impl Answer {
    fn fields(&self) -> Vec<String> {
        let total = self.entries.len();
        let mut fields = vec![if total > TEXT_ROWS {
            format!("{TEXT_ROWS} of {total} entries shown")
        } else {
            count(total, "entry", "entries")
        }];
        let of = |kind: fn(&Kind) -> bool| self.entries.iter().filter(|e| kind(&e.kind)).count();
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

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let address = pinned(&self.repo, &self.commit, self.path.as_deref());
        f.write_str(&header(&address, &self.fields()))?;
        for entry in self.entries.iter().take(TEXT_ROWS) {
            f.write_str(&entry.row())?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub repo: Repo,
    pub target: Target,
    pub recursive: bool,
    pub json: bool,
}

pub fn run(args: &Args, client: &Client) -> Result<Answer, Error> {
    let (at, tree) = client.tree(&args.repo, &args.target, args.recursive)?;
    let path = at.path.clone().filter(|path| !path.is_empty());
    let entries = match tree {
        Tree::Entries(listed) => {
            let mut entries: Vec<Entry> = listed
                .into_iter()
                .map(|entry| Entry::new(&entry.path, entry.is_directory, entry.submodule, &at.repo))
                .collect();
            entries.sort_by(|a, b| a.path.split('/').cmp(b.path.split('/')));
            entries
        }
        Tree::Submodule(submodule) => {
            vec![Entry::new(path.as_deref().unwrap_or_default(), false, Some(submodule), &at.repo)]
        }
        Tree::File(details) => return Err(a_file(&details, &at)),
    };
    Ok(Answer {
        repo: at.repo,
        rev: at.rev,
        commit: at.commit,
        path,
        recursive: args.recursive,
        entries,
    })
}

/// The error for a path that names a file.
fn a_file(details: &Details, at: &Resolved) -> Error {
    let address = pinned(&at.repo, &at.commit, None);
    let what = if details.binary {
        format!("binary, {} bytes", details.byte_size)
    } else {
        count(details.total_lines, "line", "lines")
    };
    let path = at.path.as_deref().unwrap_or_default();
    Error::missing(format!("{path} is a file at {address} ({what})"))
}
