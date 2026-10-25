use std::fs;
use std::path::{Path, PathBuf};

/// Appends the `.rs` files under `dir`.
///
/// # Panics
///
/// When a directory can't be read.
pub fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

/// The string literals in Rust source, with a `\` line continuation and
/// the common escapes decoded; comments, char literals and lifetimes are
/// skipped.
pub fn string_literals(source: &str) -> Vec<String> {
    let c: Vec<char> = source.chars().collect();
    let ident = |i: usize| c[i].is_alphanumeric() || c[i] == '_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let next = c.get(i + 1).copied();
        if c[i] == '/' && next == Some('/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if c[i] == '/' && next == Some('*') {
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if c[i] == '\'' {
            if next == Some('\\') {
                i += 3;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if c.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c[i] == 'r'
            && (i == 0
                || !ident(i - 1)
                || (c[i - 1] == 'b' && (i == 1 || !ident(i - 2))))
            && matches!(next, Some('"' | '#'))
        {
            let hashes = c[i + 1..].iter().take_while(|&&h| h == '#').count();
            let open = i + 1 + hashes;
            if c.get(open) != Some(&'"') {
                i += 1;
                continue;
            }
            let mut end = open + 1;
            while end < c.len()
                && !(c[end] == '"'
                    && c[end + 1..]
                        .iter()
                        .take(hashes)
                        .filter(|&&h| h == '#')
                        .count()
                        == hashes)
            {
                end += 1;
            }
            out.push(c[open + 1..end].iter().collect());
            i = end + 1 + hashes;
        } else if c[i] == '"' {
            let mut text = String::new();
            i += 1;
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' {
                    i += 1;
                    match c.get(i) {
                        Some('\n') => {
                            while c
                                .get(i + 1)
                                .is_some_and(|w| w.is_whitespace())
                            {
                                i += 1;
                            }
                        }
                        Some('n') => text.push('\n'),
                        Some('t') => text.push('\t'),
                        Some(&other) => text.push(other),
                        None => {}
                    }
                } else {
                    text.push(c[i]);
                }
                i += 1;
            }
            out.push(text);
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests;
