use std::collections::HashMap;

use crate::address::pinned;
use crate::error::Error;
use crate::escape::shell_word;
use crate::output::{Notes, count};

use super::text::tenths;
use super::{Alert, Answer};
use crate::sourcegraph::UnindexedFile;

const FACTS: usize = 2;
const MIB: u64 = 1 << 20;
const KIB: u64 = 1 << 10;
const LISTED: usize = 3;
const PROCESS_FAILURE: &str = "Unable To Process Query";
const REVISIONS: &str = "Some revisions could not be searched";
const NO_REPOSITORIES: &str = "No repositories found";
const PINNED: &str = "at a pinned revision the search can't tell which files it skipped; files over 1 MiB are never searched";
const UNLISTED: &str = "the search can't tell which files it skipped; files over 1 MiB and files with too many distinct trigrams are never searched";
const UNLISTED_INDEX_NO: &str =
    "the search can't tell which files it skipped; files over 1 MiB are never searched";

impl Answer {
    /// Writes the answer's notes, or the error it is: an alert that means
    /// the search failed, or a stopped search with nothing in it.
    pub fn judge(&self, notes: &mut Notes) -> Result<(), Error> {
        if let Some(alert) = &self.alert {
            if alert.title == PROCESS_FAILURE {
                return Err(Error::upstream(description(alert).unwrap_or(&alert.title)));
            }
            if self.is_zero() {
                if alert.title == REVISIONS {
                    return Err(revisions(alert));
                }
                if alert.title == NO_REPOSITORIES {
                    let error = proposals(alert, Error::missing(&alert.title));
                    return Err(match &self.query.repo_names {
                        Some(names) => error.fact(format!(
                            "sego search {} lists the matching repositories",
                            shell_word(names)
                        )),
                        None => error,
                    });
                }
            }
        }
        if self.is_zero() && self.stopped {
            return Err(Error::missing(format!(
                "the search stopped at {} s with no results",
                tenths(self.elapsed_ms)
            )));
        }
        push_notes(self, notes);
        Ok(())
    }
}

fn description(alert: &Alert) -> Option<&str> {
    alert.description.as_deref().filter(|d| !d.trim().is_empty())
}

fn revisions(alert: &Alert) -> Error {
    let Some(text) = description(alert) else {
        return Error::missing(&alert.title);
    };
    let lines: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("* "))
        .filter_map(|rest| rest.split_once('@'))
        .map(|(repo, rev)| format!("revision {rev} not found in {repo}"))
        .collect();
    let Some((first, rest)) = lines.split_first() else {
        return Error::missing(text);
    };
    let error = Error::missing(first);
    match rest {
        [] => error,
        [one] => error.fact(one),
        [one, two] => error.fact(one).fact(two),
        [one, ..] => {
            error.fact(one).fact(format!("and {} more revisions not found", lines.len() - FACTS))
        }
    }
}

fn proposals(alert: &Alert, mut error: Error) -> Error {
    for proposed in alert.proposed.iter().take(FACTS) {
        error = error.fact(format!("the server proposes: {}", proposed.query));
    }
    error
}

fn push_notes(searched: &Answer, notes: &mut Notes) {
    if let Some(note) = &searched.query.flag_note
        && searched.is_zero()
        && !searched.at_least
    {
        notes.push(note);
    }
    if let Some(alert) = &searched.alert {
        match description(alert) {
            Some(text) => notes.push(format!("{}: {text}", alert.title)),
            None => notes.push(&alert.title),
        }
        for proposed in &alert.proposed {
            notes.push(format!("the server proposes: {}", proposed.query));
        }
    }
    let mut order: Vec<&str> = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for kind in &searched.unshown {
        let count = counts.entry(kind).or_insert(0);
        if *count == 0 {
            order.push(kind);
        }
        *count += 1;
    }
    for kind in order {
        match counts[kind] {
            1 => notes.push(format!("1 result of kind {kind} not shown")),
            n => notes.push(format!("{n} results of kind {kind} not shown")),
        }
    }
    if let Some(cap) = searched.select_cut {
        notes.push(format!(
            "at a pinned revision, select: keeps only what each repository's first {cap} matches hold; count:all gives every one"
        ));
    }
    if let Some((first, rest)) = unindexed_note(searched) {
        notes.push_lines(first, rest);
    }
    if let Some(note) = unlisted_note(searched) {
        notes.push(note);
    }
}

/// The note on a content search that can't list the files it skipped, for
/// a zero or a complete answer. At a pin it is only for a zero: hits show
/// the files were searched, and a file over 1 MiB they miss goes unsaid.
fn unlisted_note(searched: &Answer) -> Option<&'static str> {
    if searched.unindexed.is_some()
        || !searched.query.content_search
        || searched.stopped
        || searched.at_least
    {
        return None;
    }
    Some(if searched.query.pinned {
        if searched.count > 0 {
            return None;
        }
        PINNED
    } else if searched.query.index_no {
        UNLISTED_INDEX_NO
    } else {
        UNLISTED
    })
}

/// The note on the text files in scope that the search index doesn't hold,
/// and the search didn't read, for a zero or a complete answer; a lower
/// bound or a stopped answer says nothing about them. A path is relative
/// only when the results and the files are all in one repository, which
/// the results already name.
fn unindexed_note(searched: &Answer) -> Option<(String, Vec<String>)> {
    if searched.stopped || searched.at_least {
        return None;
    }
    let files: Vec<&UnindexedFile> = searched
        .unindexed
        .iter()
        .flatten()
        .filter(|file| !searched.query.index_no || file.bytes > MIB)
        .collect();
    if files.is_empty() {
        return None;
    }
    let (mut over, mut dense): (Vec<&UnindexedFile>, Vec<&UnindexedFile>) =
        files.iter().copied().partition(|file| file.bytes > MIB);
    over.sort_by_key(|file| std::cmp::Reverse(file.bytes));
    dense.sort_by_key(|file| std::cmp::Reverse(file.bytes));
    let reasons: Vec<String> =
        [(over.len(), "over 1 MiB"), (dense.len(), "too many distinct trigrams")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, reason)| format!("{n} {reason}"))
            .collect();
    let first = format!(
        "the contents of {} in scope aren't in the search index ({}):",
        count(files.len(), "file", "files"),
        reasons.join(", ")
    );
    let relative = !searched.groups.is_empty()
        && searched.groups.iter().all(|group| files.iter().all(|file| file.repo == group.repo()));
    let mut rest: Vec<String> = over
        .iter()
        .chain(&dense)
        .take(LISTED)
        .map(|file| {
            let path = if relative {
                shell_word(&file.path).into_owned()
            } else {
                pinned(&file.repo, &file.commit, Some(&file.path)).to_string()
            };
            format!("  {path}  {}", binary_size(file.bytes))
        })
        .collect();
    if files.len() > LISTED {
        rest.push(format!(
            "  … (+{}; --json has the full list)",
            count(files.len() - LISTED, "more file", "more files")
        ));
    }
    Some((first, rest))
}

fn binary_size(bytes: u64) -> String {
    if bytes >= MIB {
        format!("{} MiB", tenths_of(bytes, MIB))
    } else if bytes >= KIB {
        format!("{} KiB", tenths_of(bytes, KIB))
    } else {
        format!("{bytes} B")
    }
}

/// `bytes / unit` to one decimal place, rounded half up.
fn tenths_of(bytes: u64, unit: u64) -> String {
    let tenths = (u128::from(bytes) * 10 + u128::from(unit) / 2) / u128::from(unit);
    format!("{}.{}", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::commands::search::query::Query;
    use crate::sourcegraph::Results;

    const MIB: u64 = 1_048_576;
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    const PINNED_NOTE: &str = "sego: note: at a pinned revision the search \
    can't tell which files it skipped; files over 1 MiB are never searched\n";
    const UNLISTED_NOTE: &str = "sego: note: the search can't tell which files \
    it skipped; files over 1 MiB and files with too many distinct trigrams \
    are never searched\n";

    fn results(overrides: &Value) -> Results {
        let mut base = json!({
            "matchCount": 0,
            "approximateResultCount": "0",
            "limitHit": false,
            "timedout": [],
            "alert": null,
            "elapsedMilliseconds": 100,
            "dynamicFilters": [],
            "results": [],
        });
        if let (Some(map), Some(target)) = (overrides.as_object(), base.as_object_mut()) {
            target.extend(map.clone());
        }
        match serde_json::from_value(base) {
            Ok(results) => results,
            Err(e) => panic!("bad response: {e}"),
        }
    }

    fn repo(name: &str) -> Value {
        json!({"__typename": "Repository", "name": name})
    }

    fn one_result() -> Value {
        json!({"matchCount": 1, "results": [repo("x")]})
    }

    fn file(repo: &str, path: &str, bytes: u64) -> UnindexedFile {
        UnindexedFile { repo: repo.into(), commit: COMMIT.into(), path: path.into(), bytes }
    }

    /// The notes `judge` writes for the response to `query`, given the files
    /// the side query listed, which is `None` when it wasn't sent.
    fn notes_for(query: &str, overrides: &Value, unindexed: Option<Vec<UnindexedFile>>) -> String {
        let answer = Answer::new(Query::new(query), results(overrides), unindexed);
        let mut notes = Notes::new();
        assert!(answer.judge(&mut notes).is_ok(), "{query}");
        let mut printed = Vec::new();
        assert!(notes.write(&mut printed).is_ok());
        String::from_utf8_lossy(&printed).into_owned()
    }

    #[test]
    fn binary_sizes_are_written_in_the_largest_whole_unit() {
        assert_eq!(binary_size(4_273_688), "4.1 MiB");
        assert_eq!(binary_size(263_448), "257.3 KiB");
        assert_eq!(binary_size(12), "12 B");
        assert_eq!(binary_size(1023), "1023 B");
        assert_eq!(binary_size(1024), "1.0 KiB");
        assert_eq!(binary_size(MIB), "1.0 MiB");
    }

    #[test]
    fn a_content_search_that_cant_list_its_unindexed_files_says_so() {
        for query in [
            "repo:x a or b",
            r"repo:^github\.com/sharkdp/bat$ (syntax_request.md or zzqqxx)",
            "repo:x foo",
            "foo",
            "repo:x type:symbol foo",
            "type:symbol foo",
        ] {
            for overrides in [json!({}), one_result()] {
                assert_eq!(notes_for(query, &overrides, None), UNLISTED_NOTE, "{query}");
            }
        }
    }

    #[test]
    fn a_search_that_reads_no_contents_has_no_unlisted_note() {
        for query in [
            "repo:x",
            r"repo:x file:\.go$",
            "repo:x type:path foo",
            "repo:x type:bogus foo",
            "repo:x@v1 type:bogus foo",
            "repo:x type:commit foo",
            "repo:x type:diff foo",
            "repo:x type:repo foo",
        ] {
            assert_eq!(notes_for(query, &json!({}), None), "", "{query}");
        }
    }

    #[test]
    fn a_pinned_content_search_has_the_pinned_note() {
        for query in [
            "repo:x@v1 a or b",
            r"repo:^github\.com/gorilla/mux$@v1.8.0 file:^mux\.go$ (StrictSlash or zzqqxx)",
            "repo:x@v1 index:no a or b",
            "repo:x@v1 type:symbol foo",
        ] {
            assert_eq!(notes_for(query, &json!({}), None), PINNED_NOTE, "{query}");
        }
    }

    #[test]
    fn a_head_or_empty_revision_takes_the_default_branch_notes() {
        let listed = |path| {
            format!(
                "sego: note: the contents of 1 file in scope aren't in the \
             search index (1 over 1 MiB):\n  {path}  2.0 MiB\n"
            )
        };
        for query in [
            "repo:x@HEAD foo",
            "repo:x@ foo",
            "repo:x rev:HEAD foo",
            "repo:x revision:HEAD foo",
            "repo:x@HEAD a or b",
        ] {
            for overrides in [json!({}), one_result()] {
                assert_eq!(notes_for(query, &overrides, None), UNLISTED_NOTE, "{query}");
            }
        }
        for query in ["repo:x@HEAD foo", "repo:x@ foo", "repo:x rev:HEAD foo"] {
            for (overrides, path) in [(json!({}), "x@0123456789ab:big"), (one_result(), "big")] {
                let files = Some(vec![file("x", "big", 2 * MIB)]);
                assert_eq!(notes_for(query, &overrides, files), listed(path), "{query}");
            }
        }
        assert_eq!(notes_for("repo:x@HEAD:v1 foo", &json!({}), None), PINNED_NOTE);
        assert_eq!(notes_for("repo:x@HEAD:v1 foo", &one_result(), None), "");
    }
}
