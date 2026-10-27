use std::fmt;

use serde::Serialize;

use crate::address::{Repo, Target, pinned, rev_field};
use crate::commands::resolved_notes;
use crate::error::Error;
use crate::escape::one_line;
use crate::escape::shell_word;
use crate::output::{Notes, count, header, mirrored, submodule_address, submodule_text};
use crate::sourcegraph::{Client, File, Resolved, Submodule};

const TEXT_ROWS: usize = 200;
pub const GITMODULES_NOTE: &str = "submodule URLs unknown";

/// What a path names, as `show` and `ls` say in JSON.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Named {
    File,
    Directory,
    Submodule,
}

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
    submodule_address: Option<String>,
    submodule_commit: Option<String>,
    #[serde(skip)]
    address_shown: Option<String>,
}

impl Entry {
    fn new(path: &str, is_dir: bool, module: Option<Submodule>, repo: &str) -> Entry {
        let kind = match (&module, is_dir) {
            (_, true) => Kind::Dir,
            (Some(_), false) => Kind::Submodule,
            (None, false) => Kind::File,
        };
        let (submodule_url, submodule_address, submodule_commit, address_shown) = match module {
            Some(module) => {
                let url = module.url.as_deref().unwrap_or_default();
                let address = submodule_address(url, &module.commit, repo);
                let shown = submodule_text(address.as_deref(), url);
                (module.url, address, Some(module.commit), Some(shown))
            }
            None => (None, None, None, None),
        };
        Entry {
            path: path.to_owned(),
            kind,
            submodule_url,
            submodule_address,
            submodule_commit,
            address_shown,
        }
    }

    fn row(&self) -> String {
        let mut row = format!("{:<9}  {}", self.kind.word(), shell_word(&self.path));
        if matches!(self.kind, Kind::Dir) {
            row.push('/');
        }
        if let Some(address) = &self.address_shown {
            row.push_str("  ");
            row.push_str(&one_line(address));
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
    kind: Named,
    recursive: bool,
    entries: Vec<Entry>,
    entries_total: Option<u64>,
    entries_more: bool,
    mirrored_at: Option<String>,
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
        fields.extend(rev_field(self.rev.as_ref(), &self.commit));
        fields.extend(self.mirrored_at.as_deref().map(mirrored));
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

pub fn run(args: &Args, client: Client, notes: &mut Notes) -> Result<Answer, Error> {
    let (at, file) = client.file(&args.repo, &args.target, None, args.recursive)?;
    Ok(answer(at, file, args.recursive, notes))
}

/// The listing of what `file` is at `at`: a directory's entries, or one row
/// for a file or a submodule.
pub fn answer(at: Resolved, file: File, recursive: bool, notes: &mut Notes) -> Answer {
    resolved_notes(&at, notes);
    let path = at.path.clone().filter(|path| !path.is_empty());
    let row_path = path.as_deref().unwrap_or_default();
    let (kind, entries, gitmodules) = match file {
        File::Directory(directory) => {
            let mut entries: Vec<Entry> = directory
                .entries
                .items
                .into_iter()
                .map(|entry| Entry::new(&entry.path, entry.is_directory, entry.submodule, &at.repo))
                .collect();
            entries.sort_by(|a, b| a.path.split('/').cmp(b.path.split('/')));
            (Named::Directory, entries, directory.gitmodules)
        }
        File::Submodule { submodule, gitmodules } => (
            Named::Submodule,
            vec![Entry::new(row_path, false, Some(submodule), &at.repo)],
            gitmodules,
        ),
        File::Blob(_) => (Named::File, vec![Entry::new(row_path, false, None, &at.repo)], Ok(())),
    };
    if let Err(error) = gitmodules
        && entries.iter().any(|entry| matches!(entry.kind, Kind::Submodule))
    {
        notes.push(format!("{GITMODULES_NOTE}: {error}"));
    }
    Answer {
        entries_total: Some(entries.len() as u64),
        entries_more: false,
        repo: at.repo,
        rev: at.rev,
        commit: at.commit,
        path,
        kind,
        recursive,
        entries,
        mirrored_at: at.mirrored_at,
    }
}
