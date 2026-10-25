use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use sego::internal::search::plan::{Filter, Plan, plan};
use sego::internal::search::query::prepare;

/// A plan as one line: `rejected`, `unread`, or the pattern type, the number
/// of searches, and each listed search's filters in the fixture's notation.
/// With `revisions`, a search's `rev:` is moved onto its `repo:` values, as
/// the server does once its checks on the search pass.
fn render(query: &str, revisions: bool) -> String {
    let searches = match plan(query) {
        Plan::Rejected => return "rejected".to_owned(),
        Plan::Unread => return "unread".to_owned(),
        Plan::Read(searches) => searches,
    };
    let count = searches.count();
    if let Some(list) = searches.list(4096) {
        let mut each: Vec<Vec<Filter>> = Vec::new();
        for search in list {
            let search: Vec<Filter> = search.into_iter().cloned().collect();
            if !each.contains(&search) {
                each.push(search);
            }
        }
        let reached =
            searches.reach(Vec::new(), |mut sofar: Vec<Filter>, f| {
                sofar.push(f.clone());
                sofar
            });
        let same = reached.len() == each.len()
            && reached.iter().all(|search| each.contains(search));
        assert!(same, "{query:?}: reach and list differ");
    }
    let listed = searches.list(4096).map(|list| {
        let list: Vec<String> = list
            .into_iter()
            .map(|filters| {
                let mut filters: Vec<Filter> =
                    filters.into_iter().cloned().collect();
                if revisions {
                    revise(&mut filters);
                }
                let filters: Vec<String> =
                    filters.iter().map(notation).collect();
                encoded(&filters.join(" "))
            })
            .collect();
        list.join("  ||  ")
    });
    let ty = searches.pattern_type.name();
    format!("{ty} {count} | {}", listed.unwrap_or_default())
}

fn revise(filters: &mut Vec<Filter>) {
    let Some(rev) = filters.iter().find(|f| f.field == "rev") else {
        return;
    };
    let rev =
        if rev.predicate { "rat=…".to_owned() } else { rev.value.clone() };
    filters.retain(|f| f.field != "rev");
    for f in filters.iter_mut() {
        if f.field == "repo" && !f.negated && !f.predicate {
            f.value = format!("{}@{rev}", f.value);
        }
    }
}

/// `field:value`, with `-` before a negated filter and ` [p]` after a
/// predicate.
fn notation(f: &Filter) -> String {
    let dash = if f.negated { "-" } else { "" };
    let mark = if f.predicate { " [p]" } else { "" };
    format!("{dash}{}:{}{mark}", f.field, f.value)
}

/// A revision the server encodes, from `rev:at.time(…)`, written `rat=…`.
fn encoded(filters: &str) -> String {
    let mut out = String::new();
    for (i, part) in filters.split("@rat=").enumerate() {
        if i > 0 {
            out.push_str("@rat=…");
        }
        let part = if i > 0 {
            &part[part.find(' ').unwrap_or(part.len())..]
        } else {
            part
        };
        out.push_str(part);
    }
    out
}

/// The server's plan as `render` writes sego's, and whether the server's
/// checks passed, so that it moved `rev:` filters.
fn expected(row: &Value) -> (String, bool) {
    let text = |at: usize| row[at].as_str().unwrap_or_default().to_owned();
    if row[1] == "rejected" || row[1] == "unread" {
        return (text(1), true);
    }
    let listed = row[3].as_array().map(|list| {
        let list = list.iter().map(|search| {
            let filters = search.as_array().into_iter().flatten();
            let filters: Vec<&str> =
                filters.filter_map(Value::as_str).collect();
            encoded(&filters.join(" "))
        });
        list.collect::<Vec<_>>().join("  ||  ")
    });
    let line =
        format!("{} {} | {}", text(1), row[2], listed.unwrap_or_default());
    (line, row[4] != "rejected by checks")
}

/// The fixture's rows whose plan sego's differs from, and the number of
/// rows compared.
fn disagreements(path: &Path) -> (usize, Vec<String>) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut found = Vec::new();
    let mut rows = 0;
    for line in text.lines() {
        let row: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        rows += 1;
        let query = row[0].as_str().unwrap_or_default();
        let (theirs, revised) = expected(&row);
        let mine = render(query, revised);
        if mine != theirs {
            found.push(format!(
                "{query:?}\n  sego:   {mine}\n  server: {theirs}"
            ));
        }
    }
    (rows, found)
}

/// A few rows of `tests/fixtures/plan/examples.jsonl` for each kind of
/// rule in rebuild spec §6.1, "The query's plan", written out.
const SPEC_EXAMPLES: &[(&str, &str)] = &[
    // outcomes
    ("repo:x foo or bar file:y", "keyword 1 | repo:x file:y"),
    ("foo or bar repo:x", "keyword 2 |   ||  repo:x"),
    ("x or ", "rejected"),
    // the pattern type
    ("patterntype:regexp foo", "regexp 1 | patterntype:regexp"),
    ("PatternType:regexp foo", "regexp 1 | patterntype:regexp"),
    ("patterntype:Regexp foo", "keyword 1 | patterntype:Regexp"),
    // filters
    ("repo:x foo:bar", "keyword 1 | repo:x"),
    ("FILE:a Lang:go foo", "keyword 1 | file:a lang:go"),
    // filter values
    (r#"repo:"a b" foo"#, "keyword 1 | repo:a b"),
    (r#"repo:'it\'s' foo"#, "keyword 1 | repo:it's"),
    (r#"repo:"a\q" foo"#, "rejected"),
    // empty values
    ("repo:", "keyword 1 | repo:"),
    ("repo: file:", "keyword 1 | file:"),
    // and, or
    ("repo:x a OR b", "keyword 1 | repo:x"),
    ("repo:x a Or(b)", "keyword 2 | repo:x  ||  "),
    ("repo:x a\tor\tb", "keyword 1 | repo:x"),
    // not
    ("not repo:x foo", "keyword 1 | -repo:x"),
    ("not -repo:x foo", "rejected"),
    ("not (foo)", "rejected"),
    // patterns
    (r#"patterntype:standard "a b""#, "standard 1 | patterntype:standard"),
    ("patterntype:regexp 'a b'", "regexp 1 | patterntype:regexp"),
    ("patterntype:literal /a b/", "literal 1 | patterntype:literal"),
    // a ( in each mode
    ("repo:x (a b) or c", "keyword 2 | repo:x  ||  "),
    (
        "repo:x patterntype:literal (a b) or c",
        "literal 1 | repo:x patterntype:literal",
    ),
    (
        "repo:x patterntype:literal (a or b) or c",
        "literal 2 | repo:x patterntype:literal  ||  ",
    ),
    // )or
    ("x (a)or b) c", "rejected"),
    ("repo:y (repo:x a)or b", "keyword 1 | repo:y repo:x"),
    // a stray )
    ("foo)", "rejected"),
    ("x )", "rejected"),
    (r#"patterntype:regexp "(" x)"#, "rejected"),
    // balanced runs
    (
        "repo:x patterntype:literal (a or\tb) or c",
        "literal 1 | repo:x patterntype:literal",
    ),
    (
        "repo:x patterntype:literal (a xfile:x) or c",
        "literal 1 | repo:x patterntype:literal",
    ),
    (
        "repo:x patterntype:literal (a file:) or c",
        "literal 2 | repo:x patterntype:literal  ||  ",
    ),
    // the order of filters
    ("(file:b bar) file:a foo", "keyword 1 | file:a file:b"),
    ("foo file:a (bar file:b)", "keyword 1 | file:a file:b"),
    ("(file:b) foo file:a", "keyword 1 | file:b file:a"),
    // combining
    (
        "repo:x file:y and (file:z foo or bar)",
        "keyword 2 | repo:x file:y file:z  ||  repo:x file:y",
    ),
    ("repo:x file:y (file:z foo or bar)", "keyword 1 | repo:x file:y file:z"),
    ("repo:x and (file:z foo or bar)", "keyword 1 | repo:x file:z"),
    // parentheses as text
    ("repo:x (foo or bar", "keyword 1 | repo:x"),
    ("repo:x (foo or bar) or baz (", "keyword 1 | repo:x"),
    ("x or or y", "rejected"),
    // shared filters
    ("repo:x foo or bar", "keyword 1 | repo:x"),
    (" foo or bar repo:x", "keyword 1 | repo:x"),
    ("(foo or bar repo:x)", "keyword 1 | repo:x"),
    // modes that reject
    (
        "patterntype:literal foo (content:bar baz)",
        "keyword 1 | patterntype:literal",
    ),
    (
        "repo:x patterntype:literal (a or b)c or d",
        "keyword 2 | repo:x patterntype:literal  ||  ",
    ),
    ("patterntype:standard /x (/ (a or b)", "rejected"),
    // field names and content:
    (
        "r:x f:y path:z l:go language:rust foo",
        "keyword 1 | repo:x file:y file:z lang:go lang:rust",
    ),
    (
        "type:commit since:2020 until:2021 m:a msg:b foo",
        "keyword 1 | type:commit after:2020 before:2021 message:a message:b",
    ),
    ("f:has.content(x) foo", "keyword 1 | file:has.content(x) [p]"),
    // sequences by mode
    (
        r#"patterntype:regexp (x or repo:y z) w "a (b""#,
        "regexp 1 | patterntype:regexp",
    ),
    (
        r#"patterntype:regexp repo:q (x or repo:y z) w "a (b""#,
        "regexp 1 | patterntype:regexp repo:q",
    ),
    (
        r#"a (file:y b) or c "q (z" patterntype:regexp"#,
        "regexp 3 |   ||  file:y  ||  patterntype:regexp",
    ),
    // searches
    (
        "(repo:a or repo:b) (file:x or file:y) foo",
        "keyword 4 | repo:a file:x  ||  repo:b file:x  ||  repo:a file:y  ||  repo:b file:y",
    ),
    ("repo:x (a or file:y b) c", "keyword 2 | repo:x  ||  repo:x file:y"),
    (r#"(repo:x or repo:y) ("a" or "b")"#, "keyword 2 | repo:x  ||  repo:y"),
    // revisions
    ("repo:x rev:v1 foo", "keyword 1 | repo:x@v1"),
    ("repo:x repo:y rev:v1 foo", "keyword 1 | repo:x@v1 repo:y@v1"),
    (
        "repo:x -repo:y repo:has.file(z) rev:v1 foo",
        "keyword 1 | repo:x@v1 -repo:y repo:has.file(z) [p]",
    ),
    // values the checks pass
    ("count:5 foo or count:6 bar", "keyword 2 | count:5  ||  count:6"),
    ("type:commit author:bob foo", "keyword 1 | type:commit author:bob"),
    ("index:False foo", "keyword 1 | index:False"),
];

#[test]
fn plans_the_spec_examples() {
    for &(query, expected) in SPEC_EXAMPLES {
        assert_eq!(render(query, true), expected, "{query:?}");
    }
}

#[test]
fn an_empty_query_plans_no_searches() {
    for query in ["", "  \t"] {
        let Plan::Read(searches) = plan(query) else { panic!("{query:?}") };
        assert_eq!(searches.count(), 0);
        assert_eq!(searches.list(1), Some(Vec::new()));
        assert_eq!(searches.reach(0, |n, _| n + 1), Vec::<i32>::new());
    }
}

fn pairs(n: usize) -> String {
    let groups: Vec<String> =
        (0..n).map(|i| format!("(repo:a{i} or repo:b{i})")).collect();
    format!("{} foo", groups.join(" "))
}

#[test]
fn searches_are_counted_and_listed_up_to_a_limit() {
    let Plan::Read(searches) = plan(&pairs(6)) else { panic!() };
    assert_eq!(searches.count(), 64);
    assert_eq!(searches.list(64).map(|list| list.len()), Some(64));
    assert_eq!(searches.list(63), None);
    assert_eq!(plan(&pairs(5)).each().map(|each| each.len()), Some(32));
    assert!(plan(&pairs(6)).each().is_none());
}

#[test]
fn reach_covers_every_search_however_many() {
    let Plan::Read(searches) = plan(&pairs(70)) else { panic!() };
    assert_eq!(searches.count(), u64::MAX);
    assert_eq!(searches.list(4096), None);
    let ends =
        searches.reach(0, |n, f| n + usize::from(f.value.starts_with('b')));
    assert_eq!(ends.len(), 71);
}

/// The server's plans for 120,000 nested groups, in keyword mode and as one
/// balanced run in literal mode, read on a small stack. These two are built
/// here rather than stored in the fixture.
#[test]
fn groups_nest_at_any_depth() {
    let nested = |depth, inner| {
        format!("{}{inner}{}", "(".repeat(depth), ")".repeat(depth))
    };
    let rows = [
        (nested(64, "repo:x foo"), "keyword 1 | repo:x"),
        (nested(65, "repo:x foo"), "keyword 1 | repo:x"),
        (nested(120_000, "repo:x foo"), "keyword 1 | repo:x"),
        (nested(120_000, "foo"), "keyword 1 | "),
        (
            format!("patterntype:literal {}", nested(120_000, "foo")),
            "literal 1 | patterntype:literal",
        ),
    ];
    let run = move || {
        for (query, expected) in rows {
            assert_eq!(render(&query, true), expected);
        }
    };
    let thread = std::thread::Builder::new().stack_size(256 << 10).spawn(run);
    let joined = thread.map(|t| t.join().is_ok());
    assert!(joined.is_ok_and(|ok| ok));
}

/// `)or` keeps a closed group's or-expression open, so a plan can nest far
/// deeper than its groups; sego reads it at any nesting, on a small stack.
#[test]
fn or_expressions_nest_without_limit() {
    let chain = |n| format!("{}y", "(a)or x ".repeat(n));
    let mebibyte = chain((1 << 20) / 8);
    let run = move || {
        let within = format!("{}{}", "(".repeat(63), chain(20_000));
        for query in [chain(601), chain(1_000), mebibyte, within] {
            let Plan::Read(searches) = plan(&query) else { panic!() };
            assert_eq!(searches.list(1).map(|list| list.len()), Some(1));
        }
    };
    let thread = std::thread::Builder::new().stack_size(256 << 10).spawn(run);
    let joined = thread.map(|t| t.join().is_ok());
    assert!(joined.is_ok_and(|ok| ok));
}

/// Plans hundreds of levels deep are planned, listed and walked on a small
/// stack: none of it recurses.
#[test]
fn deep_plans_need_little_stack() {
    let queries = [
        format!("{}y", "(a)or x ".repeat(600)),
        format!("{}y", "(repo:a a)or x and file:b y ".repeat(600)),
        format!("{}y", "(a)or x (b) ".repeat(599)),
        format!(
            "{}{}y{}",
            "(".repeat(63),
            "(a)or x ".repeat(537),
            ")".repeat(63)
        ),
        format!("patterntype:regexp {}y", "(a)or x (b) ".repeat(599)),
    ];
    let run = move || {
        for query in &queries {
            let Plan::Read(searches) = plan(query) else { panic!("{query}") };
            searches.list(4096);
            searches.reach(0, |n, _| n + 1);
        }
    };
    let thread = std::thread::Builder::new().stack_size(256 << 10).spawn(run);
    assert!(
        thread
            .and_then(|t| t
                .join()
                .map_err(|_| std::io::ErrorKind::Other.into()))
            .is_ok()
    );
}

/// Shapes that combine the most items for their length, at 2,048 bytes,
/// with the server's plans.
#[test]
fn plans_the_costliest_queries_of_2_048_bytes() {
    let fill = |head: &str, unit: &str| {
        let mut query = format!("{head}{}", unit.repeat(2048 / unit.len()));
        query.truncate(2048);
        query
    };
    let shapes = [
        ("", "(a b ", "keyword 1 | "),
        ("", "a or (", "keyword 1 | "),
        ("", "x and (a ", "keyword 1 | "),
        ("", "(", "keyword 1 | "),
        ("", "(a)or x ", "keyword 1 | "),
        ("", "a (b or ", "rejected"),
        ("", "(a (b ", "keyword 1 | "),
        ("patterntype:regexp ", "a(", "regexp 1 | patterntype:regexp"),
    ];
    for (head, unit, expected) in shapes {
        let query = fill(head, unit);
        assert_eq!(render(&query, true), expected, "{head}{unit:?}");
    }
}

/// The least time of three that `prepare` takes for a query of `bytes`:
/// `head`, then repeats of `unit`, then `tail` repeated as often.
fn preparing(head: &str, unit: &str, tail: &str, bytes: usize) -> Duration {
    let units = bytes / (unit.len() + tail.len());
    let query = format!("{head}{}{}", unit.repeat(units), tail.repeat(units));
    let time = || {
        let started = Instant::now();
        prepare(&query);
        started.elapsed()
    };
    (0..3).map(|_| time()).min().unwrap_or_default()
}

/// 64 KiB of each shape takes at most a few times sixteen times what 4 KiB
/// does: time that grows with the square of the length would take some 256
/// times as long. The ratio, not the time, is checked, so a loaded machine
/// doesn't fail it.
#[test]
fn prepares_in_linear_time() {
    let nests = [
        "(f:x a ",
        "(a f:x ",
        "(f:x a b ",
        "(-f:x a ",
        "(not f:x a ",
        "(content:x a ",
    ];
    let mut shapes: Vec<(&str, &str, &str)> = vec![
        ("", "f:a ", ""),
        ("", "\"a ", ""),
        ("", "a(b ", ""),
        ("", "foo or ", ""),
        ("", "repo:x foo or bar ", ""),
        ("", "(repo:a or repo:b) ", ""),
        ("patterntype:standard ", "( a", ""),
        ("", "(a)or x ", ""),
        ("", "(a b ", ""),
        ("", "a or (", ""),
        ("", "x and (a ", ""),
        ("", "(", ""),
    ];
    for unit in nests {
        shapes.push(("", unit, ")"));
    }
    for head in [
        "patterntype:regexp ",
        "patterntype:literal ",
        "patterntype:standard ",
        "patterntype:structural ",
    ] {
        shapes.push((head, "(f:x a ", ")"));
    }
    for (head, unit, tail) in shapes {
        let small = preparing(head, unit, tail, 4 << 10);
        let large = preparing(head, unit, tail, 64 << 10);
        assert!(
            large < small * 48 + Duration::from_millis(20),
            "{head}{unit:?}: {large:?}, against {small:?} for a sixteenth"
        );
    }
}

/// One search can hold a filter for every few bytes; listing it, as
/// `prepare` does for `index:no`, gives each of them once.
#[test]
fn lists_a_mebibyte_of_filters() {
    let query = "f:a ".repeat((1 << 20) / 4);
    let searches = plan(&query).each().map(|each| each[0].len());
    assert_eq!(searches, Some(1 << 18));
}

/// Every row of the fixture of the server's plans agrees, and there is at
/// least one.
#[test]
fn plans_match_the_servers() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plan/examples.jsonl");
    let (rows, found) = disagreements(&path);
    assert!(rows > 0, "{}: no plans to compare", path.display());
    assert!(
        found.is_empty(),
        "{} of {rows} plans differ\n{}",
        found.len(),
        found.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}
