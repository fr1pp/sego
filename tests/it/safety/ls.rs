use std::ffi::OsString;
use std::process::ExitCode;

use serde_json::Value;

use super::{
    COMMIT, ESCAPED, ESCAPED_MESSAGE, REPO, Ran, assert_clean_error,
    assert_not_served, assert_safe, hostile, hostile_name, json, requested,
    sego,
};

fn ls(path: &str, json: bool) -> Ran {
    let mut args: Vec<OsString> = vec!["ls".into()];
    if json {
        args.push("--json".into());
    }
    let address = if path.is_empty() {
        REPO.to_owned()
    } else {
        format!("{REPO}:{path}")
    };
    args.push(address.into());
    sego(args)
}

const LS_PATHS: [&str; 9] = [
    "",
    "HOSTILE",
    "sub",
    "f.txt",
    "bin.dat",
    "HOSTILE-gone",
    "absent",
    "broken.txt",
    "rejected.txt",
];
const LS_ERRORS: [&str; 5] =
    ["f.txt", "bin.dat", "HOSTILE-gone", "broken.txt", "rejected.txt"];
const LS_KINDS: [&str; 3] = ["file", "dir", "submodule"];
/// The hostile repository at `COMMIT`, as an address prints it: `$'…'`.
const QUOTED_ADDRESS: &str = "$'github.com/hostile/repoesc\\033[0mrtl\\342\\200\\256xzw\\342\\200\\213xnew\\nlinecr\\015nul\\000xls\\342\\200\\250xdel\\177x@0123456789ab'";
/// Header, then one `kind  path` row per entry, nothing else. `kinds` counts
/// the rows of each of `LS_KINDS`.
fn assert_ls_layout(ran: &Ran, kinds: [usize; 3]) {
    assert!(ran.out.ends_with('\n'), "{:?}", ran.out);
    let body = ran.out.strip_suffix('\n').unwrap_or(&ran.out);
    let lines: Vec<&str> = body.split('\n').collect();
    let header = lines[0];
    assert!(header.starts_with("$'github.com/hostile/repo"), "{header:?}");
    assert!(header.contains("@0123456789ab"), "{header:?}");
    let mut seen = [0; 3];
    for (i, line) in lines.iter().enumerate().skip(1) {
        let kind = LS_KINDS.iter().position(|kind| {
            line.strip_prefix(&format!("{kind:<9}  "))
                .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(' '))
        });
        match kind {
            Some(kind) => seen[kind] += 1,
            None => panic!("line {i} is not a row: {line:?}"),
        }
    }
    assert_eq!(seen, kinds, "{:?}", ran.out);
    assert_eq!(lines.len(), 1 + kinds.iter().sum::<usize>(), "{:?}", ran.out);
}

#[test]
fn no_unsafe_character_reaches_any_ls_output() {
    for path in LS_PATHS {
        for json in [false, true] {
            let ran = ls(&requested(path), json);
            let what = format!("ls {path:?} json={json}");
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn ls_text_has_only_the_layout_lines() {
    let ran = ls("", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert_ls_layout(&ran, [1, 1, 2]);
    assert!(
        ran.out.lines().next().is_some_and(|header| header
            .ends_with("  4 entries  1 dir  1 file  2 submodules")),
        "{:?}",
        ran.out
    );
    assert!(
        ran.out.contains("\nsubmodule  mod  example.com/hostile/lib@esc\\u{1b}[0mrtl\\u{202e}x\n"),
        "{:?}",
        ran.out
    );
    assert!(
        ran.out.contains(&format!("https://example.com/{ESCAPED}\n")),
        "{:?}",
        ran.out
    );

    let ran = ls(&hostile(), false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_ls_layout(&ran, [1, 1, 1]);

    let ran = ls("sub", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_ls_layout(&ran, [0, 0, 1]);
}

#[test]
fn ls_json_values_equal_the_hostile_originals() {
    let ran = ls("", true);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    let value = json(&ran);
    assert_eq!(value["repo"], hostile_name());
    assert_eq!(value["path"], Value::Null);
    let entries = value["entries"].as_array().expect("entries");
    let paths: Vec<&str> =
        entries.iter().map(|e| e["path"].as_str().unwrap_or("")).collect();
    let dir = format!("{}-dir", hostile());
    let file = hostile();
    let expected = [file.as_str(), dir.as_str(), "mod", "sub"];
    for path in expected {
        assert!(paths.contains(&path), "{path:?} not in {paths:?}");
    }
    assert_eq!(paths.len(), expected.len(), "{paths:?}");
    let entry = |path: &str| {
        entries
            .iter()
            .find(|e| e["path"] == path)
            .unwrap_or_else(|| panic!("no {path:?}"))
    };
    assert_eq!(entry("sub")["kind"], "submodule");
    assert_eq!(
        entry("sub")["submodule_url"],
        format!("https://example.com/{}", hostile())
    );
    assert_eq!(entry("sub")["submodule_target"], Value::Null);
    assert_eq!(entry("mod")["submodule_commit"], hostile());
    assert_eq!(
        entry("mod")["submodule_target"],
        format!(
            "example.com/hostile/lib@{}",
            hostile().chars().take(12).collect::<String>()
        )
    );
    assert_eq!(entry(&dir)["kind"], "dir");
    assert_eq!(entry(&hostile())["kind"], "file");

    let value = json(&ls(&hostile(), true));
    assert_eq!(value["path"], hostile());
    let paths: Vec<&str> = value["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["path"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(paths.len(), 3, "{paths:?}");
    assert!(paths.contains(&format!("{0}/{0}", hostile()).as_str()));
    assert!(paths.contains(&format!("{0}/{0}-dir", hostile()).as_str()));
    assert!(paths.contains(&format!("{}/sub", hostile()).as_str()));

    let value = json(&ls("sub", true));
    assert_eq!(
        value["entries"][0]["submodule_url"],
        format!("https://example.com/{}", hostile())
    );
    assert_eq!(value["entries"][0]["submodule_commit"], COMMIT);
}

#[test]
fn ls_error_paths_fail_with_a_clean_message_and_no_server_text() {
    for path in LS_ERRORS {
        for as_json in [false, true] {
            let ran = ls(&requested(path), as_json);
            let what = format!("ls {path} json={as_json}");
            assert_clean_error(&what, &ran, 1);
            if path != "rejected.txt" {
                assert!(!ran.err.contains("LEAK"), "{what}: {:?}", ran.err);
            }
        }
    }
    let ran = ls("f.txt", false);
    let at = QUOTED_ADDRESS;
    assert_eq!(
        ran.err,
        format!("sego: error: f.txt is a file at {at} (3 lines)\n")
    );
    let ran = ls("bin.dat", false);
    assert_eq!(
        ran.err,
        format!(
            "sego: error: bin.dat is a file at {at} (binary, 2048 bytes)\n"
        )
    );
    for as_json in [false, true] {
        assert_not_served("ls absent", &ls("absent", as_json));
    }
    let ran = ls(&requested("HOSTILE-gone"), false);
    assert_eq!(
        ran.err,
        format!("sego: error: {ESCAPED}-gone not found in {at}\n")
    );
    let ran = ls("broken.txt", false);
    assert_eq!(
        ran.err,
        "sego: error: Sourcegraph failed to answer the directory listing\n"
    );
    let ran = ls("rejected.txt", false);
    assert_eq!(
        ran.err,
        format!(
            "sego: error: Sourcegraph rejected sego's request: \
             {ESCAPED_MESSAGE}\n"
        )
    );
}
