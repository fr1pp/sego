use super::read::{
    KNOWN, PatternType, Written, WrittenFilter as Filter, chosen, has_pattern, unquoted,
};

const GUESSED: &[&str] = &["repogroup", "ext", "extension", "filename", "branch", "org"];
const RESULT_TYPES: &[&str] = &["file", "path", "repo", "symbol", "commit", "diff", "structural"];
const PATTERN_FIELDS: &[&str] = &["file", "f", "path", "lang", "l", "language", "content"];

/// The notes for a query, in check order. Each fires only on a query that
/// can't be meant as written.
pub fn check(written: &[Written], ty: PatternType) -> Vec<String> {
    let mut notes = Vec::new();
    unknown_filters(written, &mut notes);
    regex_in_keyword_mode(written, ty, &mut notes);
    unknown_types(written, &mut notes);
    path_without_pattern(written, &mut notes);
    revision_ranges(written, &mut notes);
    symbol_select_without_pattern(written, ty, &mut notes);
    symbols_of_content(written, &mut notes);
    notes
}

fn filters(written: &[Written]) -> impl Iterator<Item = &Filter> {
    written.iter().filter_map(|item| match item {
        Written::Filter(filter) => Some(filter),
        _ => None,
    })
}

fn is_field(filter: &Filter, names: &[&str]) -> bool {
    names.iter().any(|name| filter.field.eq_ignore_ascii_case(name))
}

/// Check 1: a pattern written as a filter of a field the server doesn't
/// know, which it searches as text, when the field is a common guess or one
/// edit from a known field.
fn unknown_filters(written: &[Written], notes: &mut Vec<String>) {
    let mut seen = Vec::new();
    for item in written {
        let Written::Pattern { text, delimited: false, .. } = item else { continue };
        let Some(field) = unknown_field(text) else { continue };
        let lower = field.to_ascii_lowercase();
        if !GUESSED.contains(&lower.as_str()) && !near_known(&lower) {
            continue;
        }
        if seen.contains(text) {
            continue;
        }
        notes.push(format!("'{field}:' is not a filter; the server searches `{text}` as text"));
        seen.push(text.clone());
    }
}

/// The field of a pattern written as `field:value` or `-field:value`, where
/// the field is letters and the value doesn't start with `:`.
fn unknown_field(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('-').unwrap_or(text);
    let (field, value) = rest.split_once(':')?;
    let letters = !field.is_empty() && field.chars().all(|c| c.is_ascii_alphabetic());
    (letters && !value.is_empty() && !value.starts_with(':')).then_some(field)
}

/// One adjacent transposition, insertion or deletion from a known field of
/// four or more letters. A known field with a trailing `s` and `patch` are
/// common keys in code, so they are not near.
fn near_known(field: &str) -> bool {
    let plural = field.strip_suffix('s').is_some_and(|stem| KNOWN.contains(&stem));
    if plural || field == "patch" {
        return false;
    }
    let f: Vec<char> = field.chars().collect();
    KNOWN.iter().filter(|known| known.len() >= 4).any(|known| {
        let k: Vec<char> = known.chars().collect();
        match f.len().cmp(&k.len()) {
            std::cmp::Ordering::Equal => (0..f.len().saturating_sub(1)).any(|i| {
                let mut swapped = f.clone();
                swapped.swap(i, i + 1);
                swapped == k
            }),
            std::cmp::Ordering::Greater => drops_one(&f, &k),
            std::cmp::Ordering::Less => drops_one(&k, &f),
        }
    })
}

/// Whether removing one character from `long` gives `short`.
fn drops_one(long: &[char], short: &[char]) -> bool {
    (0..long.len()).any(|i| long[..i].iter().chain(&long[i + 1..]).eq(short.iter()))
}

fn regex_in_keyword_mode(written: &[Written], ty: PatternType, notes: &mut Vec<String>) {
    if ty != PatternType::Keyword {
        return;
    }
    let first = written.iter().find_map(|item| match item {
        Written::Pattern { text, delimited: false, .. }
            if looks_like_regex(text) && flags(text).is_none() =>
        {
            Some(text)
        }
        _ => None,
    });
    if let Some(term) = first {
        notes.push(format!("keyword mode matches `{term}` literally; a regex goes inside /.../"));
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn looks_like_regex(term: &str) -> bool {
    let c: Vec<char> = term.chars().collect();
    let at = |i: usize| c.get(i).copied();
    if matches!(c.first(), Some('^'))
        && at(1).is_some_and(|n| n.is_ascii_alphabetic() || n == '\\' || n == '[')
    {
        return true;
    }
    (0..c.len()).any(|i| match c[i] {
        '\\' => at(i + 1).is_some_and(|n| {
            (i > 0 && is_word(c[i - 1]) && ".()[]{}|*+?^/".contains(n))
                || ("wWsSdDbB".contains(n) && !at(i + 2).is_some_and(|a| a.is_ascii_alphabetic()))
        }),
        '.' => {
            matches!(at(i + 1), Some('*' | '+')) && at(i + 2).is_some_and(|n| n != ';' && n != '.')
        }
        '[' => class_of_ranges(&c[i + 1..]),
        _ => false,
    })
}

/// Whether the bracket body starting `rest` holds two ranges, or one range
/// followed by a quantifier.
fn class_of_ranges(rest: &[char]) -> bool {
    let Some(end) = rest.iter().position(|&x| x == ']') else {
        return false;
    };
    let is_range = |x: char, y: char| {
        x < y
            && ((x.is_ascii_lowercase() && y.is_ascii_lowercase())
                || (x.is_ascii_uppercase() && y.is_ascii_uppercase())
                || (x.is_ascii_digit() && y.is_ascii_digit()))
    };
    let mut ranges = 0;
    let mut body = &rest[..end];
    while let [x, tail @ ..] = body {
        body = match tail {
            ['-', y, after @ ..] if is_range(*x, *y) => {
                ranges += 1;
                after
            }
            _ => tail,
        };
    }
    ranges >= 2 || (ranges == 1 && matches!(rest.get(end + 1), Some('+' | '*' | '?' | '{')))
}

fn unknown_types(written: &[Written], notes: &mut Vec<String>) {
    let mut seen: Vec<&str> = Vec::new();
    for filter in filters(written).filter(|f| !f.negated && is_field(f, &["type"])) {
        let value = filter.value.as_str();
        let bare = unquoted(value);
        if RESULT_TYPES.contains(&bare) || seen.contains(&value) {
            continue;
        }
        seen.push(value);
        notes.push(format!("type:{value} is not a result type; the server returns nothing"));
    }
}

fn path_without_pattern(written: &[Written], notes: &mut Vec<String>) {
    let typed = filters(written).any(|f| !f.negated && is_field(f, &["type"]) && f.value == "path");
    let only_filters = !has_pattern(written);
    if typed && only_filters && !filters(written).any(|f| is_field(f, PATTERN_FIELDS)) {
        notes.push("type:path with only filters returns nothing".to_owned());
    }
}

fn revision_ranges(written: &[Written], notes: &mut Vec<String>) {
    for filter in filters(written).filter(|f| is_field(f, &["repo", "r"])) {
        let Some((_, revisions)) = filter.value.split_once('@') else {
            continue;
        };
        let end = revisions
            .split(':')
            .find(|rev| rev.contains(".."))
            .and_then(|rev| rev.rsplit_once("..").map(|(_, end)| end));
        if let Some(end) = end.filter(|end| !end.is_empty()) {
            let note = format!("{} searches {end} only", filter.raw());
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }
}

/// A `content:` filter or a path's content predicate supplies the pattern:
/// the server searches the predicate's argument as one. Negated,
/// the server rejects either, so neither gets the note. A field the server
/// doesn't know is searched as text. In literal and structural mode `/./`
/// is text, and the server rejects a second `patterntype:`, so the note
/// names the one to replace.
fn symbol_select_without_pattern(written: &[Written], ty: PatternType, notes: &mut Vec<String>) {
    let symbols = chosen(written, "type").any(|v| unquoted(v) == "symbol");
    let selected = chosen(written, "select").any(|v| unquoted(v).starts_with("symbol"));
    let supplies = |f: &Filter| {
        is_field(f, &["content"])
            || (is_field(f, &["file", "f", "path"])
                && (f.value.starts_with("has.content(")
                    || f.value.starts_with("contains.content(")))
    };
    if !symbols || !selected || has_pattern(written) || filters(written).any(supplies) {
        return;
    }
    let remedy = if matches!(ty, PatternType::Literal | PatternType::Structural) {
        let mode = ty.name();
        format!("with patterntype:regexp in place of patterntype:{mode}, /./")
    } else {
        "/./".to_owned()
    };
    notes.push(format!(
        "type:symbol with select:symbol and no pattern returns nothing; \
         {remedy} matches every symbol"
    ));
}

/// A content search's `select:symbol` returns nothing on sourcegraph.com,
/// pinned or not (live rule `symbols_of_content`).
fn symbols_of_content(written: &[Written], notes: &mut Vec<String>) {
    let selected = chosen(written, "select").any(|v| unquoted(v).starts_with("symbol"));
    let content = chosen(written, "type").all(|v| unquoted(v) == "file");
    if selected && content && has_pattern(written) {
        notes.push(
            "select:symbol over a content search returns nothing on sourcegraph.com; \
             type:symbol searches symbols"
                .to_owned(),
        );
    }
}

const FLAGS: &str = "gims";
const ROOTS: &[&str] = &[
    "bin", "boot", "dev", "etc", "home", "lib", "media", "mnt", "opt", "proc", "root", "run",
    "sbin", "srv", "sys", "tmp", "usr", "var",
];

/// The flags of `/RE/FLAGS`: a `/`, text up to the first unescaped `/`,
/// then one or more of `FLAGS`, each at most once, in any order. The server
/// searches the whole term as text. Text that is a top-level Unix directory
/// makes it a path, such as `/usr/i`.
fn flags(term: &str) -> Option<&str> {
    let rest = term.strip_prefix('/')?;
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '/' => {
                let (body, flags) = (&rest[..i], &rest[i + 1..]);
                let set = !flags.is_empty()
                    && flags
                        .char_indices()
                        .all(|(at, flag)| FLAGS.contains(flag) && !flags[..at].contains(flag));
                return (!body.is_empty() && set && !ROOTS.contains(&body)).then_some(flags);
            }
            _ => {}
        }
    }
    None
}

/// The note for the first `/RE/FLAGS` term in `keyword` or `standard`
/// mode. It is for a zero answer only: such a term can be meant as written,
/// as a regex literal in code or a path prefix.
pub fn flag_note(written: &[Written], ty: PatternType) -> Option<String> {
    if !matches!(ty, PatternType::Keyword | PatternType::Standard) {
        return None;
    }
    let (term, flags) = written.iter().find_map(|item| match item {
        Written::Pattern { text, delimited: false, .. } => Some((text, flags(text)?)),
        _ => None,
    })?;
    let inline: String = ['s', 'm'].into_iter().filter(|&f| flags.contains(f)).collect();
    let mut clauses = Vec::new();
    if flags.contains('i') || inline.is_empty() {
        clauses.push(if chosen(written, "case").any(|v| unquoted(v) == "yes") {
            "case:yes matches case even with (?i)"
        } else {
            "it ignores case by default"
        });
    }
    let example = format!("flags go inside: /(?{inline}).../");
    if !inline.is_empty() {
        clauses.push(&example);
    }
    Some(format!("the server searches `{term}` as text; {}", clauses.join(", and ")))
}

#[cfg(test)]
mod tests {
    use crate::commands::search::query::Query;

    fn not_a_filter(field: &str, token: &str) -> String {
        format!("'{field}:' is not a filter; the server searches `{token}` as text")
    }

    const PATH_ONLY: &str = "type:path with only filters returns nothing";

    fn keyword(term: &str) -> String {
        format!("keyword mode matches `{term}` literally; a regex goes inside /.../")
    }

    fn bad_type(value: &str) -> String {
        format!("type:{value} is not a result type; the server returns nothing")
    }

    fn run(rows: Vec<(&str, Vec<String>)>) {
        for (query, expected) in rows {
            assert_eq!(Query::new(query).notes, expected, "{query:?}");
        }
    }

    #[test]
    fn a_misspelled_filter_is_noted_and_text_with_a_colon_is_not() {
        run(vec![
            ("fiel:x", vec![not_a_filter("fiel", "fiel:x")]),
            ("langauge:go", vec![not_a_filter("langauge", "langauge:go")]),
            ("ext:go", vec![not_a_filter("ext", "ext:go")]),
            ("-lnag:go", vec![not_a_filter("lnag", "-lnag:go")]),
            ("fle:x", vec![not_a_filter("fle", "fle:x")]),
            ("langg:go", vec![not_a_filter("langg", "langg:go")]),
            ("file1:x", vec![]),
            ("fiel:", vec![]),
            ("fiel::x", vec![]),
            (
                "fiel:x fiel:x fiel:y",
                vec![not_a_filter("fiel", "fiel:x"), not_a_filter("fiel", "fiel:y")],
            ),
            ("std::mem", vec![]),
            ("https://x", vec![]),
            ("TODO:", vec![]),
            ("-lang:go", vec![]),
            ("json:\"name\"", vec![]),
            ("localhost:8080", vec![]),
            ("display:none", vec![]),
            ("key:value", vec![]),
            ("files:a", vec![]),
            ("paths:a", vec![]),
            ("repos:a", vec![]),
            ("rules:a", vec![]),
            ("types:a", vec![]),
            ("messages:a", vec![]),
            ("authors:a", vec![]),
            ("contents:a", vec![]),
            ("patch:a", vec![]),
        ]);
    }

    #[test]
    fn text_that_reads_as_a_regex_in_keyword_mode_is_noted() {
        run(vec![
            ("\\bfoo\\b", vec![keyword("\\bfoo\\b")]),
            ("[a-zA-Z]", vec![keyword("[a-zA-Z]")]),
            ("[0-9]+", vec![keyword("[0-9]+")]),
            ("file\\.go", vec![keyword("file\\.go")]),
            ("fmt\\.Errorf", vec![keyword("fmt\\.Errorf")]),
            ("a.*b", vec![keyword("a.*b")]),
            ("^foo", vec![keyword("^foo")]),
            ("^[ab]", vec![keyword("^[ab]")]),
            ("^_x", vec![]),
            ("a.*.json", vec![]),
            ("[a-ab-b]", vec![]),
            ("[A-zB-z]", vec![]),
            ("[0-z1-z]", vec![]),
            ("string|null", vec![]),
            ("Subrouter|NewRouter", vec![]),
            ("dp[i-j]", vec![]),
            ("tsconfig.*.json", vec![]),
            ("..\\.gitignore", vec![]),
            ("foo|bar", vec![]),
            ("[a-z]", vec![]),
            ("\\.go", vec![]),
            ("import.*;", vec![]),
            ("\"\\.go\"", vec![]),
            ("/a.*b/", vec![]),
            ("file:\\.go$", vec![]),
            ("patterntype:regexp a.*b", vec![]),
            // the pattern type as the server reads it
            (r#"repo:x patterntype:"regexp" foo\.bar"#, vec![]),
            (r"repo:x patterntype:'regexp' foo\.bar", vec![]),
            (r"repo:x PaTtErNtYpE:regexp foo\.bar", vec![]),
            ("repo:x patterntype:literal foo (bar or baz) [a-z0-9]+", vec![keyword("[a-z0-9]+")]),
            ("a?", vec![]),
            ("a+", vec![]),
            ("a*", vec![]),
            ("a||b", vec![]),
            ("java.util.*", vec![]),
            ("PROT_READ|PROT_WRITE", vec![]),
            ("O_RDONLY|O_CREAT", vec![]),
            ("s[i-1]", vec![]),
            ("users$", vec![]),
            ("\\$HOME", vec![]),
            ("\\begin", vec![]),
            ("/func \\(r \\*Router\\) \\w+\\(/", vec![]),
        ]);
    }

    #[test]
    fn a_result_type_that_is_not_one_is_noted() {
        run(vec![
            ("type:File", vec![bad_type("File")]),
            ("type:structural", vec![]),
            ("-type:bogus", vec![]),
            ("type:\"file\"", vec![]),
        ]);
    }

    #[test]
    fn type_path_with_only_filters_is_noted() {
        run(vec![
            ("repo:x type:path", vec![PATH_ONLY.to_owned()]),
            ("type:path NewRouter", vec![]),
            ("type:path file:\\.go$", vec![]),
            ("type:path lang:go", vec![]),
            ("type:path -file:_test", vec![]),
        ]);
    }

    #[test]
    fn a_revision_range_in_a_repo_filter_is_noted_as_searching_its_end_only() {
        run(vec![
            (
                "repo:^github\\.com/gorilla/mux$@v1.8.0..v1.7.4",
                vec![
                    "repo:^github\\.com/gorilla/mux$@v1.8.0..v1.7.4 searches \
                 v1.7.4 only"
                        .to_owned(),
                ],
            ),
            ("repo:x@a:b...c", vec!["repo:x@a:b...c searches c only".to_owned()]),
            ("repo:x@a..b repo:x@a..b", vec!["repo:x@a..b searches b only".to_owned()]),
            ("repo:x@v1.8.0", vec![]),
            ("repo:x@a..", vec![]),
        ]);
    }

    #[test]
    fn several_notes_come_in_check_order() {
        run(vec![(
            "lnag:go lang:go foo\\.rs",
            vec![not_a_filter("lnag", "lnag:go"), keyword("foo\\.rs")],
        )]);
    }

    const SYMBOL_SELECT: &str = "type:symbol with select:symbol and no pattern \
returns nothing; /./ matches every symbol";
    fn symbol_select_as_text(mode: &str) -> String {
        format!(
            "type:symbol with select:symbol and no pattern returns nothing; with \
         patterntype:regexp in place of patterntype:{mode}, /./ matches every \
         symbol"
        )
    }

    const SYMBOLS_OF_CONTENT: &str = "select:symbol over a content search returns nothing \
on sourcegraph.com; type:symbol searches symbols";

    #[test]
    fn a_symbol_select_of_a_content_search_is_noted() {
        run(vec![
            ("repo:x@v1 NewRouter select:symbol", vec![SYMBOLS_OF_CONTENT.to_owned()]),
            ("repo:x NewRouter select:symbol.function", vec![SYMBOLS_OF_CONTENT.to_owned()]),
            ("repo:x type:file NewRouter Select:'symbol'", vec![SYMBOLS_OF_CONTENT.to_owned()]),
            ("repo:x type:symbol NewRouter select:symbol", vec![]),
            ("repo:x NewRouter select:file", vec![]),
            ("repo:x NewRouter -select:symbol", vec![]),
            ("repo:x type:commit NewRouter select:symbol", vec![]),
        ]);
    }

    #[test]
    fn a_symbol_select_without_a_pattern_is_noted() {
        run(vec![
            (
                "repo:^github\\.com/gorilla/mux$ type:symbol file:README\\.md$ select:symbol.class",
                vec![SYMBOL_SELECT.to_owned()],
            ),
            (
                "repo:^github\\.com/gorilla/mux$@v1.8.0 type:symbol file:^mux\\.go$ select:symbol.method",
                vec![SYMBOL_SELECT.to_owned()],
            ),
            (
                "repo:^github\\.com/gorilla/mux$ type:symbol file:^mux\\.go$ select:symbol.method",
                vec![SYMBOL_SELECT.to_owned()],
            ),
            ("repo:x type:symbol select:symbol", vec![SYMBOL_SELECT.to_owned()]),
            ("repo:x TYPE:symbol Select:symbol.class", vec![SYMBOL_SELECT.to_owned()]),
            ("repo:x type:\"symbol\" select:'symbol.class'", vec![SYMBOL_SELECT.to_owned()]),
            ("repo:x (type:symbol select:symbol.class)", vec![SYMBOL_SELECT.to_owned()]),
            (
                "repo:^github\\.com/gorilla/mux$ type:symbol /./ file:README\\.md$ select:symbol.class",
                vec![],
            ),
            ("repo:^github\\.com/gorilla/mux$ type:symbol NewRouter select:symbol.class", vec![]),
            ("repo:^github\\.com/gorilla/mux$ type:symbol \"New\" select:symbol.class", vec![]),
            (
                "repo:^github\\.com/gorilla/mux$ type:symbol file:README\\.md$ -select:symbol.class",
                vec![],
            ),
            (
                "repo:^github\\.com/gorilla/mux$ -type:symbol file:README\\.md$ select:symbol.class",
                vec![],
            ),
            ("repo:x type:symbol file:x", vec![]),
            ("repo:x type:symbol (file:x or NewRouter) select:symbol", vec![]),
            ("repo:x type:file select:symbol.class", vec![]),
            ("repo:x type:symbol select:file", vec![]),
            // content: is the pattern
            (
                "repo:^github\\.com/gorilla/mux$ type:symbol content:Router select:symbol.struct",
                vec![],
            ),
            ("repo:x type:symbol content:foo select:symbol", vec![]),
            ("repo:x type:symbol CONTENT:foo select:symbol", vec![]),
            // the server rejects a negated content: (live: the search fails)
            ("repo:x type:symbol -content:foo select:symbol", vec![]),
            // a content predicate on the path is the pattern
            (
                "repo:^github\\.com/gorilla/mux$ type:symbol file:has.content(Router) select:symbol.struct",
                vec![],
            ),
            ("repo:x type:symbol f:contains.content(x) select:symbol", vec![]),
            // the server rejects a negated predicate
            ("repo:x type:symbol -file:has.content(x) select:symbol", vec![]),
            // other path predicates, and names that aren't predicates, aren't
            (
                "repo:x type:symbol file:has.owner(alice) select:symbol",
                vec![SYMBOL_SELECT.to_owned()],
            ),
            (
                "repo:x type:symbol path:has.contributor(bob) select:symbol",
                vec![SYMBOL_SELECT.to_owned()],
            ),
            (
                "repo:x type:symbol file:HAS.CONTENT(x) select:symbol",
                vec![SYMBOL_SELECT.to_owned()],
            ),
            ("repo:x type:symbol file:has.content select:symbol", vec![SYMBOL_SELECT.to_owned()]),
            // an unknown field is searched as text, negated or not
            ("repo:x type:symbol setObject:forKey select:symbol.method", vec![]),
            ("repo:x type:symbol -setObject:forKey select:symbol.method", vec![]),
            // the server errors rather than answering zero
            ("repo:x type:symbol file:x not zzqq select:symbol.method", vec![]),
        ]);
    }

    #[test]
    fn the_symbol_select_note_names_the_remedy_for_the_pattern_type() {
        run(vec![
            // /./ is a regex in keyword, standard and regexp mode, and text in
            // literal and structural mode
            ("repo:x type:symbol select:symbol patterntype:regexp", vec![SYMBOL_SELECT.to_owned()]),
            (
                "repo:x type:symbol select:symbol patterntype:standard",
                vec![SYMBOL_SELECT.to_owned()],
            ),
            (
                "repo:x type:symbol select:symbol patterntype:literal",
                vec![symbol_select_as_text("literal")],
            ),
            (
                "repo:x type:symbol select:symbol patterntype:structural",
                vec![symbol_select_as_text("structural")],
            ),
            (
                "repo:x type:symbol select:symbol PatternType:literal",
                vec![symbol_select_as_text("literal")],
            ),
        ]);
    }

    const CASE: &str = "it ignores case by default";

    fn flag(term: &str, rest: &str) -> String {
        format!("the server searches `{term}` as text; {rest}")
    }

    /// The trailing-flag note waits for the answer, so no query check prints it.
    fn flags(found: Vec<(&str, String)>, silent: Vec<&str>) {
        let rows = found
            .into_iter()
            .map(|(query, note)| (query, Some(note)))
            .chain(silent.into_iter().map(|query| (query, None)));
        for (query, expected) in rows {
            let facts = Query::new(query);
            assert_eq!(facts.flag_note, expected, "{query:?}");
            assert_eq!(facts.notes, Vec::<String>::new(), "{query:?}");
        }
    }

    #[test]
    fn a_flag_after_the_closing_slash_is_noted() {
        let s = "flags go inside: /(?s).../";
        flags(
            vec![
                (
                    r"repo:^github\.com/publicsuffix/list$ file:^public_suffix_list\.dat$ /cloudflare/i index:no count:all timeout:60s",
                    flag("/cloudflare/i", CASE),
                ),
                (
                    "repo:^github\\.com/rails/rails$@7-0-stable file:^activerecord/ /trilogy/i count:all",
                    flag("/trilogy/i", CASE),
                ),
                ("lang:javascript /msie/i", flag("/msie/i", CASE)),
                ("/RE/i", flag("/RE/i", CASE)),
                ("/RE/s", flag("/RE/s", s)),
                ("/RE/m", flag("/RE/m", "flags go inside: /(?m).../")),
                ("/RE/g", flag("/RE/g", CASE)),
                ("/RE/gi", flag("/RE/gi", CASE)),
                ("/RE/ig", flag("/RE/ig", CASE)),
                ("/RE/gm", flag("/RE/gm", "flags go inside: /(?m).../")),
                ("/RE/gim", flag("/RE/gim", &format!("{CASE}, and flags go inside: /(?m).../"))),
                (r"/a\/b/i", flag(r"/a\/b/i", CASE)),
                ("/foo(bar)/i", flag("/foo(bar)/i", CASE)),
                (r"/foo\.bar/i", flag(r"/foo\.bar/i", CASE)),
                ("patterntype:standard /foo/i", flag("/foo/i", CASE)),
                ("repo:x (/foo/i or bar)", flag("/foo/i", CASE)),
                ("repo:x not /foo/i", flag("/foo/i", CASE)),
                ("/foo/i /bar/s", flag("/foo/i", CASE)),
                ("case:yes /foo/i", flag("/foo/i", "case:yes matches case even with (?i)")),
                (
                    "CASE:yes /foo/gim",
                    flag(
                        "/foo/gim",
                        "case:yes matches case even with (?i), and flags go inside: /(?m).../",
                    ),
                ),
                ("case:yes /foo/s", flag("/foo/s", s)),
                ("-case:yes /foo/i", flag("/foo/i", CASE)),
                ("case:no /foo/i", flag("/foo/i", CASE)),
            ],
            vec![
                "/usr/lib/i",
                "/usr/i",
                "/proc/s",
                "/dev/sg",
                "/etc/m",
                "/tmp/x",
                "https://example.com/i",
                "//cdn.example.com/i",
                "a/b/c",
                "a/b/i",
                "//i",
                "/foo/bar",
                "/foo/",
                "/a/b/",
                "/foo/ i",
                "\"/foo/i\"",
                "file:/foo/i",
                "content:\"/foo/i\"",
                // tails that aren't a flag set the check takes
                "/foo/x",
                "/foo/u",
                "/foo/I",
                "/foo/ii",
                "/foo/gg",
                // modes where /.../ is not a regex
                "patterntype:regexp /foo/i",
                "patterntype:literal /foo/i",
                "patterntype:structural file:^a\\.go$ lang:go /foo/i",
            ],
        );
    }

    #[test]
    fn any_set_of_flags_after_the_closing_slash_is_noted() {
        let s = "flags go inside: /(?s).../";
        flags(
            vec![
                ("/RE/is", flag("/RE/is", &format!("{CASE}, and {s}"))),
                ("/RE/si", flag("/RE/si", &format!("{CASE}, and {s}"))),
                ("/RE/im", flag("/RE/im", &format!("{CASE}, and flags go inside: /(?m).../"))),
                ("/RE/sm", flag("/RE/sm", "flags go inside: /(?sm).../")),
                ("/RE/ms", flag("/RE/ms", "flags go inside: /(?sm).../")),
                ("/RE/gs", flag("/RE/gs", s)),
                ("/RE/gims", flag("/RE/gims", &format!("{CASE}, and flags go inside: /(?sm).../"))),
                ("/RE/smig", flag("/RE/smig", &format!("{CASE}, and flags go inside: /(?sm).../"))),
            ],
            vec!["/foo/isx", "/foo/ii", "/foo/gimss", "/foo/sms"],
        );
    }

    fn nested(depth: usize, inner: &str) -> String {
        format!("{}{inner}{}", "(".repeat(depth), ")".repeat(depth))
    }

    #[test]
    fn groups_are_read_at_any_depth() {
        let exact = r"repo:^github\.com/gorilla/mux$ foo";
        let query = |depth| format!("{} select:repo", nested(depth, exact));
        assert!(!Query::new(&query(64)).lower_bound);
        assert!(!Query::new(&query(65)).lower_bound);
        assert!(!Query::new(&query(100_000)).lower_bound);
    }

    #[test]
    fn unbalanced_parentheses_are_read_in_linear_time() {
        let query = format!("{}foo", "(".repeat(120_000));
        let started = std::time::Instant::now();
        let escaped = format!("{}foo", "\\(".repeat(60_000));
        assert_eq!(Query::new(&escaped).notes, Vec::<String>::new());
        assert_eq!(Query::new(&query).notes, Vec::<String>::new());
        let elapsed = started.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
    }
}
