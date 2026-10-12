use std::io;
use std::slice;
use std::time::Instant;

use serde::Serialize;
use serde_json::{Value, json};

use crate::address::{Address, Rev, pinned, rev_field, short};
use crate::cli::SHOW_SYNOPSIS;
use crate::diff::files::{FileRow, TEXT_ROWS, files_field, row_text, rows};
use crate::error::Error;
use crate::escape::{render_line, strip_cr};
use crate::output::text::header;
use crate::output::{Answer, Notes, date};
use crate::upstream::{
    RepoPlan, Root, Transport, Var, View, build_repository, exchange,
    locate_repository, missing_revisions, no_commits, revision_lookup_failed,
    text_at,
};

const MESSAGE_INDENT: &str = "    ";
const ROOT_NOTE: &str = "root commit: Sourcegraph has no diff for it";

const COMMIT: &str = "commit(rev: $rev) { oid subject message author { \
                      date person { displayName } } committer { date person \
                      { displayName } } parents { oid } }";

const COMPARISON: &str = "comparison(base: $base, head: $rev) {\n\
                          all: fileDiffs(first: 0) { totalCount diffStat { \
                          added deleted } }\n\
                          page: fileDiffs(first: 200) { nodes { oldPath \
                          newPath stat { added deleted } } }\n\
                          }";

#[derive(Serialize)]
pub struct Committed {
    repo: String,
    rev: String,
    commit: String,
    author: String,
    author_date: String,
    committer: Option<String>,
    committer_date: Option<String>,
    parents: Vec<String>,
    subject: String,
    message: String,
    files: Vec<FileRow>,
    files_total: Option<u64>,
    added: Option<u64>,
    deleted: Option<u64>,
}

impl Committed {
    fn fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        fields.push(self.author_date.clone());
        fields.push(self.author.clone());
        fields.push(if self.parents.is_empty() {
            "parents none".to_owned()
        } else {
            let shorts: Vec<String> =
                self.parents.iter().map(|oid| short(oid)).collect();
            format!("parents {}", shorts.join(" "))
        });
        if let (Some(total), Some(added), Some(deleted)) =
            (self.files_total, self.added, self.deleted)
        {
            let printed = self.files.len().min(TEXT_ROWS);
            fields.push(files_field(printed, total));
            fields.push(format!("+{added} -{deleted}"));
        }
        if self.parents.len() > 1 {
            fields.push("diff against first parent".to_owned());
        }
        fields.extend(rev_field(Some(&self.rev)));
        fields
    }
}

impl Answer for Committed {
    fn write_json(&self, out: &mut dyn io::Write) -> io::Result<()> {
        crate::output::json::write(self, out)
    }

    fn text(&self) -> String {
        let address = pinned(&self.repo, &self.commit, None);
        let mut out = header(&address, &self.fields());
        let mut lines: Vec<&str> = self.message.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        for line in lines {
            let (line, _) = strip_cr(line);
            if !line.trim_matches([' ', '\t']).is_empty() {
                out.push_str(MESSAGE_INDENT);
                out.push_str(&render_line(line, None));
            }
            out.push('\n');
        }
        if !self.files.is_empty() {
            out.push('\n');
            for row in self.files.iter().take(TEXT_ROWS) {
                out.push_str(&row_text(row));
                out.push('\n');
            }
        }
        out
    }
}

fn plan(rev: &str) -> RepoPlan {
    RepoPlan {
        operation: "ShowCommit",
        vars: vec![
            Var { name: "rev", ty: "String!", value: json!(rev) },
            Var {
                name: "base",
                ty: "String!",
                value: json!(format!("{rev}^1")),
            },
        ],
        roots: vec![
            Root { alias: "m", template: COMMIT.to_owned() },
            Root { alias: "d", template: COMPARISON.to_owned() },
        ],
        parts: &[],
    }
}

fn file_list_failed() -> Error {
    Error::failed("the file list")
}

fn date_at(commit: &Value, who: &str) -> Option<String> {
    text_at(commit, &[who, "date"]).map(date)
}

fn parents_of(commit: &Value) -> Result<Vec<String>, Error> {
    commit
        .get("parents")
        .and_then(Value::as_array)
        .ok_or_else(revision_lookup_failed)?
        .iter()
        .map(|parent| {
            text_at(parent, &["oid"])
                .map(str::to_owned)
                .ok_or_else(revision_lookup_failed)
        })
        .collect()
}

pub fn run(
    address: &Address,
    transport: &dyn Transport,
    deadline: Instant,
    notes: &mut Notes,
) -> Result<Committed, Error> {
    let Rev::One(rev) = &address.rev else {
        return Err(Error::usage(
            "a commit needs a revision, like REPO@REV",
            SHOW_SYNOPSIS,
        ));
    };
    let plan = plan(rev);
    let request = build_repository(&plan, &address.repo);
    let envelope = exchange(transport, &request, deadline)?;
    let found = locate_repository(&address.repo, &envelope, &plan)?;
    let repo = &found.repo;
    let (at_m, at_d) = (found.at("m"), found.at("d"));
    let m = View::new(&envelope, slice::from_ref(&at_m));
    let d = View::new(&envelope, slice::from_ref(&at_d));

    if m.errored_beneath(&["commit"]) {
        return Err(revision_lookup_failed());
    }
    let Some(commit) = m.get(&["commit"]) else {
        return Err(no_commits(repo, &m)
            .unwrap_or_else(|| missing_revisions(repo, &[("", rev)])));
    };
    let field = |keys: &[&str]| {
        text_at(commit, keys)
            .map(str::to_owned)
            .ok_or_else(revision_lookup_failed)
    };
    let oid = field(&["oid"])?;
    let subject = field(&["subject"])?;
    let message = field(&["message"])?.replace("\r\n", "\n");
    let parents = parents_of(commit)?;
    let author = field(&["author", "person", "displayName"])?;
    let author_date =
        date_at(commit, "author").ok_or_else(revision_lookup_failed)?;

    let mut committed = Committed {
        repo: repo.clone(),
        rev: rev.clone(),
        commit: oid,
        author,
        author_date,
        committer: text_at(commit, &["committer", "person", "displayName"])
            .map(str::to_owned),
        committer_date: date_at(commit, "committer"),
        parents,
        subject,
        message,
        files: Vec::new(),
        files_total: None,
        added: None,
        deleted: None,
    };
    if committed.parents.is_empty() {
        notes.push(ROOT_NOTE);
        return Ok(committed);
    }

    if d.errored_beneath(&[]) {
        return Err(file_list_failed());
    }
    let count = |keys: &[&str]| {
        d.get(keys).and_then(Value::as_u64).ok_or_else(file_list_failed)
    };
    committed.files_total = Some(count(&["comparison", "all", "totalCount"])?);
    committed.added =
        Some(count(&["comparison", "all", "diffStat", "added"])?);
    committed.deleted =
        Some(count(&["comparison", "all", "diffStat", "deleted"])?);
    let page = d
        .get(&["comparison", "page", "nodes"])
        .and_then(Value::as_array)
        .ok_or_else(file_list_failed)?;
    committed.files = rows(page);
    Ok(committed)
}
