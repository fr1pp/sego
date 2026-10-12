use sego::internal::output::Answer;
use sego::internal::search::model::{
    Chunk, Group, LineOut, Searched, ServerOut, SymbolRow, Widen,
};
use sego::internal::search::query::Unit;
use sego::internal::search::text::TEXT_UNITS;

const COMMIT: &str = "3fce3b5bb023aaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn base(query: &str) -> Searched {
    Searched {
        query: query.to_owned(),
        pattern_type: "keyword".to_owned(),
        unit: Unit::Matches,
        count: 30,
        at_least: false,
        stopped: false,
        elapsed_ms: 700,
        limit_ms: 60_000,
        repos: 1,
        widen: Vec::new(),
        unindexed: None,
        alert: None,
        unshown: Vec::new(),
        groups: Vec::new(),
        server: ServerOut {
            match_count: 30,
            approximate_count: "30".to_owned(),
            limit_hit: false,
            timedout: Vec::new(),
        },
    }
}

fn widen(filter: &str, repos: u64) -> Widen {
    Widen { filter: filter.to_owned(), repos }
}

fn line(number: u32, text: &str, range: Option<[u32; 2]>) -> LineOut {
    LineOut {
        number,
        text: text.to_owned(),
        matched: range.is_some(),
        ranges: range.into_iter().collect(),
    }
}

fn chunk(lines: Vec<LineOut>, units: u32) -> Chunk {
    Chunk { lines, units }
}

fn symbol(line: Option<u32>, name: &str, kind: &str) -> SymbolRow {
    SymbolRow {
        line,
        name: name.to_owned(),
        kind: kind.to_owned(),
        container: None,
    }
}

fn file(repo: &str, path: &str, chunks: Vec<Chunk>) -> Group {
    file_with(repo, path, chunks, Vec::new())
}

fn file_with(
    repo: &str,
    path: &str,
    chunks: Vec<Chunk>,
    symbols: Vec<SymbolRow>,
) -> Group {
    Group::File {
        repo: repo.to_owned(),
        commit: COMMIT.to_owned(),
        rev: None,
        path: path.to_owned(),
        binary: false,
        chunks,
        symbols,
    }
}

fn path_group(n: usize) -> Group {
    file("r", &format!("p{n}"), Vec::new())
}

fn one_range(n: u32) -> Chunk {
    chunk(vec![line(n * 10, "x", Some([0, 1]))], 1)
}

#[test]
fn the_two_spec_examples() {
    let mut s =
        base("repo:^github\\.com/BurntSushi/ripgrep$ fn main count:30");
    s.groups = vec![file(
        "github.com/BurntSushi/ripgrep",
        "crates/core/main.rs",
        vec![
            chunk(
                vec![
                    line(
                        42,
                        "/// Then, as it was, then again it will be.",
                        None,
                    ),
                    line(43, "fn main() -> ExitCode {", Some([0, 7])),
                    line(44, "    match run(flags::parse()) {", None),
                ],
                1,
            ),
            chunk(
                vec![
                    line(76, "", None),
                    line(
                        77,
                        "fn run(result: crate::flags::ParseResult<HiArgs>) -> anyhow::Result<ExitCode> {",
                        Some([0, 6]),
                    ),
                    line(
                        78,
                        "    use crate::flags::{Mode, SearchMode};",
                        None,
                    ),
                ],
                1,
            ),
        ],
    )];
    let expected = "\
30 matches  1 repo  0.7 s
query  repo:^github\\.com/BurntSushi/ripgrep$ fn main count:30

github.com/BurntSushi/ripgrep@3fce3b5bb023:crates/core/main.rs
  42-/// Then, as it was, then again it will be.
  43:fn main() -> ExitCode {
  44-    match run(flags::parse()) {
--
  76-
  77:fn run(result: crate::flags::ParseResult<HiArgs>) -> anyhow::Result<ExitCode> {
  78-    use crate::flags::{Mode, SearchMode};
";
    assert_eq!(s.text(), expected);

    let mut s = base("file:(^|/)error\\.rs$ lang:rust type:path count:all");
    s.count = 28900;
    s.repos = 8133;
    s.elapsed_ms = 16_200;
    s.unit = Unit::Files;
    s.groups = (0..101).map(path_group).collect();
    s.groups[0] = file(
        "github.com/BurntSushi/ripgrep",
        "crates/core/error.rs",
        Vec::new(),
    );
    s.groups[1] = file(
        "github.com/awslabs/aws-sdk-rust",
        "sdk/s3/src/error.rs",
        Vec::new(),
    );
    let text = s.text();
    let head: Vec<&str> = text.lines().take(6).collect();
    assert_eq!(
        head,
        [
            "100 of 28900 files  8133 repos  16.2 s",
            "query  file:(^|/)error\\.rs$ lang:rust type:path count:all",
            "",
            "github.com/BurntSushi/ripgrep@3fce3b5bb023:crates/core/error.rs",
            "github.com/awslabs/aws-sdk-rust@3fce3b5bb023:sdk/s3/src/error.rs",
            "r@3fce3b5bb023:p2",
        ]
    );
}

fn first_line(s: &Searched) -> String {
    s.text().lines().next().unwrap_or_default().to_owned()
}

#[test]
fn line_one_for_each_row_of_the_spec_table() {
    let mut s = base("q");
    s.count = 3;
    s.elapsed_ms = 1200;
    s.groups = vec![path_group(0)];
    assert_eq!(first_line(&s), "3 matches  1 repo  1.2 s");

    s.count = 30;
    s.at_least = true;
    s.repos = 4;
    s.elapsed_ms = 2000;
    assert_eq!(first_line(&s), "30+ matches  4 repos  2.0 s");

    s.count = 2;
    s.repos = 1;
    s.stopped = true;
    s.elapsed_ms = 21_300;
    assert_eq!(first_line(&s), "2+ matches  1 repo  stopped at 21.3 s");

    s.count = 0;
    s.at_least = false;
    s.stopped = false;
    s.repos = 0;
    s.elapsed_ms = 700;
    s.groups.clear();
    assert_eq!(first_line(&s), "0 matches  0.7 s");
}

#[test]
fn count_forms() {
    let mut s = base("q");
    s.count = 1;
    s.groups = vec![path_group(0)];
    assert_eq!(first_line(&s), "1 match  1 repo  0.7 s");

    s.at_least = true;
    assert_eq!(first_line(&s), "1+ matches  1 repo  0.7 s");

    s.at_least = false;
    s.unit = Unit::Symbols;
    assert_eq!(first_line(&s), "1 symbol  1 repo  0.7 s");
    s.unit = Unit::Files;
    assert_eq!(first_line(&s), "1 file  1 repo  0.7 s");
    s.unit = Unit::Repos;
    assert_eq!(first_line(&s), "1 repo  0.7 s");
    s.count = 3;
    s.repos = 3;
    assert_eq!(first_line(&s), "3 repos  0.7 s");
    s.count = 1;
    s.repos = 1;
    s.unit = Unit::Matches;

    s.count = 100;
    s.at_least = true;
    assert_eq!(first_line(&s), "100+ matches  1 repo  0.7 s");
}

#[test]
fn text_fewer_than_held_uses_the_of_form() {
    let mut s = base("q");
    s.count = 836;
    s.groups = (0..101).map(path_group).collect();
    assert_eq!(first_line(&s), "100 of 836 matches  1 repo  0.7 s");

    s.at_least = true;
    s.stopped = true;
    s.elapsed_ms = 21_300;
    assert_eq!(
        first_line(&s),
        "100 of 836+ matches  1 repo  stopped at 21.3 s"
    );
}

#[test]
fn limit_at_and_past_the_boundary() {
    assert_eq!(TEXT_UNITS, 100);
    let mut s = base("q");
    s.count = 100;
    s.groups = (0..100).map(path_group).collect();
    let text = s.text();
    assert!(text.starts_with("100 matches  "), "{text}");
    assert_eq!(text.lines().count(), 3 + 100);

    s.count = 101;
    s.groups = (1..=101).map(|n| file("r", "p", vec![one_range(n)])).collect();
    assert_eq!(s.units(), 101);
    let text = s.text();
    assert!(text.starts_with("100 of 101 matches  "), "{text}");
    assert_eq!(text.matches("r@3fce3b5bb023:p\n").count(), 100);

    let wide = chunk((1..=5).map(|n| line(n, "x", Some([0, 1]))).collect(), 5);
    let mut groups: Vec<Group> = (0..99).map(path_group).collect();
    groups.push(file("r", "wide", vec![wide]));
    groups.push(path_group(100));
    s.count = 105;
    s.groups = groups;
    assert_eq!(s.units(), 105);
    let text = s.text();
    assert!(text.starts_with("104 of 105 matches  "), "{text}");
    assert!(text.ends_with("   5:x\n"), "{text}");
    assert!(!text.contains(":p100"));
}

#[test]
fn widen_on_a_zero() {
    let mut s = base("lang:go Foo");
    s.count = 0;
    s.repos = 0;
    s.widen = vec![widen("archived:yes", 68_912), widen("fork:yes", 1)];
    assert_eq!(
        s.text(),
        "0 matches  0.7 s\n\
         query  lang:go Foo\n\
         widen  archived:yes (68912 repos)  fork:yes (1 repo)\n"
    );

    s.widen.clear();
    assert_eq!(s.text().lines().count(), 2);
}

#[test]
fn widen_waits_for_a_real_zero() {
    let mut s = base("lang:go Foo");
    s.count = 0;
    s.groups = vec![path_group(0)];
    s.widen = vec![widen("fork:yes", 4)];
    assert!(!s.text().contains("widen"));
}

#[test]
fn widen_waits_for_an_answer_that_is_not_at_least() {
    let mut s = base("lang:go Foo");
    s.count = 0;
    s.at_least = true;
    s.widen = vec![widen("fork:yes", 4)];
    assert_eq!(s.text(), "0+ matches  1 repo  0.7 s\nquery  lang:go Foo\n");
}

#[test]
fn widen_is_for_a_zero_only() {
    let mut s = base("lang:go Foo");
    s.groups = vec![path_group(0)];
    s.widen = vec![widen("fork:yes", 4)];
    assert!(!s.text().contains("widen"));
}

#[test]
fn symbols_merge_with_chunks() {
    let mut s = base("q");
    s.groups = vec![file_with(
        "r",
        "a.rs",
        vec![
            chunk(
                vec![line(9, "a", None), line(10, "fn one() {", Some([3, 6]))],
                1,
            ),
            chunk(vec![line(11, "b", Some([0, 1]))], 1),
            chunk(vec![line(30, "c", Some([0, 1]))], 1),
        ],
        vec![
            symbol(Some(10), "one", "function"),
            symbol(Some(20), "Mid", "struct"),
            symbol(None, "ghost", "struct"),
            symbol(Some(40), "Tail", "struct"),
        ],
    )];
    let expected = "\
30 matches  1 repo  0.7 s
query  q

r@3fce3b5bb023:a.rs
   9-a
  10:fn one() {
  10:function  one
  11:b
  20:struct  Mid
  30:c
  40:struct  Tail
";
    assert_eq!(s.text(), expected);
}

fn commit_group(
    lines: Vec<String>,
    highlights: Vec<[u32; 3]>,
    diff: bool,
) -> Group {
    Group::Commit {
        repo: "r".to_owned(),
        commit: COMMIT.to_owned(),
        date: "2026-07-23".to_owned(),
        author: "Ann".to_owned(),
        subject: "Fix it".to_owned(),
        lines,
        highlights,
        diff,
    }
}

fn numbered(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("m{i}")).collect()
}

#[test]
fn commit_results() {
    let mut s = base("type:commit q");
    s.groups = vec![commit_group(
        vec!["Fix it".to_owned(), String::new(), "more".to_owned()],
        vec![[0, 4, 6]],
        false,
    )];
    let expected = "\
30 matches  1 repo  0.7 s
query  type:commit q

r@3fce3b5bb023  2026-07-23  Ann  Fix it
    Fix it
    
";
    assert_eq!(s.text(), expected);

    let mut s = base("q");
    s.groups = vec![Group::Repo { repo: "github.com/a b/c".to_owned() }];
    assert!(s.text().ends_with("\n\ngithub.com/a b/c\n"));
}

#[test]
fn a_commit_prints_the_lines_around_its_highlights() {
    let mut s = base("type:commit q");
    s.groups =
        vec![commit_group(numbered(12), vec![[0, 0, 1], [6, 0, 1]], false)];
    let text = s.text();
    let body: Vec<&str> = text.lines().skip(4).map(str::trim_start).collect();
    assert_eq!(body, ["m1", "m2", "--", "m6", "m7", "m8"]);
    assert_eq!(s.units(), 2);
}

#[test]
fn contiguous_commit_context_has_no_separator() {
    let mut s = base("type:commit q");
    s.groups =
        vec![commit_group(numbered(12), vec![[3, 0, 1], [5, 0, 1]], false)];
    let text = s.text();
    let body: Vec<&str> = text.lines().skip(4).map(str::trim_start).collect();
    assert_eq!(body, ["m3", "m4", "m5", "m6", "m7"]);
}

#[test]
fn a_diff_prints_all_its_lines() {
    let mut s = base("type:diff q");
    s.groups = vec![commit_group(numbered(12), vec![[0, 0, 1]], true)];
    let text = s.text();
    let body: Vec<&str> = text.lines().skip(4).map(str::trim_start).collect();
    assert_eq!(body, numbered(12));
}

#[test]
fn cut_centres_on_the_match() {
    let mut text = "a".repeat(200_000);
    text.replace_range(150_000..150_005, "NEEDL");
    let mut s = base("q");
    s.groups = vec![file(
        "r",
        "long.txt",
        vec![chunk(vec![line(1, &text, Some([150_000, 150_005]))], 1)],
    )];
    let out = s.text();
    let row = out.lines().last().unwrap_or_default();
    assert!(row.starts_with("   1:…"), "{row}");
    assert!(row.contains("NEEDL"));
    assert!(row.len() < 700, "{}", row.len());
}

#[test]
fn directories_and_the_root() {
    let mut s = base("q");
    s.groups =
        vec![file("r", "", Vec::new()), file("r", "src/dir/", Vec::new())];
    assert!(
        s.text().ends_with("\nr@3fce3b5bb023:./\nr@3fce3b5bb023:src/dir/\n")
    );
}

#[test]
fn binary_files() {
    let mut s = base("q");
    s.groups = vec![Group::File {
        repo: "r".to_owned(),
        commit: COMMIT.to_owned(),
        rev: None,
        path: "a.bin".to_owned(),
        binary: true,
        chunks: Vec::new(),
        symbols: Vec::new(),
    }];
    assert!(
        s.text()
            .ends_with("r@3fce3b5bb023:a.bin\nbinary file; lines not shown\n")
    );
}

#[test]
fn hostile_strings_print_escaped() {
    let mut s = base("a\nb");
    s.groups = vec![
        file("r\u{1b}x", "p\u{1b}y", Vec::new()),
        Group::Commit {
            repo: "r".to_owned(),
            commit: COMMIT.to_owned(),
            date: "d".to_owned(),
            author: "a".to_owned(),
            subject: "s\u{1b}z".to_owned(),
            lines: Vec::new(),
            highlights: Vec::new(),
            diff: false,
        },
    ];
    let text = s.text();
    assert!(!text.contains('\u{1b}'), "{text:?}");
    assert!(text.contains("query  a\\u{a}b\n"));
    assert!(text.contains("\\u{1b}"));
    assert!(text.contains("s\\u{1b}z"));

    let mut s = base("q");
    s.count = 0;
    s.widen = vec![widen("archived:\u{1b}[31m", 2)];
    let text = s.text();
    assert!(!text.contains('\u{1b}'), "{text:?}");
    assert!(text.contains("widen  archived:\\u{1b}[31m (2 repos)\n"));
}

#[test]
fn elapsed_rounding() {
    for (ms, shown) in [(16_250, "16.3 s"), (49, "0.0 s"), (60_009, "60.0 s")]
    {
        let mut s = base("q");
        s.elapsed_ms = ms;
        let first = s.text().lines().next().unwrap_or_default().to_owned();
        assert!(first.ends_with(&format!("  {shown}")), "{first}");
    }
}

#[test]
fn a_binary_file_with_symbols_prints_what_units_counts() {
    let mut s = base("q");
    s.count = 3;
    s.groups = vec![Group::File {
        repo: "r".to_owned(),
        commit: COMMIT.to_owned(),
        rev: None,
        path: "a.bin".to_owned(),
        binary: true,
        chunks: Vec::new(),
        symbols: vec![
            symbol(Some(3), "a", "function"),
            symbol(None, "ghost", "function"),
            symbol(Some(5), "b", "function"),
        ],
    }];
    assert_eq!(s.units(), 3);
    assert_eq!(
        s.text(),
        "3 matches  1 repo  0.7 s\n\
         query  q\n\
         \n\
         r@3fce3b5bb023:a.bin\n\
         binary file; lines not shown\n\
         \x20  3:function  a\n\
         \x20  5:function  b\n"
    );
}

#[test]
fn symbols_without_a_line_leave_a_path_match() {
    let mut s = base("q");
    s.count = 1;
    s.groups = vec![file_with(
        "r",
        "a.rs",
        Vec::new(),
        vec![symbol(None, "ghost", "struct")],
    )];
    assert_eq!(s.units(), 1);
    assert_eq!(
        s.text(),
        "1 match  1 repo  0.7 s\n\
         query  q\n\
         \n\
         r@3fce3b5bb023:a.rs\n"
    );
}

#[test]
fn symbols_sit_between_the_chunk_lines_around_them() {
    let mut s = base("q");
    s.count = 6;
    s.groups = vec![file_with(
        "r",
        "a.rs",
        vec![
            chunk(vec![line(20, "a", Some([0, 1])), line(21, "b", None)], 1),
            chunk(
                vec![
                    line(23, "c", None),
                    line(24, "d", None),
                    line(25, "e", Some([0, 1])),
                    line(26, "f", None),
                ],
                1,
            ),
            chunk(vec![line(30, "g", Some([0, 1]))], 1),
            chunk(vec![line(40, "h", Some([0, 1]))], 1),
        ],
        vec![
            symbol(Some(25), "inner", "function"),
            symbol(Some(26), "edge", "function"),
        ],
    )];
    let expected = "\
6 matches  1 repo  0.7 s
query  q

r@3fce3b5bb023:a.rs
  20:a
  21-b
--
  23-c
  24-d
  25:e
  25:function  inner
  26-f
  26:function  edge
  30:g
--
  40:h
";
    assert_eq!(s.text(), expected);
}

#[test]
fn the_focus_is_the_leftmost_range() {
    let mut text = "a".repeat(600);
    text.replace_range(10..15, "LEFT_");
    text.replace_range(500..505, "RIGHT");
    let mut s = base("q");
    s.groups = vec![file(
        "r",
        "wide.txt",
        vec![chunk(vec![line(1, &text, Some([500, 505]))], 1)],
    )];
    if let Group::File { chunks, .. } = &mut s.groups[0] {
        chunks[0].lines[0].ranges.push([10, 15]);
    }
    let out = s.text();
    let row = out.lines().last().unwrap_or_default();
    assert!(row.starts_with("   1:aaaaaaaaaaLEFT_"), "{row}");
}
