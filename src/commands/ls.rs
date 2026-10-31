use std::fmt;

use serde::Serialize;

use crate::address::{Repo, Target, pinned, rev_field};
use crate::commands::AddressKind;
use crate::error::Error;
use crate::escape::one_line;
use crate::escape::shell_word;
use crate::output::{Notes, closing_fields, count, header, submodule_text};
use crate::sourcegraph::{Client, File, Resolved, Submodule};

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
    submodule_commit: Option<String>,
}

impl Entry {
    fn new(path: &str, is_dir: bool, module: Option<Submodule>) -> Entry {
        let kind = match (&module, is_dir) {
            (_, true) => Kind::Dir,
            (Some(_), false) => Kind::Submodule,
            (None, false) => Kind::File,
        };
        Entry { path: path.to_owned(), kind, submodule_commit: module.and_then(|m| m.commit) }
    }

    fn row(&self) -> String {
        let mut row = format!("{:<9}  {}", self.kind.word(), shell_word(&self.path));
        if matches!(self.kind, Kind::Dir) {
            row.push('/');
        }
        if matches!(self.kind, Kind::Submodule) {
            row.push_str("  ");
            row.push_str(&one_line(&submodule_text(self.submodule_commit.as_deref())));
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
    kind: AddressKind,
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
        let rev = rev_field(self.rev.as_ref(), &self.commit);
        fields.extend(closing_fields(rev, self.mirrored_at.as_deref()));
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
    let mirrored_at = at.lookup.noting_other_host(|note| notes.push(note));
    let path = at.path.clone().filter(|path| !path.is_empty());
    let row_path = path.as_deref().unwrap_or_default();
    let (kind, entries) = match file {
        File::Directory(directory) => {
            let mut entries: Vec<Entry> = directory
                .entries
                .items
                .into_iter()
                .map(|entry| Entry::new(&entry.path, entry.is_directory, entry.submodule))
                .collect();
            entries.sort_by(|a, b| a.path.split('/').cmp(b.path.split('/')));
            (AddressKind::Directory, entries)
        }
        File::Submodule(submodule) => {
            (AddressKind::Submodule, vec![Entry::new(row_path, false, Some(submodule))])
        }
        File::Blob(_) => (AddressKind::File, vec![Entry::new(row_path, false, None)]),
    };
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
        mirrored_at,
    }
}
