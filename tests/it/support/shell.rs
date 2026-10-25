use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

use super::replay::Replay;

const PROXY_VARIABLES: [&str; 6] = [
    "ALL_PROXY",
    "all_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

pub struct Case {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub exit: i32,
    pub stdout_closed: bool,
    pub stdout: String,
    pub stderr: String,
    header: Vec<String>,
}

pub struct Outcome {
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
    pub requests: usize,
}

/// Splits like a POSIX shell for the cases a `.case` file needs: single
/// quotes are literal, double quotes honour backslash escapes, and a
/// backslash outside quotes escapes the next character.
pub fn shell_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("unterminated '".to_owned()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err("unterminated \"".to_owned()),
                        },
                        Some(c) => word.push(c),
                        None => return Err("unterminated \"".to_owned()),
                    }
                }
            }
            '\\' => {
                started = true;
                match chars.next() {
                    Some(c) => word.push(c),
                    None => return Err("trailing backslash".to_owned()),
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

fn is_section(line: &str) -> bool {
    line == "--- stdout" || line == "--- stderr"
}

impl Case {
    pub fn parse(text: &str, path: &Path) -> Result<Case, String> {
        let mut lines = text.lines().peekable();
        let mut header = Vec::new();
        while let Some(line) = lines.next_if(|line| !is_section(line)) {
            header.push(line.to_owned());
        }
        let mut args = None;
        let mut env = Vec::new();
        let mut exit = None;
        let mut stdout_closed = false;
        for line in &header {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once(':')
                .map(|(key, value)| (key, value.trim()))
                .ok_or_else(|| format!("not a key: value line: {line}"))?;
            match key {
                "args" => args = Some(shell_words(value)?),
                "env" => {
                    let (name, value) = value
                        .split_once('=')
                        .ok_or_else(|| format!("env needs K=V: {line}"))?;
                    env.push((name.to_owned(), value.to_owned()));
                }
                "exit" => {
                    exit = Some(
                        value
                            .parse::<i32>()
                            .map_err(|_| format!("bad exit: {value}"))?,
                    );
                }
                "stdout" if value == "closed" => stdout_closed = true,
                "stdout" => return Err(format!("stdout: {value}")),
                _ => return Err(format!("unknown key: {key}")),
            }
        }
        let (mut stdout, mut stderr) = (String::new(), String::new());
        while let Some(marker) = lines.next() {
            let section = match marker {
                "--- stdout" => &mut stdout,
                "--- stderr" => &mut stderr,
                other => return Err(format!("unknown section: {other}")),
            };
            while let Some(line) = lines.next_if(|line| !is_section(line)) {
                section.push_str(line);
                section.push('\n');
            }
        }
        Ok(Case {
            path: path.to_path_buf(),
            args: args.ok_or("no args: line")?,
            env,
            exit: exit.ok_or("no exit: line")?,
            stdout_closed,
            stdout,
            stderr,
            header,
        })
    }

    pub fn load(path: &Path) -> Case {
        let text = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Case::parse(&text, path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// The command that runs the case against `url`, with `cache` as its
    /// `SEGO_CACHE_DIR`.
    pub fn command(&self, url: &str, cache: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sego"));
        command.args(&self.args).stdin(Stdio::null());
        for name in PROXY_VARIABLES {
            command.env_remove(name);
        }
        command.env("SEGO_ENDPOINT", url);
        command.env("SEGO_CACHE_DIR", cache);
        for (name, value) in &self.env {
            command.env(name, value);
        }
        command
    }

    pub fn run(&self, replay: &Replay) -> Outcome {
        let url = replay.url();
        let cache = TempDir::new()
            .unwrap_or_else(|e| panic!("{}: {e}", self.path.display()));
        let mut command = self.command(&url, cache.path());
        if self.stdout_closed {
            let (reader, writer) =
                std::io::pipe().unwrap_or_else(|e| panic!("pipe: {e}"));
            drop(reader);
            command.stdout(writer);
        }
        let output = command
            .output()
            .unwrap_or_else(|e| panic!("{}: {e}", self.path.display()));
        Outcome {
            exit: output.status.code().unwrap_or(-1),
            stdout: stand_in(&output.stdout, &url),
            stderr: stand_in(&output.stderr, &url),
            requests: replay.take_requests(),
        }
    }

    /// `None` when the outcome is what the case expects.
    pub fn failure(&self, outcome: &Outcome) -> Option<String> {
        let mut message = String::new();
        if outcome.exit != self.exit {
            let _ = writeln!(
                message,
                "exit: expected {}, got {}",
                self.exit, outcome.exit
            );
        }
        if !self.stdout_closed && outcome.stdout != self.stdout {
            let _ = write!(
                message,
                "stdout:\n{}",
                line_diff(&self.stdout, &outcome.stdout)
            );
        }
        if outcome.stderr != self.stderr {
            let _ = write!(
                message,
                "stderr:\n{}",
                line_diff(&self.stderr, &outcome.stderr)
            );
        }
        (!message.is_empty()).then_some(message)
    }

    /// Rewrites the file's `exit:` line and sections from the outcome.
    pub fn bless(&self, outcome: &Outcome) {
        let mut text = String::new();
        for line in &self.header {
            if line.starts_with("exit:") {
                let _ = writeln!(text, "exit: {}", outcome.exit);
            } else {
                let _ = writeln!(text, "{line}");
            }
        }
        for (name, content) in [
            ("stdout", if self.stdout_closed { "" } else { &outcome.stdout }),
            ("stderr", &outcome.stderr),
        ] {
            if !content.is_empty() {
                let _ = write!(text, "--- {name}\n{content}");
            }
        }
        fs::write(&self.path, text)
            .unwrap_or_else(|e| panic!("{}: {e}", self.path.display()));
    }
}

/// The replay server stands in for sourcegraph.com, so its address reads as
/// that host in what sego printed.
fn stand_in(output: &[u8], url: &str) -> String {
    let host = url.strip_prefix("http://").unwrap_or(url);
    String::from_utf8_lossy(output).replace(host, "sourcegraph.com")
}

/// A longest-common-subsequence diff: `-` lines are expected only, `+` lines
/// are actual only.
fn line_diff(expected: &str, actual: &str) -> String {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut diff = String::new();
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            let _ = writeln!(diff, "  {}", a[i]);
            i += 1;
            j += 1;
        } else if j == b.len()
            || (i < a.len() && table[i + 1][j] >= table[i][j + 1])
        {
            let _ = writeln!(diff, "- {}", a[i]);
            i += 1;
        } else {
            let _ = writeln!(diff, "+ {}", b[j]);
            j += 1;
        }
    }
    if expected.ends_with('\n') != actual.ends_with('\n') {
        diff.push_str("(the trailing newline differs)\n");
    }
    diff
}
