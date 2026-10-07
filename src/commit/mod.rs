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
use crate::output::{Answer, Notes};
use crate::upstream::{
    GRAPHQL_PATH, RepoPlan, Root, Transport, Var, View, build_repository,
    classify, locate_repository, missing_revisions,
};

const MESSAGE_INDENT: &str = "    ";
const DATE_LEN: usize = 10;
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
    author: Option<String>,
    author_date: Option<String>,
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
        fields.extend(self.author_date.clone());
        fields.extend(self.author.clone());
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
            if !line.is_empty() {
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

fn lookup_failed() -> Error {
    Error::failed("the revision lookup")
}

fn file_list_failed() -> Error {
    Error::failed("the file list")
}

fn text_at(commit: &Value, keys: &[&str]) -> Option<String> {
    let mut value = commit;
    for key in keys {
        value = value.get(key)?;
    }
    value.as_str().map(str::to_owned)
}

fn date_at(commit: &Value, who: &str) -> Option<String> {
    text_at(commit, &[who, "date"])
        .map(|date| date.chars().take(DATE_LEN).collect())
}

fn parents_of(commit: &Value) -> Result<Vec<String>, Error> {
    commit
        .get("parents")
        .and_then(Value::as_array)
        .ok_or_else(lookup_failed)?
        .iter()
        .map(|parent| text_at(parent, &["oid"]).ok_or_else(lookup_failed))
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
    let reply = transport.post(GRAPHQL_PATH, &request.body(), deadline)?;
    let envelope = classify(reply)?;
    let found = locate_repository(&address.repo, &envelope, &plan)?;
    let repo = &found.repo;
    let (at_m, at_d) = (found.at("m"), found.at("d"));
    let m = View::new(&envelope, slice::from_ref(&at_m));
    let d = View::new(&envelope, slice::from_ref(&at_d));

    if m.errored_beneath(&["commit"]) {
        return Err(lookup_failed());
    }
    let Some(commit) = m.get(&["commit"]) else {
        if m.get(&["defaultBranch"]).is_some() {
            return Err(missing_revisions(repo, &[("", rev)]));
        }
        if m.errored_beneath(&["defaultBranch"]) {
            return Err(Error::failed("the repository lookup"));
        }
        return Err(Error::missing(format!("{repo} has no commits")));
    };
    let oid = text_at(commit, &["oid"]).ok_or_else(lookup_failed)?;
    let subject = text_at(commit, &["subject"]).ok_or_else(lookup_failed)?;
    let message = text_at(commit, &["message"])
        .ok_or_else(lookup_failed)?
        .replace("\r\n", "\n");
    let parents = parents_of(commit)?;

    let mut committed = Committed {
        repo: repo.clone(),
        rev: rev.clone(),
        commit: oid,
        author: text_at(commit, &["author", "person", "displayName"]),
        author_date: date_at(commit, "author"),
        committer: text_at(commit, &["committer", "person", "displayName"]),
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
