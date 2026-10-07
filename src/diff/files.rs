use serde::Serialize;
use serde_json::Value;

use crate::quote::shell_word;

pub const TEXT_ROWS: usize = 200;

#[derive(Serialize)]
pub struct FileRow {
    /// `A`, `D`, `R` or `M`. A type, mode or submodule change is an `M`.
    pub status: char,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub added: u64,
    pub deleted: u64,
}

fn text_at(node: &Value, key: &str) -> Option<String> {
    node.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn count_at(node: &Value, key: &str) -> u64 {
    node.get("stat")
        .and_then(|stat| stat.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn status(old: Option<&String>, new: Option<&String>) -> char {
    match (old, new) {
        (None, _) => 'A',
        (_, None) => 'D',
        (Some(old), Some(new)) if old != new => 'R',
        _ => 'M',
    }
}

/// One row per `fileDiffs` node, from its `oldPath`, `newPath` and `stat`.
pub fn rows(nodes: &[Value]) -> Vec<FileRow> {
    nodes
        .iter()
        .map(|node| {
            let old_path = text_at(node, "oldPath");
            let new_path = text_at(node, "newPath");
            FileRow {
                status: status(old_path.as_ref(), new_path.as_ref()),
                old_path,
                new_path,
                added: count_at(node, "added"),
                deleted: count_at(node, "deleted"),
            }
        })
        .collect()
}

/// `M  +2 -2  path`, or `R  +1 -1  old -> new`, without a line break.
pub fn row_text(row: &FileRow) -> String {
    let word = |path: &Option<String>| {
        shell_word(path.as_deref().unwrap_or_default()).into_owned()
    };
    let path = match row.status {
        'R' => format!("{} -> {}", word(&row.old_path), word(&row.new_path)),
        'D' => word(&row.old_path),
        _ => word(&row.new_path),
    };
    format!("{}  +{} -{}  {path}", row.status, row.added, row.deleted)
}

/// `shown` rows are printed out of `total` files.
pub fn files_field(shown: usize, total: u64) -> String {
    if (shown as u64) < total {
        return format!("{shown} of {total} files shown");
    }
    let unit = if total == 1 { "file" } else { "files" };
    format!("{total} {unit}")
}
