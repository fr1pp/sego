mod answer;
mod query;
mod read;

use crate::error::Error;
use crate::output::Notes;
use crate::sourcegraph::{Client, Search};

pub use answer::Answer;
use query::Query;
use read::PatternType;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub query: String,
    pub json: bool,
}

pub fn run(args: &Args, client: Client, notes: &mut Notes) -> Result<Answer, Error> {
    let query = Query::new(&args.query);
    let results = match client.search(&query.text)? {
        Search::Results(results) => results,
        Search::BadRegexp { reason } => return Err(regexp_error(&reason, &query)),
    };
    let answer = Answer::new(query, results);
    answer.judge(notes)?;
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsStr;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use crate::error::report;
    use crate::sourcegraph::{DEADLINE, Endpoint, Item, Results};

    const PAUSE: Duration = Duration::from_millis(500);

    type Rule = fn() -> Result<(), String>;

    /// sourcegraph.com's results for `query`.
    fn searched_live(query: &str) -> Result<Results, String> {
        sleep(PAUSE);
        let query = Query::new(query);
        let deadline = Instant::now() + DEADLINE;
        let cache = Endpoint::default().cache(Some(OsStr::new("off")));
        let client =
            Client::new(crate::connect(Endpoint::default()), Endpoint::default(), &cache, deadline);
        match client.search(&query.text) {
            Ok(Search::Results(results)) => Ok(results),
            Ok(Search::BadRegexp { reason }) => Err(format!("{}: {reason}", query.text)),
            Err(e) => Err(format!("{}: {}", query.text, report(&e))),
        }
    }

    fn cap_and_plus() -> Result<(), String> {
        let results = searched_live(r"repo:^github\.com/gorilla/mux$ func")?;
        let (count, hit) = (results.match_count, results.limit_hit);
        if count == 30 && hit {
            Ok(())
        } else {
            Err(format!("matchCount {count}, limitHit {hit}; want 30 and true"))
        }
    }

    fn complete_answer() -> Result<(), String> {
        let results = searched_live(r"repo:^github\.com/gorilla/mux$ catchallhandler")?;
        let (count, hit) = (results.match_count, results.limit_hit);
        if (1..30).contains(&count) && !hit {
            Ok(())
        } else {
            Err(format!("matchCount {count}, limitHit {hit}; want 1 to 29 and false"))
        }
    }

    fn utility_counts() -> Result<(), String> {
        let results = searched_live(r"repo:^github\.com/golang/ zqxjkvbnmwqtyplkh")?;
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

    /// `FileMatch::placeholder_reason` still reads the index's placeholder:
    /// bat's binary assets are among its unindexed files.
    fn placeholder_format() -> Result<(), String> {
        let results = searched_live(r"repo:^github\.com/sharkdp/bat$ NOT-INDEXED count:all")?;
        let reasons: Vec<&str> = results
            .items
            .iter()
            .filter_map(|item| match item {
                Item::FileMatch(file) => file.placeholder_reason(),
                _ => None,
            })
            .collect();
        if reasons.contains(&"contains binary content") {
            Ok(())
        } else {
            Err(format!(
                "{} results, none a placeholder for binary content (reasons read: {reasons:?}); \
                 a zero may be a backend that failed to answer, so run it again",
                results.items.len()
            ))
        }
    }

    /// sego sends `timeout:60s` as the most sourcegraph.com allows: a search
    /// that runs longer stops at 60 s, whatever `timeout:` it asks for.
    fn deadline_ceiling() -> Result<(), String> {
        let results =
            searched_live(r"repo:^github\.com/torvalds/linux$ type:diff zqxjvkwq timeout:90s")?;
        let ms = results.elapsed_milliseconds;
        let stopped = !results.timedout.is_empty();
        if ms > 85_000 {
            Err(format!("ran {ms} ms; the most allowed is no longer 60 s"))
        } else if ms >= 59_000 {
            Ok(())
        } else if stopped {
            Err(format!("stopped early at {ms} ms, likely under load; run it again"))
        } else {
            Err(format!("the search finished in {ms} ms; it no longer runs long"))
        }
    }

    #[test]
    #[ignore = "sends requests to sourcegraph.com"]
    fn the_rules_search_relies_on_still_hold_on_the_live_server() {
        let rules: [(&str, Rule); 5] = [
            ("placeholder format", placeholder_format),
            ("cap and plus", cap_and_plus),
            ("complete answer", complete_answer),
            ("utility counts", utility_counts),
            ("deadline ceiling", deadline_ceiling),
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
}
