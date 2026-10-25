use crate::search::lint::{Filter, Op, Token, tokenize};
use crate::search::query::prepare;

fn term(text: &str) -> Token {
    Token::Term(text.to_owned())
}

fn quoted(text: &str) -> Token {
    Token::Quoted(text.to_owned())
}

fn pattern(text: &str) -> Token {
    Token::Pattern(text.to_owned())
}

fn filter(field: &str, value: &str) -> Token {
    Token::Filter(Filter {
        negated: false,
        field: field.to_owned(),
        value: value.to_owned(),
    })
}

fn not_filter(field: &str, value: &str) -> Token {
    Token::Filter(Filter {
        negated: true,
        field: field.to_owned(),
        value: value.to_owned(),
    })
}

fn check(rows: Vec<(&str, Vec<Token>)>) {
    for (query, expected) in rows {
        assert_eq!(tokenize(query), expected, "{query:?}");
    }
}

#[test]
fn groups_and_quotes() {
    check(vec![
        ("foo bar", vec![term("foo"), term("bar")]),
        ("", vec![]),
        ("   ", vec![]),
        ("foo(\"a b\")", vec![term("foo(\"a b\")")]),
        ("foo()", vec![term("foo()")]),
        ("fmt.Println(x y)", vec![term("fmt.Println(x y)")]),
        ("foo(a b) c", vec![term("foo(a b)"), term("c")]),
        ("foo(a b", vec![term("foo(a"), term("b")]),
        (
            "(foo bar)",
            vec![Token::Open, term("foo"), term("bar"), Token::Close],
        ),
        ("(foo", vec![term("(foo")]),
        ("(a b c", vec![term("(a"), term("b"), term("c")]),
        ("((foo)", vec![term("((foo)")]),
        ("bar)", vec![term("bar)")]),
        ("foo())", vec![term("foo())")]),
        (
            "((a))",
            vec![
                Token::Open,
                Token::Open,
                term("a"),
                Token::Close,
                Token::Close,
            ],
        ),
        ("file:(^|/)x", vec![filter("file", "(^|/)x")]),
        ("file:(^|/)x foo", vec![filter("file", "(^|/)x"), term("foo")]),
        ("\"not found\" foo", vec![quoted("\"not found\""), term("foo")]),
        ("'a b'", vec![quoted("'a b'")]),
        ("\"a \\\" b\"", vec![quoted("\"a \\\" b\"")]),
        ("don't stop", vec![term("don't"), term("stop")]),
        ("'don't' stop", vec![term("'don't'"), term("stop")]),
        ("foo\"a b\"", vec![term("foo\"a"), term("b\"")]),
        ("foo \"a b", vec![term("foo"), term("\"a"), term("b")]),
        ("\"a b\"c", vec![term("\"a"), term("b\"c")]),
        ("(\"a b\")", vec![Token::Open, quoted("\"a b\""), Token::Close]),
        ("\"\"", vec![quoted("\"\"")]),
    ]);
}

#[test]
fn patterns_filters_and_operators() {
    check(vec![
        ("/a b/", vec![pattern("/a b/")]),
        ("/a.*b/", vec![pattern("/a.*b/")]),
        ("/a or b/", vec![pattern("/a or b/")]),
        ("/a/b/", vec![term("/a/b/")]),
        ("/src/.*\\.go/", vec![term("/src/.*\\.go/")]),
        ("//", vec![term("//")]),
        ("/a\\/ b/", vec![pattern("/a\\/ b/")]),
        ("/a/b c", vec![term("/a/b"), term("c")]),
        ("/a/ c", vec![pattern("/a/"), term("c")]),
        ("/a/b/ c", vec![term("/a/b/"), term("c")]),
        (
            "/func \\(r \\*Router\\) \\w+\\(/ count:all",
            vec![
                pattern("/func \\(r \\*Router\\) \\w+\\(/"),
                filter("count", "all"),
            ],
        ),
        ("(/a b/)", vec![Token::Open, pattern("/a b/"), Token::Close]),
        ("/open", vec![term("/open")]),
        (
            "repo:contains.file(path:a content:b) x",
            vec![filter("repo", "contains.file(path:a content:b)"), term("x")],
        ),
        ("content:\"a b\"", vec![filter("content", "\"a b\"")]),
        (
            "(content:\"a b\" x)",
            vec![
                Token::Open,
                filter("content", "\"a b\""),
                term("x"),
                Token::Close,
            ],
        ),
        (
            "repo:\"a b\"x y",
            vec![filter("repo", "\"a b\""), term("x"), term("y")],
        ),
        ("repo:\"a b", vec![filter("repo", "\"a"), term("b")]),
        ("'a \\' b'", vec![quoted("'a \\' b'")]),
        (
            "(content:\"a b\")",
            vec![Token::Open, filter("content", "\"a b\""), Token::Close],
        ),
        ("a ) b", vec![term("a"), term(")"), term("b")]),
        ("()", vec![term("()")]),
        ("( )", vec![term("()")]),
        ("a () b", vec![term("a"), term("()"), term("b")]),
        ("-file:_test\\.go", vec![not_filter("file", "_test\\.go")]),
        ("std::mem", vec![term("std::mem")]),
        ("TODO:", vec![term("TODO:")]),
        ("-x:", vec![term("-x:")]),
        ("a1:b", vec![term("a1:b")]),
        ("https://x", vec![filter("https", "//x")]),
        ("NOT x", vec![Token::Op(Op::Not), term("x")]),
        ("Or", vec![Token::Op(Op::Or)]),
        (
            "a and b OR c",
            vec![
                term("a"),
                Token::Op(Op::And),
                term("b"),
                Token::Op(Op::Or),
                term("c"),
            ],
        ),
        ("\"or\"", vec![quoted("\"or\"")]),
        (
            "(repo:x a)",
            vec![Token::Open, filter("repo", "x"), term("a"), Token::Close],
        ),
    ]);
}

#[test]
fn raw_gives_the_token_as_written() {
    let rows = [
        (term("a b"), "a b"),
        (quoted("\"a\""), "\"a\""),
        (pattern("/a/"), "/a/"),
        (filter("file", "x"), "file:x"),
        (not_filter("file", "x"), "-file:x"),
        (Token::Open, "("),
        (Token::Close, ")"),
        (Token::Op(Op::Or), "or"),
    ];
    for (token, raw) in rows {
        assert_eq!(token.raw(), raw);
    }
}

fn not_a_filter(field: &str, token: &str) -> String {
    format!(
        "'{field}:' is not a filter; the server searches `{token}` as text"
    )
}

const INTERSECT: &str = "lang: filters intersect: a file must match both";
const ESCAPES_OR: &str = "filters outside parentheses apply only to the \
operand of OR they sit in; the other operands search every repository";
const PATH_ONLY: &str = "type:path with only filters returns nothing";

fn keyword(term: &str) -> String {
    format!(
        "keyword mode matches `{term}` literally; a regex goes inside /.../"
    )
}

fn bad_type(value: &str) -> String {
    format!("type:{value} is not a result type; the server returns nothing")
}

fn run(rows: Vec<(&str, Vec<String>)>) {
    for (query, expected) in rows {
        assert_eq!(prepare(query).notes, expected, "{query:?}");
    }
}

#[test]
fn checks_filters_and_keywords() {
    run(vec![
        // 1: fire
        ("fiel:x", vec![not_a_filter("fiel", "fiel:x")]),
        ("langauge:go", vec![not_a_filter("langauge", "langauge:go")]),
        ("ext:go", vec![not_a_filter("ext", "ext:go")]),
        ("-lnag:go", vec![not_a_filter("lnag", "-lnag:go")]),
        (
            "fiel:x fiel:x fiel:y",
            vec![
                not_a_filter("fiel", "fiel:x"),
                not_a_filter("fiel", "fiel:y"),
            ],
        ),
        // 1: don't fire
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
        // 2
        ("lang:go lang:rust", vec![INTERSECT.to_owned()]),
        ("repo:a or repo:b", vec![]),
        ("file:^mux file:\\.go$", vec![]),
        ("repo:^github\\.com/gorilla/ repo:mux$", vec![]),
        ("lang:go lang:Go", vec![]),
        // 3: fire
        ("\\bfoo\\b", vec![keyword("\\bfoo\\b")]),
        ("[a-zA-Z]", vec![keyword("[a-zA-Z]")]),
        ("[0-9]+", vec![keyword("[0-9]+")]),
        ("file\\.go", vec![keyword("file\\.go")]),
        ("fmt\\.Errorf", vec![keyword("fmt\\.Errorf")]),
        ("a.*b", vec![keyword("a.*b")]),
        ("^foo", vec![keyword("^foo")]),
        // 3: don't fire
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
        (
            "repo:x patterntype:literal foo (bar or baz) [a-z0-9]+",
            vec![keyword("[a-z0-9]+")],
        ),
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
fn checks_or_types_and_revisions() {
    run(vec![
        // 4: fire
        ("repo:x file:mux.go or file:route.go", vec![ESCAPES_OR.to_owned()]),
        ("repo:x file:y a or (b and c)", vec![ESCAPES_OR.to_owned()]),
        ("repo:x (a case:yes) or (b case:no)", vec![ESCAPES_OR.to_owned()]),
        ("repo:x router or lang:markdown", vec![ESCAPES_OR.to_owned()]),
        ("foo or bar repo:x", vec![ESCAPES_OR.to_owned()]),
        ("a repo:x or b", vec![ESCAPES_OR.to_owned()]),
        // 4: don't fire
        ("repo:x foo or bar", vec![]),
        ("repo:x a or b lang:go", vec![]),
        ("repo:x a or b or c file:y", vec![]),
        ("repo:a x or repo:b y", vec![]),
        ("repo:x file:y (a or (b and c))", vec![]),
        ("(repo:x or repo:y) (\"a\" or \"b\")", vec![]),
        ("repo:a x or (repo:b y)", vec![]),
        ("repo:a x or -repo:b y", vec![]),
        ("a -repo:x or b", vec![]),
        // 4: the shared-filter rule
        ("repo:x not file:y foo or bar", vec![]),
        ("repo:x -file:y foo or bar", vec![]),
        ("repo:x foo or bar file:y", vec![]),
        ("repo:x file:y (a and b) or c", vec![ESCAPES_OR.to_owned()]),
        ("repo:x (foo bar) or baz", vec![ESCAPES_OR.to_owned()]),
        ("repo:x foo or (bar baz)", vec![ESCAPES_OR.to_owned()]),
        // a leading space, other modes, and parentheses around the query
        (" foo or bar repo:x", vec![]),
        (" repo:x foo or bar", vec![]),
        ("repo:x patterntype:regexp (foo bar) or baz", vec![]),
        (
            "repo:x patterntype:regexp (foo and bar) or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        ("repo:x patterntype:literal (foo bar) or baz", vec![]),
        ("(repo:x foo or bar)", vec![]),
        ("(foo or bar repo:x)", vec![]),
        ("(foo or repo:x bar)", vec![ESCAPES_OR.to_owned()]),
        ("repo:x (a or file:y b) c", vec![]),
        ("repo:x (foo or bar) file:y", vec![]),
        // 5
        ("type:File", vec![bad_type("File")]),
        ("type:structural", vec![]),
        ("-type:bogus", vec![]),
        ("type:\"file\"", vec![]),
        // 6
        ("repo:x type:path", vec![PATH_ONLY.to_owned()]),
        ("type:path NewRouter", vec![]),
        ("type:path file:\\.go$", vec![]),
        ("type:path lang:go", vec![]),
        ("type:path -file:_test", vec![]),
        // 7
        (
            "repo:^github\\.com/gorilla/mux$@v1.8.0..v1.7.4",
            vec![
                "repo:^github\\.com/gorilla/mux$@v1.8.0..v1.7.4 searches \
                 v1.7.4 only"
                    .to_owned(),
            ],
        ),
        ("repo:x@a:b...c", vec!["repo:x@a:b...c searches c only".to_owned()]),
        (
            "repo:x@a..b repo:x@a..b",
            vec!["repo:x@a..b searches b only".to_owned()],
        ),
        ("repo:x@v1.8.0", vec![]),
        ("repo:x@a..", vec![]),
    ]);
}

#[test]
fn checks_or_text_in_parentheses() {
    run(vec![
        // quoted and slash text inside parentheses, read as the server does
        (
            "repo:x patterntype:regexp (\"a or b\") or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:standard (\"a or b\") or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:literal (\"a or b\") or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:regexp ('a or b') or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:standard ('a or b') or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:literal ('a or b') or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:regexp (\"foo repo:y\") or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:regexp (foo \"a and b\") or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:standard (\"x not y\") or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:standard (\"foo)\" bar) or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:regexp foo or (\"a or b\")",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:regexp (/a or b/) or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        (
            "repo:x patterntype:standard (/a or b/) or baz",
            vec![ESCAPES_OR.to_owned()],
        ),
        ("repo:x patterntype:regexp (foo or) or baz", vec![]),
        ("repo:x patterntype:regexp (foo and) or baz", vec![]),
        ("repo:x patterntype:regexp (foo\\ or bar) or baz", vec![]),
        ("repo:x (foo or) or baz", vec![ESCAPES_OR.to_owned()]),
    ]);
}

#[test]
fn checks_several_at_once() {
    run(vec![(
        "lnag:go lang:go lang:rust foo\\.rs",
        vec![
            not_a_filter("lnag", "lnag:go"),
            INTERSECT.to_owned(),
            keyword("foo\\.rs"),
        ],
    )]);
}

const STRUCTURAL_SCOPE: &str = "a structural search over more than one file \
can miss matches without a +; file:^PATH$ makes it complete, and a regex \
search marks a cut with +";
const STRUCTURAL_LANGUAGE: &str = "with no lang: and no file: extension, a \
structural search can parse files as the wrong language and miss matches; \
lang: names the language";

#[test]
fn checks_structural_scope() {
    let both =
        || vec![STRUCTURAL_SCOPE.to_owned(), STRUCTURAL_LANGUAGE.to_owned()];
    let scope = || vec![STRUCTURAL_SCOPE.to_owned()];
    let language = || vec![STRUCTURAL_LANGUAGE.to_owned()];
    run(vec![
        ("repo:x patterntype:structural foo(...)", both()),
        ("repo:x file:^src/ patterntype:structural foo(...)", both()),
        ("repo:x lang:go patterntype:structural foo(...)", scope()),
        ("repo:x file:\\.go$ patterntype:structural foo(...)", scope()),
        ("repo:x file:^pkg/a\\.go patterntype:structural foo(...)", scope()),
        ("repo:x file:a.go patterntype:structural foo(...)", scope()),
        (
            "repo:x file:^pkg/(a|b)\\.go$ patterntype:structural foo(...)",
            scope(),
        ),
        ("repo:x file:^pkg/.*\\.go$ patterntype:structural foo(...)", scope()),
        (
            "repo:x file:^src/a\\.go$|^b\\.go$ patterntype:structural foo(...)",
            scope(),
        ),
        ("repo:x file:^Makefile$ patterntype:structural foo(...)", language()),
        ("repo:x file:^pkg/a\\.go$ patterntype:structural foo(...)", vec![]),
        ("repo:x file:\"^pkg/a.go$\" patterntype:structural foo(...)", vec![]),
        (
            "repo:x -file:^src/ file:^pkg/a\\.go$ patterntype:structural foo(...)",
            vec![],
        ),
        (
            "repo:x lang:go file:^Makefile$ patterntype:structural foo(...)",
            vec![],
        ),
        ("repo:x foo(...)", vec![]),
        ("repo:x patterntype:regexp foo", vec![]),
        ("repo:x patterntype:structural patterntype:keyword foo", vec![]),
        // stated rules
        (
            "repo:x -lang:go file:^Makefile$ patterntype:structural foo(...)",
            language(),
        ),
        (
            "repo:x file:^pkg/ file:\\.go$ patterntype:structural foo(...)",
            both(),
        ),
        (
            "repo:x file:\\.go$ file:^pkg/ patterntype:structural foo(...)",
            scope(),
        ),
        ("repo:x file:^pkg/$ patterntype:structural foo(...)", both()),
        ("repo:x file:^$ patterntype:structural foo(...)", both()),
        ("repo:x file:^a\\+\\.go$ patterntype:structural foo(...)", vec![]),
        ("repo:x path:^pkg/a\\.go$ PatternType:structural foo(...)", vec![]),
        ("repo:x f:^pkg/a\\.go$ patterntype:structural foo(...)", vec![]),
        // class escapes are patterns
        ("repo:x file:^a\\d\\.go$ patterntype:structural foo(...)", scope()),
        ("repo:x file:^\\w+\\.go$ patterntype:structural foo(...)", scope()),
        // predicates aren't paths
        (
            "repo:x file:has.content(x) file:\\.go$ patterntype:structural foo(...)",
            scope(),
        ),
        (
            "repo:x file:contains.content(x) file:^pkg/a\\.go$ patterntype:structural foo(...)",
            vec![],
        ),
        // the server's extensions are lower-case
        ("repo:x file:\\.GO$ patterntype:structural foo(...)", both()),
    ]);
}

#[test]
fn checks_structural_branches() {
    let both =
        || vec![STRUCTURAL_SCOPE.to_owned(), STRUCTURAL_LANGUAGE.to_owned()];
    run(vec![
        // each search of the plan, with its own filters
        ("patterntype:structural file:^a\\.go$", vec![]),
        // parentheses as the server parses them, per mode and around the query
        ("file:^a\\.go$ patterntype:structural (:[x]) or bar(...)", vec![]),
        (
            "file:^a\\.go$ patterntype:structural (foo(...) bar(...)) or baz(...)",
            vec![],
        ),
        (
            "repo:x patterntype:structural (foo(...) bar(...)) or baz(...)",
            vec![STRUCTURAL_SCOPE.to_owned(), STRUCTURAL_LANGUAGE.to_owned()],
        ),
        (
            "repo:x patterntype:structural (foo(...) and bar(...)) or baz(...)",
            vec![
                ESCAPES_OR.to_owned(),
                STRUCTURAL_SCOPE.to_owned(),
                STRUCTURAL_LANGUAGE.to_owned(),
            ],
        ),
        (
            "repo:x patterntype:structural (foo(...) x:y) or baz(...)",
            vec![STRUCTURAL_SCOPE.to_owned(), STRUCTURAL_LANGUAGE.to_owned()],
        ),
        (
            "repo:x patterntype:structural (foo(...)) bar(...) or baz(...)",
            vec![STRUCTURAL_SCOPE.to_owned(), STRUCTURAL_LANGUAGE.to_owned()],
        ),
        (
            "(file:^a\\.go$ foo(...) or bar(...)) patterntype:structural",
            vec![],
        ),
        (
            "(file:^a\\.go$ patterntype:structural foo(...) or bar(...))",
            vec![],
        ),
        (
            "((file:^a\\.go$ patterntype:structural foo(...) or bar(...)))",
            vec![],
        ),
        (
            "patterntype:structural ((file:^a\\.go$ foo(...) or bar(...)))",
            vec![],
        ),
        (
            "repo:x file:^a\\.go$ patterntype:structural ((foo(...) or bar(...)))",
            vec![],
        ),
        (
            "  foo(...) or bar(...) file:^a\\.go$ patterntype:structural",
            vec![],
        ),
        (
            "repo:x patterntype:structural (file:^a\\.go$ foo(...) or bar(...))",
            vec![],
        ),
        (
            "repo:x patterntype:structural (file:^a\\.go$ foo(...) or file:^b\\.go$ bar(...))",
            vec![],
        ),
        (
            "repo:x file:^a\\.go$ patterntype:structural (foo(...) or bar(...))",
            vec![],
        ),
        (
            "repo:x patterntype:structural (foo(...) or bar(...)) file:^a\\.go$",
            vec![],
        ),
        (
            "repo:x file:^a\\.go$ patterntype:structural foo(...) or bar(...)",
            vec![],
        ),
        (
            "repo:x not file:y file:^a\\.go$ patterntype:structural foo(...) or bar(...)",
            vec![],
        ),
        ("foo(...) or bar(...) file:^a\\.go$ patterntype:structural", both()),
        (
            "file:^a\\.go$ patterntype:structural (foo(...) and bar(...)) or baz(...)",
            both(),
        ),
        ("patterntype:structural foo(...) or file:^a\\.go$ bar(...)", both()),
        (
            "file:^a\\.go$ patterntype:structural foo(...) or (lang:go bar(...)) or baz(...)",
            both(),
        ),
        // lang: aliases, and not before a filter
        (
            "repo:x l:go file:^Makefile$ patterntype:structural foo(...)",
            vec![],
        ),
        (
            "repo:x language:go file:^Makefile$ patterntype:structural foo(...)",
            vec![],
        ),
        ("repo:x patterntype:structural foo(...) not file:^a\\.go$", both()),
        // the server rejects structural search with type:
        ("repo:x type:file patterntype:structural foo(...)", vec![]),
        ("repo:x -type:diff patterntype:structural foo(...)", vec![]),
    ]);
}

#[test]
fn checks_structural_groups() {
    let both =
        || vec![STRUCTURAL_SCOPE.to_owned(), STRUCTURAL_LANGUAGE.to_owned()];
    run(vec![
        // quoted text, an empty field and deep parentheses inside a group
        (
            "file:^a\\.go$ patterntype:structural (\"a or b\") or bar(...)",
            both(),
        ),
        (
            "file:^a\\.go$ patterntype:structural (foo(...) \"x or y\") or bar(...)",
            both(),
        ),
        ("patterntype:structural (foo(...) repo:) or baz(...)", both()),
        (
            "file:^a\\.go$ patterntype:structural (foo(...) repo:) or baz(...)",
            both(),
        ),
        (
            "repo:x patterntype:structural (foo(...) repo:) or baz(...)",
            vec![
                ESCAPES_OR.to_owned(),
                STRUCTURAL_SCOPE.to_owned(),
                STRUCTURAL_LANGUAGE.to_owned(),
            ],
        ),
        (
            "patterntype:structural repo:x (((file:^a\\.go$ foo(...) or bar(...))))",
            vec![],
        ),
        // a stray ) makes the server search in keyword mode
        ("patterntype:structural (\"foo)\" bar(...)) or baz(...)", vec![]),
        (
            "repo:x file:^a\\.go$ patterntype:structural (\"foo)\" bar(...)) or baz(...)",
            vec![ESCAPES_OR.to_owned()],
        ),
    ]);
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

#[test]
fn checks_symbol_select_without_pattern() {
    run(vec![
        // fire
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
        (
            "repo:x TYPE:symbol Select:symbol.class",
            vec![SYMBOL_SELECT.to_owned()],
        ),
        (
            "repo:x type:\"symbol\" select:'symbol.class'",
            vec![SYMBOL_SELECT.to_owned()],
        ),
        (
            "repo:x (type:symbol select:symbol.class)",
            vec![SYMBOL_SELECT.to_owned()],
        ),
        // don't fire
        (
            "repo:^github\\.com/gorilla/mux$ type:symbol /./ file:README\\.md$ select:symbol.class",
            vec![],
        ),
        (
            "repo:^github\\.com/gorilla/mux$ type:symbol NewRouter select:symbol.class",
            vec![],
        ),
        (
            "repo:^github\\.com/gorilla/mux$ type:symbol \"New\" select:symbol.class",
            vec![],
        ),
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
        (
            "repo:x type:symbol file:has.content select:symbol",
            vec![SYMBOL_SELECT.to_owned()],
        ),
        // an unknown field is searched as text, negated or not
        ("repo:x type:symbol setObject:forKey select:symbol.method", vec![]),
        ("repo:x type:symbol -setObject:forKey select:symbol.method", vec![]),
        // the server errors rather than answering zero
        ("repo:x type:symbol file:x not zzqq select:symbol.method", vec![]),
    ]);
}

#[test]
fn checks_symbol_select_remedy_by_mode() {
    run(vec![
        // /./ is a regex in keyword, standard and regexp mode, and text in
        // literal and structural mode
        (
            "repo:x type:symbol select:symbol patterntype:regexp",
            vec![SYMBOL_SELECT.to_owned()],
        ),
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
        let prepared = prepare(query);
        assert_eq!(prepared.flag_note, expected, "{query:?}");
        assert_eq!(prepared.notes, Vec::<String>::new(), "{query:?}");
    }
}

#[test]
fn finds_a_flag_after_the_closing_slash() {
    let s = "flags go inside: /(?s).../";
    flags(
        vec![
            // found
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
            (
                "/RE/gim",
                flag(
                    "/RE/gim",
                    &format!("{CASE}, and flags go inside: /(?m).../"),
                ),
            ),
            (r"/a\/b/i", flag(r"/a\/b/i", CASE)),
            ("/foo(bar)/i", flag("/foo(bar)/i", CASE)),
            (r"/foo\.bar/i", flag(r"/foo\.bar/i", CASE)),
            ("patterntype:standard /foo/i", flag("/foo/i", CASE)),
            ("repo:x (/foo/i or bar)", flag("/foo/i", CASE)),
            ("repo:x not /foo/i", flag("/foo/i", CASE)),
            ("/foo/i /bar/s", flag("/foo/i", CASE)),
            // case:yes
            (
                "case:yes /foo/i",
                flag("/foo/i", "case:yes matches case even with (?i)"),
            ),
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
            // paths, URLs and other text
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
            // a pattern, or a pattern then a term
            "/foo/",
            "/a/b/",
            "/foo/ i",
            // quoted text and filter values
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
fn finds_any_set_of_flags() {
    let s = "flags go inside: /(?s).../";
    flags(
        vec![
            ("/RE/is", flag("/RE/is", &format!("{CASE}, and {s}"))),
            ("/RE/si", flag("/RE/si", &format!("{CASE}, and {s}"))),
            (
                "/RE/im",
                flag(
                    "/RE/im",
                    &format!("{CASE}, and flags go inside: /(?m).../"),
                ),
            ),
            ("/RE/sm", flag("/RE/sm", "flags go inside: /(?sm).../")),
            ("/RE/ms", flag("/RE/ms", "flags go inside: /(?sm).../")),
            ("/RE/gs", flag("/RE/gs", s)),
            (
                "/RE/gims",
                flag(
                    "/RE/gims",
                    &format!("{CASE}, and flags go inside: /(?sm).../"),
                ),
            ),
            (
                "/RE/smig",
                flag(
                    "/RE/smig",
                    &format!("{CASE}, and flags go inside: /(?sm).../"),
                ),
            ),
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
    assert!(!prepare(&query(64)).lower_bound);
    assert!(!prepare(&query(65)).lower_bound);
    assert!(!prepare(&query(100_000)).lower_bound);

    let structural = |depth| {
        format!("patterntype:structural {}", nested(depth, "file:y foo"))
    };
    assert_eq!(
        prepare(&structural(64)).notes,
        [STRUCTURAL_SCOPE, STRUCTURAL_LANGUAGE]
    );
    for depth in [65, 100_000] {
        assert_eq!(
            prepare(&structural(depth)).notes,
            [STRUCTURAL_SCOPE, STRUCTURAL_LANGUAGE]
        );
    }
}

#[test]
fn unbalanced_parentheses_tokenize_in_linear_time() {
    let query = format!("{}foo", "(".repeat(120_000));
    let started = std::time::Instant::now();
    let tokens = tokenize(&query);
    assert_eq!(tokens, vec![term(&query)]);
    let escaped = format!("{}foo", "\\(".repeat(60_000));
    assert_eq!(tokenize(&escaped), vec![term(&escaped)]);
    assert_eq!(prepare(&query).notes, Vec::<String>::new());
    let elapsed = started.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
}
