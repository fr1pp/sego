use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;

use serde_json::Value;

use sego::internal::escape::is_unsafe;
use sego::{Endpoint, Transport};

use crate::support::exec::exec_os;
use crate::support::fake::Fake;
use crate::support::fixture::{Fixture, load_all};

mod history;
mod ls;
mod refs;
mod search;
mod show;

const CORPUS: [&str; 8] = [
    "esc\u{1b}[0m",
    "rtl\u{202e}x",
    "zw\u{200b}x",
    "new\nline",
    "cr\r",
    "nul\u{0}x",
    "ls\u{2028}x",
    "del\u{7f}x",
];
/// The server's hostile message as an error line prints it.
const ESCAPED_MESSAGE: &str = "LEAK esc\\u{1b}[0m rtl\\u{202e}x zw\\u{200b}x \
                               new line cr\\u{d} nul\\u{0}x ls\\u{2028}x \
                               del\\u{7f}x";
/// `hostile()` as `one_line` prints it.
const ESCAPED: &str = "esc\\u{1b}[0mrtl\\u{202e}xzw\\u{200b}xnew \
                       linecr\\u{d}nul\\u{0}xls\\u{2028}xdel\\u{7f}x";
const REPO: &str = "github.com/hostile/repo";

struct Ran {
    code: ExitCode,
    out: String,
    err: String,
}

fn fixtures() -> Vec<Fixture> {
    static LOADED: OnceLock<Vec<Fixture>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            load_all(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/hostile"),
            )
        })
        .clone()
}

fn hostile() -> String {
    CORPUS.concat()
}

fn hostile_name() -> String {
    format!("{REPO}{}", hostile())
}

fn sego(args: Vec<OsString>) -> Ran {
    let fake = Fake::fixtures(fixtures());
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(fake.clone()) };
    let ran = exec_os(args, None, OsStr::new("off"), &connect);
    Ran {
        code: ran.code,
        out: String::from_utf8(ran.out).unwrap_or_else(|e| panic!("{e}")),
        err: String::from_utf8(ran.err).unwrap_or_else(|e| panic!("{e}")),
    }
}

/// The line break is the one unsafe character sego writes itself; the
/// line tests check where.
fn assert_safe(what: &str, text: &str) {
    if let Some(c) = text.chars().find(|&c| c != '\n' && is_unsafe(c)) {
        panic!("{what} holds U+{:04X}: {text:?}", u32::from(c));
    }
}

fn assert_clean_error(what: &str, ran: &Ran, code: u8) {
    assert_eq!(ran.code, ExitCode::from(code), "{what}: {}", ran.err);
    assert!(ran.out.is_empty(), "{what}: {:?}", ran.out);
    assert!(ran.err.starts_with("sego: error: "), "{what}: {:?}", ran.err);
    assert!(ran.err.ends_with('\n'), "{what}: {:?}", ran.err);
    assert_eq!(ran.err.matches('\n').count(), 1, "{what}: {:?}", ran.err);
}

fn json(ran: &Ran) -> Value {
    serde_json::from_str(&ran.out)
        .unwrap_or_else(|e| panic!("{e}: {:?}", ran.out))
}

fn requested(path: &str) -> String {
    path.replace("HOSTILE", &hostile())
}

/// The two-line error for a repository Sourcegraph doesn't have.
fn assert_not_served(what: &str, ran: &Ran) {
    assert_eq!(ran.code, ExitCode::from(1), "{what}: {}", ran.err);
    assert!(ran.out.is_empty(), "{what}: {:?}", ran.out);
    assert_eq!(
        ran.err,
        format!(
            "sego: error: {REPO} is not on sourcegraph.com\n  \
             sourcegraph.com serves indexed repositories from github.com, \
             gitlab.com and a few other hosts; a renamed repository appears \
             only under its new name\n"
        ),
        "{what}"
    );
}

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn the_ls_and_refs_fixtures_carry_the_corpus() {
    let fixtures = fixtures();
    let response = |name: &str| {
        let fixture = fixtures
            .iter()
            .find(|f| f.file.ends_with(name))
            .unwrap_or_else(|| panic!("no {name}"));
        fixture.response.clone().unwrap_or(Value::Null)
    };
    let root = response("ls-root.yaml");
    let c0 = &root["data"]["r0"]["c0"];
    assert_eq!(
        root["data"]["r0"]["name"].as_str(),
        Some(hostile_name().as_str())
    );
    let entries = &c0["tree"]["entries"];
    assert_eq!(entries[0]["path"].as_str(), Some(hostile().as_str()));
    assert_eq!(
        entries[1]["path"].as_str(),
        Some(format!("{}-dir", hostile()).as_str())
    );
    assert_eq!(
        entries[2]["submodule"]["url"].as_str(),
        Some(format!("https://example.com/{}", hostile()).as_str())
    );
    assert_eq!(
        entries[3]["submodule"]["commit"].as_str(),
        Some(hostile().as_str())
    );
    let listed = response("ls-path.yaml");
    let entries = &listed["data"]["r0"]["c0"]["tree"]["entries"];
    assert_eq!(
        entries[0]["path"].as_str(),
        Some(format!("{}/{}", hostile(), hostile()).as_str())
    );

    let refs = response("refs-text.yaml");
    let r0 = &refs["data"]["r0"];
    assert_eq!(r0["name"].as_str(), Some(hostile_name().as_str()));
    assert_eq!(
        r0["defaultBranch"]["displayName"].as_str(),
        Some(hostile().as_str())
    );
    assert_eq!(
        r0["branches"]["nodes"][1]["displayName"].as_str(),
        Some(format!("b-{}", hostile()).as_str())
    );
    assert_eq!(
        r0["branches"]["nodes"][1]["target"]["oid"].as_str(),
        Some(hostile().as_str())
    );
    assert_eq!(
        r0["tags"]["nodes"][0]["target"]["commit"]["committer"]["date"]
            .as_str(),
        Some(hostile().as_str())
    );
    assert_eq!(
        r0["tags"]["nodes"][2]["displayName"].as_str(),
        Some(format!("u-{}", hostile()).as_str())
    );
    let matched = fixtures
        .iter()
        .find(|f| f.file.ends_with("refs-match.yaml"))
        .and_then(|f| f.variables.as_ref())
        .and_then(|v| v["query"].as_str().map(str::to_owned));
    assert_eq!(matched, Some(hostile()));
}

#[test]
fn the_hostile_fixture_files_hold_no_raw_unsafe_character() {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostile");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_safe(&path.display().to_string(), &text);
        checked += 1;
    }
    assert!(checked >= 20, "{checked} fixture files");
}
