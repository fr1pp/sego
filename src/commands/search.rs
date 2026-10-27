mod answer;
mod checks;
mod plan;
mod query;

use crate::error::{Error, Kind};
use crate::output::Notes;
use crate::sourcegraph::{Client, Listing, Search};

pub use answer::Answer;
use plan::PatternType;
use query::Query;

fn pointed(error: Error) -> Error {
    if error.kind() == Kind::Deadline {
        error.fact("repo:, lang: or file: narrows what the search reads")
    } else {
        error
    }
}

const REASON_CHARS: usize = 200;

/// The error for a search whose regular expression the server's Go regexp
/// couldn't compile, as a structural hole the query syntax lets through.
fn regexp_error(reason: &str, query: &Query) -> Error {
    let reason = if reason.chars().count() > REASON_CHARS {
        format!("{}…", reason.chars().take(REASON_CHARS).collect::<String>())
    } else {
        reason.to_owned()
    };
    let error = Error::usage_from_response(format!(
        "the search's regular expression doesn't compile: {reason}"
    ));
    if query.pattern_type == PatternType::Structural {
        error
            .fact("structural search compiles each :[name~regex] hole as an RE2 regular expression")
    } else {
        error
    }
}

/// Whether the side query's listing holds every unindexed file: no alert,
/// nothing cut, and back before its deadline.
fn complete(listing: &Listing, deadline_ms: u64) -> bool {
    listing.alert.is_none()
        && !listing.limit_hit
        && listing.timedout.is_empty()
        && listing.elapsed_milliseconds < deadline_ms
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub query: String,
    pub json: bool,
}

pub fn run(args: &Args, client: Client, notes: &mut Notes) -> Result<Answer, Error> {
    let query = Query::new(&args.query);
    for note in &query.notes {
        notes.push(note);
    }
    let search = client.search(&query.text, query.unindexed.as_deref()).map_err(pointed)?;
    let (results, listing) = match search {
        Search::Results { results, unindexed } => (results, unindexed),
        Search::BadRegexp { reason } => return Err(regexp_error(&reason, &query)),
    };
    let deadline_ms = query.deadline_ms;
    // An empty listing can't be told from one that failed under load, so it
    // leaves the files skipped unknown (ADR 0007).
    let unindexed = listing
        .filter(|listing| complete(listing, deadline_ms))
        .map(Listing::files)
        .filter(|files| !files.is_empty());
    let answer = Answer::new(query, results, unindexed);
    answer.judge(notes)?;
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsStr;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use crate::cache::Cache;
    use crate::error::{Error, report};
    use crate::sourcegraph::{DEADLINE, Endpoint, Listing, Results};

    const SQLITE_FILE: &str = "c_src/sqlite3.c";
    const X509_FILE: &str = "src/crypto/x509/x509_test.go";
    const TRIGRAM_LIMIT: u64 = 1_048_576;
    const PAUSE: Duration = Duration::from_secs(1);

    type Rule = fn() -> Result<(), String>;

    /// sourcegraph.com's results for `query`, and the listing of the side
    /// query sego sends with it.
    fn searched_live(query: &str) -> Result<(Results, Option<Listing>), String> {
        sleep(PAUSE);
        let query = Query::new(query);
        let deadline = Instant::now() + DEADLINE;
        let cache = Cache::new(Some(OsStr::new("off")), &Endpoint::default());
        let client =
            Client::new(crate::connect(Endpoint::default()), Endpoint::default(), &cache, deadline);
        match client.search(&query.text, query.unindexed.as_deref()) {
            Ok(Search::Results { results, unindexed }) => Ok((results, unindexed)),
            Ok(Search::BadRegexp { reason }) => Err(format!("{}: {reason}", query.text)),
            Err(e) => Err(format!("{}: {}", query.text, report(&e))),
        }
    }

    fn listed_bytes(query: &str, path: &str) -> Result<u64, String> {
        let (_, listing) = searched_live(query)?;
        let files = listing.ok_or_else(|| format!("{query}: no listing"))?.files();
        files.iter().find(|f| f.path == path).map(|f| f.bytes).ok_or_else(|| {
            let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
            format!("{path} is not listed (listed: {paths:?})")
        })
    }

    fn placeholder_listing() -> Result<(), String> {
        let bytes = listed_bytes(r"repo:^github\.com/elixir-sqlite/exqlite$ zqxv", SQLITE_FILE)?;
        if bytes > TRIGRAM_LIMIT {
            Ok(())
        } else {
            Err(format!("{SQLITE_FILE} is {bytes} bytes, not over {TRIGRAM_LIMIT}"))
        }
    }

    fn trigram_reason() -> Result<(), String> {
        let query = r"repo:^github\.com/golang/go$ file:^src/crypto/x509/ zqxv";
        let bytes = listed_bytes(query, X509_FILE)?;
        if bytes <= TRIGRAM_LIMIT {
            Ok(())
        } else {
            Err(format!("{X509_FILE} is {bytes} bytes, over {TRIGRAM_LIMIT}"))
        }
    }

    fn cap_and_plus() -> Result<(), String> {
        let (results, _) = searched_live(r"repo:^github\.com/gorilla/mux$ func")?;
        let (count, hit) = (results.match_count, results.limit_hit);
        if count == 30 && hit {
            Ok(())
        } else {
            Err(format!("matchCount {count}, limitHit {hit}; want 30 and true"))
        }
    }

    fn complete_answer() -> Result<(), String> {
        let (results, _) = searched_live(r"repo:^github\.com/gorilla/mux$ catchallhandler")?;
        let (count, hit) = (results.match_count, results.limit_hit);
        if (1..30).contains(&count) && !hit {
            Ok(())
        } else {
            Err(format!("matchCount {count}, limitHit {hit}; want 1 to 29 and false"))
        }
    }

    fn utility_counts() -> Result<(), String> {
        let (results, _) = searched_live(r"repo:^github\.com/gorilla/ zqxjkvbnmwqtyplkh")?;
        let found = results.dynamic_filters.iter().find(|f| {
            f.value == "archived:yes" && f.kind.eq_ignore_ascii_case("utility") && f.count >= 1
        });
        match found {
            Some(_) => Ok(()),
            None => Err(format!(
                "no utility filter archived:yes with count >= 1 ({:?})",
                results.dynamic_filters
            )),
        }
    }

    /// The local check `symbols_of_content` notes that this returns nothing.
    fn symbols_of_content() -> Result<(), String> {
        let (results, _) =
            searched_live(r"repo:^github\.com/gorilla/mux$ NewRouter select:symbol")?;
        match results.match_count {
            0 => Ok(()),
            n => Err(format!("matchCount {n}; select:symbol over content now answers")),
        }
    }

    /// The `repos` half of `lower_bound`: over these five repositories, a
    /// capped repo select answers fewer than `count:all`, unflagged (#32).
    fn repo_select_cut() -> Result<(), String> {
        const SCOPE: &str = r"repo:^github\.com/(torvalds/linux|chromium/chromium|kubernetes/kubernetes|golang/go|gorilla/mux)$ func select:repo timeout:60s";
        let (capped, _) = searched_live(&format!("{SCOPE} count:10"))?;
        let (every, _) = searched_live(&format!("{SCOPE} count:all"))?;
        let stopped = |r: &Results| !r.timedout.is_empty() || r.elapsed_milliseconds >= 60_000;
        if stopped(&capped) || stopped(&every) || every.limit_hit {
            return Err("a search stopped or count:all was cut; run it again".to_owned());
        }
        let (n, all) = (capped.match_count, every.match_count);
        match (n < all, capped.limit_hit) {
            (true, false) => Ok(()),
            (true, true) => Err(format!(
                "{n} of {all} repositories with limitHit true; the server now reports this cut, \
                 so the repos half of lower_bound may go"
            )),
            (false, _) => Err(format!(
                "{n} of {all} repositories; no cut happened, so this scope no longer shows one"
            )),
        }
    }

    #[test]
    #[ignore = "sends requests to sourcegraph.com"]
    fn the_rules_search_relies_on_for_counts_and_unindexed_files_still_hold_on_the_live_server() {
        let rules: [(&str, Rule); 7] = [
            ("placeholder listing", placeholder_listing),
            ("trigram reason", trigram_reason),
            ("cap and plus", cap_and_plus),
            ("complete answer", complete_answer),
            ("utility counts", utility_counts),
            ("symbols of content", symbols_of_content),
            ("repo select cut", repo_select_cut),
        ];
        let failures: Vec<String> = rules
            .iter()
            .filter_map(|(name, check)| check().err().map(|why| format!("{name}: {why}")))
            .collect();
        assert!(
            failures.is_empty(),
            "{} of {} rules no longer hold:\n{}",
            failures.len(),
            rules.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn a_search_past_the_client_deadline_points_to_a_narrower_scope() {
        let error = pointed(Error::deadline("sourcegraph.com"));
        assert_eq!(
            report(&error),
            "sego: error: no answer from sourcegraph.com within 90 s\n  repo:, lang: or file: \
         narrows what the search reads\n"
        );
    }

    #[test]
    fn a_search_that_fails_to_connect_has_no_pointer_to_a_narrower_scope() {
        let error = Error::network("could not reach sourcegraph.com: connection refused");
        assert_eq!(
            report(&pointed(error)),
            "sego: error: could not reach sourcegraph.com: connection refused\n"
        );
    }
}
