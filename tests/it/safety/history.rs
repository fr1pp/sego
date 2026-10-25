use std::ffi::OsString;
use std::process::ExitCode;

use serde_json::{Value, json};

use super::{
    COMMIT, CORPUS, ESCAPED, ESCAPED_MESSAGE, REPO, Ran, assert_safe,
    fixtures, hostile, hostile_name, json, sego,
};
use sego::internal::escape::is_unsafe;

const HEAD: &str = COMMIT;
const NEXT: &str = "89abcdef0123456789abcdef0123456789abcdef";
const PARENT: &str = "fedcba9876543210fedcba9876543210fedcba98";
const HEAD12: &str = "0123456789ab";
const NEXT12: &str = "89abcdef0123";
const PARENT12: &str = "fedcba987654";

/// `hostile()` inside `$'…'`, as `shell_word` writes it.
const QUOTED: &str = "esc\\033[0mrtl\\342\\200\\256xzw\\342\\200\\213xnew\\nlinecr\\015nul\\000xls\\342\\200\\250xdel\\177x";
/// `hostile()` as `render_line` prints it: a line break inside a line is
/// escaped too.
const RENDERED: &str = "esc\\u{1b}[0mrtl\\u{202e}xzw\\u{200b}xnew\\u{a}\
                        linecr\\u{d}nul\\u{0}xls\\u{2028}xdel\\u{7f}x";
/// The first 12 characters of `hostile()`, as an abbreviated id.
const ESCAPED_OID: &str = "esc\\u{1b}[0mrtl\\u{202e}x";
/// The first 10 characters of `hostile()`, as a date.
const ESCAPED_DATE: &str = "esc\\u{1b}[0mrtl";

fn mode(json: bool) -> &'static str {
    if json { "json" } else { "text" }
}

/// `hostile()` without `[`, which would make a path a glob.
fn plain() -> String {
    hostile().replace('[', "")
}

fn plain_escaped() -> String {
    ESCAPED.replace('[', "")
}

fn plain_quoted() -> String {
    QUOTED.replace('[', "")
}

/// The lead of a header: the hostile repository at `suffix`, as one shell
/// word.
fn lead(suffix: &str) -> String {
    format!("$'{REPO}{QUOTED}@{suffix}'")
}

/// A reference escape for a single line.
fn escaped(line: &str) -> String {
    line.chars()
        .map(|c| {
            if is_unsafe(c) {
                format!("\\u{{{:x}}}", u32::from(c))
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn run(command: &str, as_json: bool, operands: Vec<String>) -> Ran {
    let mut args: Vec<OsString> = vec![command.into()];
    if as_json {
        args.push("--json".into());
    }
    args.extend(operands.into_iter().map(OsString::from));
    sego(args)
}

fn diff(scenario: &str, as_json: bool, paths: &[String]) -> Ran {
    let mode = mode(as_json);
    let mut operands =
        vec![format!("{REPO}@{scenario}-{mode}-a...{scenario}-{mode}-b")];
    operands.extend(paths.iter().cloned());
    run("diff", as_json, operands)
}

fn commit(scenario: &str, as_json: bool) -> Ran {
    let mode = mode(as_json);
    run("show", as_json, vec![format!("{REPO}@{scenario}-{mode}")])
}

fn log_rev(scenario: &str, as_json: bool, path: Option<&str>) -> Ran {
    let mode = mode(as_json);
    let mut address = format!("{REPO}@{scenario}-{mode}");
    if let Some(path) = path {
        address.push(':');
        address.push_str(path);
    }
    run("log", as_json, vec![address])
}

fn log_range(scenario: &str, as_json: bool, path: Option<&str>) -> Ran {
    let mode = mode(as_json);
    let mut address =
        format!("{REPO}@{scenario}-{mode}-a..{scenario}-{mode}-b");
    if let Some(path) = path {
        address.push(':');
        address.push_str(path);
    }
    run("log", as_json, vec![address])
}

fn diff_runs(as_json: bool) -> Vec<(&'static str, Ran)> {
    vec![
        ("list", diff("list", as_json, &[])),
        ("patch", diff("patch", as_json, &[plain()])),
        (
            "missing-path",
            diff("missing-path", as_json, &[format!("{}-gone", plain())]),
        ),
        ("failed", diff("failed", as_json, &[])),
        ("rejected", diff("rejected", as_json, &[])),
    ]
}

fn commit_runs(as_json: bool) -> Vec<(&'static str, Ran)> {
    ["commit", "missing", "failed", "rejected"]
        .into_iter()
        .map(|scenario| (scenario, commit(scenario, as_json)))
        .collect()
}

fn log_runs(as_json: bool) -> Vec<(&'static str, Ran)> {
    let gone = format!("{}-gone", plain());
    vec![
        ("rev", log_rev("rev", as_json, Some(&hostile()))),
        ("range", log_range("range", as_json, None)),
        ("range-path", log_range("range-path", as_json, Some(&hostile()))),
        ("missing-path", log_rev("missing-path", as_json, Some(&gone))),
        ("failed", log_range("failed", as_json, None)),
        ("rejected", log_rev("rejected", as_json, None)),
    ]
}

/// The patch the diff fixtures serve for the plain hostile path.
fn patch() -> String {
    let plain = plain();
    let hostile = hostile();
    [
        format!("diff --git {plain} {plain}"),
        format!("--- {plain}"),
        format!("+++ {plain}"),
        "@@ -1 +1 @@".to_owned(),
        format!("-{hostile}"),
        format!("+{hostile}"),
    ]
    .join("\n")
        + "\n"
}

/// The message the commit fixtures serve, as the server stores it.
fn message() -> String {
    let mut all: Vec<String> = CORPUS.map(str::to_owned).to_vec();
    all.extend(["mid\rcr".to_owned(), "tab\tok".to_owned()]);
    format!("{}\n", all.join("\n"))
}

fn fixture_response(name: &str) -> Value {
    fixtures()
        .iter()
        .find(|f| f.file.ends_with(name))
        .unwrap_or_else(|| panic!("no {name}"))
        .response
        .clone()
        .unwrap_or(Value::Null)
}

fn fixture_variables(name: &str) -> Value {
    fixtures()
        .iter()
        .find(|f| f.file.ends_with(name))
        .and_then(|f| f.variables.clone())
        .unwrap_or_else(|| panic!("no variables in {name}"))
}

#[test]
fn the_history_fixtures_carry_the_corpus() {
    let h = hostile();
    for as_json in [false, true] {
        let mode = mode(as_json);
        for scenario in ["list", "patch", "missing-path", "failed", "rejected"]
        {
            let name = format!("diff-{scenario}-{mode}.yaml");
            let vars = fixture_variables(&name);
            assert_eq!(vars["a"], format!("{scenario}-{mode}-a"), "{name}");
        }
        for scenario in ["commit", "missing", "failed", "rejected"] {
            let name = format!("commit-{scenario}-{mode}.yaml");
            let vars = fixture_variables(&name);
            assert_eq!(vars["rev"], format!("{scenario}-{mode}"), "{name}");
        }
        for (scenario, ranged) in [
            ("rev", false),
            ("range", true),
            ("range-path", true),
            ("missing-path", false),
            ("failed", true),
            ("rejected", false),
        ] {
            let name = format!("log-{scenario}-{mode}.yaml");
            let vars = fixture_variables(&name);
            let key = if ranged { "a" } else { "rev" };
            let expected = if ranged {
                format!("{scenario}-{mode}-a")
            } else {
                format!("{scenario}-{mode}")
            };
            assert_eq!(vars[key], expected, "{name}");
        }

        let list = fixture_response(&format!("diff-list-{mode}.yaml"));
        let c0 = &list["data"]["c0"];
        assert_eq!(c0["name"], hostile_name());
        let nodes = &c0["comparison"]["page"]["nodes"];
        assert_eq!(nodes[0]["newPath"], h);
        assert_eq!(nodes[1]["oldPath"], format!("o-{h}"));
        assert_eq!(nodes[2]["newPath"], format!("{h}-added"));
        assert_eq!(nodes[3]["oldPath"], format!("{h}-deleted"));

        let patched = fixture_response(&format!("diff-patch-{mode}.yaml"));
        let c0 = &patched["data"]["c0"]["comparison"];
        assert_eq!(c0["patch"]["rawDiff"], patch());
        assert_eq!(
            fixture_variables(&format!("diff-patch-{mode}.yaml"))["paths"][0],
            plain()
        );

        let shown = fixture_response(&format!("commit-commit-{mode}.yaml"));
        let commit = &shown["data"]["m0"]["commit"];
        assert_eq!(commit["subject"], h);
        assert_eq!(commit["message"], message());
        assert_eq!(commit["author"]["person"]["displayName"], h);
        assert_eq!(
            commit["committer"]["person"]["displayName"],
            format!("c-{h}")
        );
        assert_eq!(
            shown["data"]["d0"]["comparison"]["page"]["nodes"][0]["newPath"],
            h
        );

        let logged = fixture_response(&format!("log-rev-{mode}.yaml"));
        let nodes = &logged["data"]["r0"]["commit"]["ancestors"]["nodes"];
        assert_eq!(nodes[0]["subject"], h);
        assert_eq!(nodes[0]["author"]["person"]["displayName"], h);
        assert_eq!(nodes[2]["oid"], h);
        let tags = &logged["data"]["t0"]["gitRefs"]["nodes"];
        assert_eq!(tags[0]["displayName"], h);
        assert_eq!(tags[1]["displayName"], format!("t-{h}"));
        assert_eq!(tags[2]["displayName"], format!("u-{h}"));
        assert_eq!(
            fixture_variables(&format!("log-rev-{mode}.yaml"))["path"],
            h
        );
    }
}

/// The run was answered from its fixture: an answer exits 0, and every
/// other scenario is one of sego's own exit-1 messages, never an HTTP error
/// from a missing fixture.
fn assert_served(what: &str, ran: &Ran, answered: bool) {
    if answered {
        assert_eq!(ran.code, ExitCode::SUCCESS, "{what}: {}", ran.err);
        assert!(!ran.out.is_empty(), "{what}");
    } else {
        assert_eq!(ran.code, ExitCode::from(1), "{what}: {}", ran.err);
        assert!(ran.out.is_empty(), "{what}: {:?}", ran.out);
        assert!(!ran.err.contains("HTTP"), "{what}: {:?}", ran.err);
    }
}

#[test]
fn no_unsafe_character_reaches_any_diff_output() {
    for as_json in [false, true] {
        for (scenario, ran) in diff_runs(as_json) {
            let what = format!("diff {scenario} json={as_json}");
            assert_served(&what, &ran, ["list", "patch"].contains(&scenario));
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn no_unsafe_character_reaches_any_commit_output() {
    for as_json in [false, true] {
        for (scenario, ran) in commit_runs(as_json) {
            let what = format!("show commit {scenario} json={as_json}");
            assert_served(&what, &ran, ["commit"].contains(&scenario));
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn no_unsafe_character_reaches_any_log_output() {
    for as_json in [false, true] {
        for (scenario, ran) in log_runs(as_json) {
            let what = format!("log {scenario} json={as_json}");
            assert_served(
                &what,
                &ran,
                ["rev", "range", "range-path"].contains(&scenario),
            );
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

fn assert_success(what: &str, ran: &Ran) {
    assert_eq!(ran.code, ExitCode::SUCCESS, "{what}: {}", ran.err);
    assert!(ran.err.is_empty(), "{what}: {:?}", ran.err);
}

fn assert_exit_one(what: &str, ran: &Ran, stderr: &str) {
    assert_eq!(ran.code, ExitCode::from(1), "{what}: {}", ran.err);
    assert!(ran.out.is_empty(), "{what}: {:?}", ran.out);
    assert_eq!(ran.err, stderr, "{what}");
}

/// A `diff` file row: a status letter, `+N -M`, then a non-empty path.
fn file_row(line: &str) -> bool {
    let mut parts = line.splitn(4, "  ");
    let status = parts.next().unwrap_or_default();
    let counts = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let counted = counts.strip_prefix('+').is_some_and(|rest| {
        rest.split_once(" -").is_some_and(|(added, deleted)| {
            !added.is_empty()
                && added.bytes().all(|b| b.is_ascii_digit())
                && !deleted.is_empty()
                && deleted.bytes().all(|b| b.is_ascii_digit())
        })
    });
    matches!(status, "A" | "D" | "R" | "M")
        && counted
        && !path.is_empty()
        && !path.starts_with(' ')
}

/// The header, then `rows` file rows; with a patch, a blank line and
/// `patch` lines of four-space indent.
fn assert_diff_layout(out: &str, rows: usize, patch: usize) {
    assert!(out.ends_with('\n'), "{out:?}");
    let lines: Vec<&str> =
        out.strip_suffix('\n').unwrap_or(out).split('\n').collect();
    let blank = usize::from(patch > 0);
    assert_eq!(lines.len(), 1 + rows + blank + patch, "{out:?}");
    assert!(lines[0].starts_with("$'github.com/hostile/repo"), "{out:?}");
    assert!(
        lines[0].contains("...") && lines[0].contains("  rev "),
        "{out:?}"
    );
    for (i, line) in lines[1..=rows].iter().enumerate() {
        assert!(file_row(line), "row {i}: {line:?}");
    }
    if patch > 0 {
        assert_eq!(lines[1 + rows], "", "{out:?}");
        for (i, line) in lines[2 + rows..].iter().enumerate() {
            assert!(line.starts_with("    "), "patch line {i}: {line:?}");
        }
    }
}

#[test]
fn diff_text_has_only_the_layout_lines() {
    let ran = diff("list", false, &[]);
    assert_success("list", &ran);
    assert_diff_layout(&ran.out, 4, 0);
    let q = QUOTED;
    assert_eq!(
        ran.out,
        format!(
            "{}  4 files  +7 -7  base {PARENT12}  rev list-text-a...list-text-b\n\
             M  +1 -1  $'{q}'\n\
             R  +2 -2  $'o-{q}' -> $'n-{q}'\n\
             A  +4 -0  $'{q}-added'\n\
             D  +0 -4  $'{q}-deleted'\n",
            lead(&format!("{HEAD12}...{NEXT12}"))
        )
    );

    let ran = diff("patch", false, &[plain()]);
    assert_success("patch", &ran);
    let patch = patch();
    let patch_lines: Vec<String> = patch
        .split_terminator('\n')
        .map(|line| format!("    {}", escaped(line)))
        .collect();
    assert_diff_layout(&ran.out, 1, patch_lines.len());
    let q = plain_quoted();
    assert_eq!(
        ran.out,
        format!(
            "{}  1 file  +1 -1  base {PARENT12}  rev patch-text-a...patch-text-b\n\
             M  +1 -1  $'{q}'\n\n{}\n",
            lead(&format!("{HEAD12}...{NEXT12}:{q}")),
            patch_lines.join("\n")
        )
    );
}

#[test]
fn diff_json_values_equal_the_hostile_originals() {
    let h = hostile();
    let ran = diff("list", true, &[]);
    assert_success("list", &ran);
    let value = json(&ran);
    assert_eq!(value["repo"], hostile_name());
    assert_eq!(value["rev_a"], "list-json-a");
    assert_eq!(value["rev_b"], "list-json-b");
    assert_eq!(value["commit_a"], HEAD);
    assert_eq!(value["commit_b"], NEXT);
    assert_eq!(value["base"], PARENT);
    assert_eq!(value["files_total"], 4);
    assert_eq!(value["added"], 7);
    assert_eq!(value["deleted"], 7);
    assert_eq!(value["patch"], Value::Null);
    assert_eq!(
        value["files"],
        json!([
            {"status": "M", "old_path": h, "new_path": h,
             "added": 1, "deleted": 1},
            {"status": "R", "old_path": format!("o-{h}"),
             "new_path": format!("n-{h}"), "added": 2, "deleted": 2},
            {"status": "A", "old_path": null, "new_path": format!("{h}-added"),
             "added": 4, "deleted": 0},
            {"status": "D", "old_path": format!("{h}-deleted"),
             "new_path": null, "added": 0, "deleted": 4},
        ])
    );

    let ran = diff("patch", true, &[plain()]);
    assert_success("patch", &ran);
    let value = json(&ran);
    assert_eq!(value["patch"], patch());
    assert_eq!(value["files"][0]["new_path"], plain());
    assert_eq!(value["files"][0]["old_path"], plain());
}

#[test]
fn diff_error_paths_fail_with_the_exact_message() {
    let gone = format!("{}-gone", plain());
    let cases = [
        (
            "missing-path",
            vec![gone],
            format!(
                "sego: error: {}-gone not found in {REPO}{ESCAPED}@{HEAD12} \
                 or @{NEXT12}\n",
                plain_escaped()
            ),
        ),
        (
            "failed",
            vec![],
            "sego: error: Sourcegraph failed to answer the comparison\n"
                .to_owned(),
        ),
        (
            "rejected",
            vec![],
            format!(
                "sego: error: Sourcegraph rejected sego's request: \
                 {ESCAPED_MESSAGE}\n"
            ),
        ),
    ];
    for (scenario, paths, stderr) in cases {
        for as_json in [false, true] {
            let ran = diff(scenario, as_json, &paths);
            assert_exit_one(
                &format!("diff {scenario} json={as_json}"),
                &ran,
                &stderr,
            );
        }
    }
}

/// The header, `message` indented lines (blank ones empty), then with
/// `rows` file rows a blank line and the rows.
fn assert_commit_layout(out: &str, message: usize, rows: usize) {
    assert!(out.ends_with('\n'), "{out:?}");
    let lines: Vec<&str> =
        out.strip_suffix('\n').unwrap_or(out).split('\n').collect();
    let blank = usize::from(rows > 0);
    assert_eq!(lines.len(), 1 + message + blank + rows, "{out:?}");
    assert!(lines[0].starts_with("$'github.com/hostile/repo"), "{out:?}");
    assert!(lines[0].contains("  parents "), "{out:?}");
    for (i, line) in lines[1..=message].iter().enumerate() {
        assert!(
            line.is_empty() || line.starts_with("    "),
            "message line {i}: {line:?}"
        );
    }
    if rows > 0 {
        assert_eq!(lines[1 + message], "", "{out:?}");
        for (i, line) in lines[2 + message..].iter().enumerate() {
            assert!(file_row(line), "row {i}: {line:?}");
        }
    }
}

#[test]
fn commit_text_has_only_the_layout_lines() {
    let ran = commit("commit", false);
    assert_success("commit", &ran);
    let message_lines: Vec<String> = message()
        .replace("\r\n", "\n")
        .split_terminator('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {}", escaped(line))
            }
        })
        .collect();
    assert_commit_layout(&ran.out, message_lines.len(), 4);
    let q = QUOTED;
    assert_eq!(
        ran.out,
        format!(
            "{}  {ESCAPED_DATE}  {ESCAPED}  parents {PARENT12}  4 files  +7 -7  \
             rev commit-text\n{}\n\n\
             M  +1 -1  $'{q}'\n\
             R  +2 -2  $'o-{q}' -> $'n-{q}'\n\
             A  +4 -0  $'{q}-added'\n\
             D  +0 -4  $'{q}-deleted'\n",
            lead(NEXT12),
            message_lines.join("\n")
        )
    );
}

#[test]
fn commit_json_values_equal_the_hostile_originals() {
    let h = hostile();
    let ran = commit("commit", true);
    assert_success("commit", &ran);
    let value = json(&ran);
    assert_eq!(value["repo"], hostile_name());
    assert_eq!(value["rev"], "commit-json");
    assert_eq!(value["commit"], NEXT);
    assert_eq!(value["author"], h);
    assert_eq!(value["author_date"], h.chars().take(10).collect::<String>());
    assert_eq!(value["committer"], format!("c-{h}"));
    assert_eq!(value["committer_date"], "2026-03-01");
    assert_eq!(value["parents"], json!([PARENT]));
    assert_eq!(value["subject"], h);
    assert_eq!(value["message"], message().replace("\r\n", "\n"));
    assert_eq!(value["files_total"], 4);
    assert_eq!(value["added"], 7);
    assert_eq!(value["deleted"], 7);
    assert_eq!(
        value["files"],
        json!([
            {"status": "M", "old_path": h, "new_path": h,
             "added": 1, "deleted": 1},
            {"status": "R", "old_path": format!("o-{h}"),
             "new_path": format!("n-{h}"), "added": 2, "deleted": 2},
            {"status": "A", "old_path": null, "new_path": format!("{h}-added"),
             "added": 4, "deleted": 0},
            {"status": "D", "old_path": format!("{h}-deleted"),
             "new_path": null, "added": 0, "deleted": 4},
        ])
    );
}

#[test]
fn commit_error_paths_fail_with_the_exact_message() {
    for as_json in [false, true] {
        let mode = mode(as_json);
        let cases = [
            (
                "missing",
                format!(
                    "sego: error: revision missing-{mode} not found in \
                     {REPO}{ESCAPED}\n"
                ),
            ),
            (
                "failed",
                "sego: error: Sourcegraph failed to answer the file list\n"
                    .to_owned(),
            ),
            (
                "rejected",
                format!(
                    "sego: error: Sourcegraph rejected sego's request: \
                     {ESCAPED_MESSAGE}\n"
                ),
            ),
        ];
        for (scenario, stderr) in cases {
            let ran = commit(scenario, as_json);
            assert_exit_one(
                &format!("show commit {scenario} json={as_json}"),
                &ran,
                &stderr,
            );
        }
    }
}

/// The three commits the log fixtures serve, as rows print them.
fn log_rows() -> Vec<String> {
    let (a, r) = (ESCAPED, RENDERED);
    vec![
        format!("{HEAD12}  2026-08-01  {a}  (tag: {a}, tag: t-{a}) {r}"),
        format!("{NEXT12}  {ESCAPED_DATE}  b-{a}  b-{r}"),
        format!("{ESCAPED_OID}  2026-07-01  c-{a}  (tag: u-{a}) c-{r}"),
    ]
}

/// A header, then `rows` rows of `commit  date  author  subject`.
fn assert_log_layout(out: &str, rows: usize) {
    assert!(out.ends_with('\n'), "{out:?}");
    let lines: Vec<&str> =
        out.strip_suffix('\n').unwrap_or(out).split('\n').collect();
    assert_eq!(lines.len(), 1 + rows, "{out:?}");
    assert!(lines[0].starts_with("$'github.com/hostile/repo"), "{out:?}");
    for (i, line) in lines[1..].iter().enumerate() {
        let fields: Vec<&str> = line.splitn(4, "  ").collect();
        assert!(!line.starts_with(' '), "row {i}: {line:?}");
        assert_eq!(fields.len(), 4, "row {i}: {line:?}");
        assert!(fields.iter().all(|f| !f.is_empty()), "row {i}: {line:?}");
    }
}

#[test]
fn log_text_has_only_the_layout_lines() {
    let q = QUOTED;
    let rows = log_rows().join("\n");

    let ran = log_rev("rev", false, Some(&hostile()));
    assert_success("rev", &ran);
    assert_log_layout(&ran.out, 3);
    assert_eq!(
        ran.out,
        format!(
            "$'{REPO}{q}@{HEAD12}:{q}'  3 commits  rev rev-text\n{rows}\n"
        )
    );

    let ran = log_range("range", false, None);
    assert_success("range", &ran);
    assert_log_layout(&ran.out, 3);
    assert_eq!(
        ran.out,
        format!(
            "$'{REPO}{q}@{PARENT12}..{HEAD12}'  3 commits  \
             rev range-text-a..range-text-b\n{rows}\n"
        )
    );

    let ran = log_range("range-path", false, Some(&hostile()));
    assert_success("range-path", &ran);
    assert_log_layout(&ran.out, 3);
    assert_eq!(
        ran.out,
        format!(
            "$'{REPO}{q}@{PARENT12}..{HEAD12}:{q}'  3 commits  \
             rev range-path-text-a..range-path-text-b\n{rows}\n"
        )
    );
}

fn log_commits() -> Value {
    let h = hostile();
    json!([
        {"commit": HEAD, "date": "2026-08-01", "author": h, "subject": h,
         "parents": [NEXT], "tags": [h, format!("t-{h}")]},
        {"commit": NEXT, "date": h.chars().take(10).collect::<String>(),
         "author": format!("b-{h}"), "subject": format!("b-{h}"),
         "parents": [h], "tags": []},
        {"commit": h, "date": "2026-07-01", "author": format!("c-{h}"),
         "subject": format!("c-{h}"), "parents": [],
         "tags": [format!("u-{h}")]},
    ])
}

#[test]
fn log_json_values_equal_the_hostile_originals() {
    let h = hostile();

    let ran = log_rev("rev", true, Some(&h));
    assert_success("rev", &ran);
    let value = json(&ran);
    assert_eq!(value["repo"], hostile_name());
    assert_eq!(value["rev"], "rev-json");
    assert_eq!(value["commit"], HEAD);
    assert_eq!(value["path"], h);
    assert_eq!(value["total"], Value::Null);
    assert_eq!(value["more"], false);
    assert_eq!(value["commits"], log_commits());

    let ran = log_range("range", true, None);
    assert_success("range", &ran);
    let value = json(&ran);
    assert_eq!(value["repo"], hostile_name());
    assert_eq!(value["rev_a"], "range-json-a");
    assert_eq!(value["rev_b"], "range-json-b");
    assert_eq!(value["commit_a"], PARENT);
    assert_eq!(value["commit_b"], HEAD);
    assert_eq!(value["path"], Value::Null);
    assert_eq!(value["total"], 3);
    assert_eq!(value["commits"], log_commits());

    let ran = log_range("range-path", true, Some(&h));
    assert_success("range-path", &ran);
    let value = json(&ran);
    assert_eq!(value["path"], h);
    assert_eq!(value["total"], Value::Null);
    assert_eq!(value["commits"], log_commits());
}

#[test]
fn log_error_paths_fail_with_the_exact_message() {
    let cases = [
        (
            "missing-path",
            format!(
                "sego: error: {}-gone not found in {REPO}{ESCAPED}@{HEAD12}\n",
                plain_escaped()
            ),
        ),
        (
            "failed",
            "sego: error: Sourcegraph failed to answer the commit count\n"
                .to_owned(),
        ),
        (
            "rejected",
            format!(
                "sego: error: Sourcegraph rejected sego's request: \
                 {ESCAPED_MESSAGE}\n"
            ),
        ),
    ];
    let gone = format!("{}-gone", plain());
    for (scenario, stderr) in cases {
        for as_json in [false, true] {
            let ran = match scenario {
                "missing-path" => log_rev(scenario, as_json, Some(&gone)),
                "failed" => log_range(scenario, as_json, None),
                _ => log_rev(scenario, as_json, None),
            };
            assert_exit_one(
                &format!("log {scenario} json={as_json}"),
                &ran,
                &stderr,
            );
        }
    }
}
