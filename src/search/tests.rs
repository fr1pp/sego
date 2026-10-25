use std::ffi::OsStr;
use std::time::Instant;

use serde_json::Value;

use crate::search::query::prepare;
use crate::search::response::{Listing, UnindexedFile};
use crate::upstream::{
    DEADLINE, Endpoint, Envelope, Ureq, Var, exchange, top_level,
};

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
fn the_rules_sego_relies_on_for_counts_and_unindexed_files_still_hold_on_the_live_server()
 {
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
        Endpoint::parse(None::<&OsStr>).unwrap_or_else(|e| panic!("{e}"));
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
