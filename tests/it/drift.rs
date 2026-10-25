use std::ffi::OsStr;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::Endpoint;
use sego::internal::search::query::prepare;
use sego::internal::search::response::{Listing, UnindexedFile};
use sego::internal::upstream::{
    DEADLINE, Envelope, Ureq, Var, exchange, top_level,
};

const PAUSE: Duration = Duration::from_secs(1);

type Rule = fn(&Envelope) -> Result<(), String>;

const SQLITE_FILE: &str = "c_src/sqlite3.c";
const X509_FILE: &str = "src/crypto/x509/x509_test.go";
const TRIGRAM_LIMIT: u64 = 1_048_576;

const LISTED: &str = "results { limitHit elapsedMilliseconds timedout { name } \
    alert { title } results { ... on FileMatch { repository { name } \
    file { path byteSize commit { oid } } } } }";
const COUNTED: &str = "results { matchCount limitHit \
    dynamicFilters { value count kind } }";

fn unindexed_of(query: &str) -> String {
    prepare(query)
        .unindexed
        .unwrap_or_else(|| panic!("`{query}` has no unindexed side query"))
}

fn listed(
    envelope: &Envelope,
    alias: &str,
) -> Result<Vec<UnindexedFile>, String> {
    let value = envelope
        .get(&[alias, "results"])
        .ok_or_else(|| format!("{alias} is missing or errored"))?;
    let listing: Listing = serde_json::from_value(value.clone())
        .map_err(|e| format!("{alias} has an unexpected shape: {e}"))?;
    Ok(listing.files())
}

fn counted(
    envelope: &Envelope,
    alias: &str,
) -> Result<(u64, bool, Value), String> {
    let value = envelope
        .get(&[alias, "results"])
        .ok_or_else(|| format!("{alias} is missing or errored"))?;
    let count = value["matchCount"].as_u64();
    let hit = value["limitHit"].as_bool();
    match (count, hit) {
        (Some(count), Some(hit)) => {
            Ok((count, hit, value["dynamicFilters"].clone()))
        }
        _ => Err(format!("{alias} lacks matchCount or limitHit")),
    }
}

fn file_bytes(files: &[UnindexedFile], path: &str) -> Result<u64, String> {
    files.iter().find(|f| f.path == path).map(|f| f.bytes).ok_or_else(|| {
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        format!("{path} is not listed (listed: {paths:?})")
    })
}

fn placeholder_listing(envelope: &Envelope) -> Result<(), String> {
    let bytes = file_bytes(&listed(envelope, "q0")?, SQLITE_FILE)?;
    if bytes > TRIGRAM_LIMIT {
        Ok(())
    } else {
        Err(format!(
            "{SQLITE_FILE} is {bytes} bytes, not over {TRIGRAM_LIMIT}"
        ))
    }
}

fn trigram_reason(envelope: &Envelope) -> Result<(), String> {
    let bytes = file_bytes(&listed(envelope, "q1")?, X509_FILE)?;
    if bytes <= TRIGRAM_LIMIT {
        Ok(())
    } else {
        Err(format!("{X509_FILE} is {bytes} bytes, over {TRIGRAM_LIMIT}"))
    }
}

fn cap_and_plus(envelope: &Envelope) -> Result<(), String> {
    let (count, hit, _) = counted(envelope, "q2")?;
    if count == 30 && hit {
        Ok(())
    } else {
        Err(format!("matchCount {count}, limitHit {hit}; want 30 and true"))
    }
}

fn complete_answer(envelope: &Envelope) -> Result<(), String> {
    let (count, hit, _) = counted(envelope, "q3")?;
    if (1..30).contains(&count) && !hit {
        Ok(())
    } else {
        Err(format!(
            "matchCount {count}, limitHit {hit}; want 1 to 29 and false"
        ))
    }
}

fn utility_counts(envelope: &Envelope) -> Result<(), String> {
    let (_, _, filters) = counted(envelope, "q4")?;
    let found = filters.as_array().into_iter().flatten().find(|f| {
        f["value"] == "archived:yes"
            && f["kind"]
                .as_str()
                .is_some_and(|k| k.eq_ignore_ascii_case("utility"))
    });
    match found.and_then(|f| f["count"].as_u64()) {
        Some(count) if count >= 1 => Ok(()),
        _ => Err(format!(
            "no utility filter archived:yes with count >= 1 ({filters})"
        )),
    }
}

#[test]
#[ignore = "sends requests to sourcegraph.com"]
fn drift_rules() {
    let queries = [
        unindexed_of(r"repo:^github\.com/elixir-sqlite/exqlite$ x"),
        unindexed_of(r"repo:^github\.com/golang/go$ file:^src/crypto/x509/ x"),
        r"repo:^github\.com/gorilla/mux$ func".to_owned(),
        r"repo:^github\.com/gorilla/mux$ catchallhandler".to_owned(),
        r"repo:^github\.com/gorilla/ zqxjkvbnmwqtyplkh".to_owned(),
    ];
    let selections = [LISTED, LISTED, COUNTED, COUNTED, COUNTED];
    let names = ["q0", "q1", "q2", "q3", "q4"];
    let vars = names
        .iter()
        .zip(&queries)
        .map(|(name, query)| Var {
            name,
            ty: "String!",
            value: Value::String(query.clone()),
        })
        .collect();
    let selection = names
        .iter()
        .zip(selections)
        .map(|(name, fields)| {
            format!(
                "{name}: search(query: ${name}, version: V3, \
                 patternType: keyword) {{ {fields} }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let request = top_level("DriftRules", vars, &selection);
    let endpoint =
        Endpoint::from_env(None::<&OsStr>).unwrap_or_else(|e| panic!("{e}"));
    let upstream = Ureq::new(endpoint);
    let envelope = exchange(&upstream, &request, Instant::now() + DEADLINE)
        .unwrap_or_else(|e| panic!("{e}"));

    let rules: [(&str, Rule); 5] = [
        ("placeholder listing", placeholder_listing),
        ("trigram reason", trigram_reason),
        ("cap and plus", cap_and_plus),
        ("complete answer", complete_answer),
        ("utility counts", utility_counts),
    ];
    let failures: Vec<String> = rules
        .iter()
        .filter_map(|(name, check)| {
            check(&envelope).err().map(|why| format!("{name}: {why}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} rules no longer hold:\n{}",
        failures.len(),
        rules.len(),
        failures.join("\n")
    );
}

/// Queries from `tests/fixtures/plan/examples.jsonl`, across the plan's
/// rules, in the three modes sourcegraph.com can be asked to read a query
/// in. Keyword mode can't be checked this way: `parseSearchQuery` reads a
/// query sent as keyword as literal.
const PLANNED: [&str; 13] = [
    "repo:x patterntype:literal (a or b) or c",
    "repo:x patterntype:literal (a -file:x) or c",
    "repo:x patterntype:literal (\"a or b\") or baz",
    "repo:x patterntype:regexp (\"b or c\") or d",
    "a (file:y b) or c \"q (z\" patterntype:regexp",
    "patterntype:regexp (\"x(\" b) (c or repo:d e) \"q (z\"",
    "repo:x patterntype:regexp (foo and bar) or baz",
    "patterntype:structural foo(...) or (lang:go bar(...))",
    r"repo:x patterntype:structural (file:^a\.go$ foo(...) or file:^b\.go$ bar(...))",
    r"foo(...) or bar(...) file:^a\.go$ patterntype:structural",
    r"repo:x not file:y file:^a\.go$ patterntype:structural foo(...) or bar(...)",
    r"repo:x file:has.content(x) file:\.go$ patterntype:structural foo(...)",
    r"repo:x l:go file:^Makefile$ patterntype:structural foo(...)",
];

/// The searches a parse tree from `parseSearchQuery` implies, each filter
/// written `-field:value`, with ` [p]` after a predicate: an `or` with a
/// filter inside splits, and an `and` multiplies its items' searches.
fn tree_searches(node: &Value) -> Vec<Vec<String>> {
    fn holds_filter(node: &Value) -> bool {
        match node {
            Value::Array(items) => items.iter().any(holds_filter),
            Value::Object(map) if map.contains_key("field") => true,
            Value::Object(map) => map.values().any(holds_filter),
            _ => false,
        }
    }
    let and = |items: &[Value]| {
        let mut searches = vec![Vec::new()];
        for item in items {
            let mut next = Vec::new();
            for own in tree_searches(item) {
                for sofar in &searches {
                    next.push([sofar.clone(), own.clone()].concat());
                }
            }
            searches = next;
        }
        searches
    };
    match node {
        Value::Array(items) => and(items),
        Value::Object(map) => {
            if let Some(Value::Array(items)) = map.get("and") {
                and(items)
            } else if let Some(Value::Array(items)) = map.get("or") {
                if holds_filter(node) {
                    items.iter().flat_map(tree_searches).collect()
                } else {
                    vec![Vec::new()]
                }
            } else if let Some(field) =
                map.get("field").and_then(Value::as_str)
            {
                let dash = if map["negated"] == true { "-" } else { "" };
                let value = map["value"].as_str().unwrap_or_default();
                let labels = map["labels"].as_array().into_iter().flatten();
                let predicate = labels.into_iter().any(|l| l == "IsPredicate");
                let mark = if predicate { " [p]" } else { "" };
                vec![vec![format!("{dash}{field}:{value}{mark}")]]
            } else {
                vec![Vec::new()]
            }
        }
        _ => vec![Vec::new()],
    }
}

/// sourcegraph.com's parse tree for each query of `PLANNED`, asked for one
/// at a time in the query's mode, implies the searches and filters the plan
/// module gives it.
#[test]
#[ignore = "sends requests to sourcegraph.com"]
fn drift_plan() {
    use sego::internal::search::plan::{Plan, plan};
    let endpoint =
        Endpoint::from_env(None::<&OsStr>).unwrap_or_else(|e| panic!("{e}"));
    let upstream = Ureq::new(endpoint);
    let mut failures = Vec::new();
    for query in PLANNED {
        let Plan::Read(searches) = plan(query) else { panic!("{query}") };
        let mode = searches.pattern_type.name();
        let mine: Vec<Vec<String>> = searches
            .list(4096)
            .unwrap_or_default()
            .into_iter()
            .map(|filters| {
                filters
                    .into_iter()
                    .map(|f| {
                        let dash = if f.negated { "-" } else { "" };
                        let mark = if f.predicate { " [p]" } else { "" };
                        format!("{dash}{}:{}{mark}", f.field, f.value)
                    })
                    .collect()
            })
            .collect();
        let var = Var { name: "q", ty: "String!", value: json!(query) };
        let selection = format!(
            "parseSearchQuery(query: $q, patternType: {mode}, \
             outputPhase: PARSE_TREE, outputFormat: JSON)"
        );
        let request = top_level("DriftPlan", vec![var], &selection);
        let envelope =
            exchange(&upstream, &request, Instant::now() + DEADLINE)
                .unwrap_or_else(|e| panic!("{query}: {e}"));
        let tree =
            envelope.data["parseSearchQuery"].as_str().unwrap_or("null");
        let tree: Value =
            serde_json::from_str(tree).unwrap_or_else(|e| panic!("{e}"));
        let theirs = tree_searches(&tree);
        if mine != theirs {
            failures.push(format!(
                "{query}\n  sego:   {mine:?}\n  server: {theirs:?}"
            ));
        }
        sleep(PAUSE);
    }
    assert!(
        failures.is_empty(),
        "{} of {} plans differ from the server's parse tree:\n{}",
        failures.len(),
        PLANNED.len(),
        failures.join("\n")
    );
}
