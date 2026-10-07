use sego::internal::search::lint::{Filter, Op, Token, tokenize};
use sego::internal::search::query::prepare;

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
        ("string|null", vec![]),
        ("Subrouter|NewRouter", vec![]),
        ("dp[i-j]", vec![]),
        ("tsconfig.*.json", vec![]),
        ("..\\.gitignore", vec![]),
        ("foo|bar", vec![]),
        ("[a-z]", vec![]),
        ("\\.go", vec![]),
        ("repo:a x or (repo:b y)", vec![]),
        ("repo:a x or -repo:b y", vec![]),
        ("a -repo:x or b", vec![]),
        ("-type:bogus", vec![]),
        ("type:\"file\"", vec![]),
        ("repo:x@a..", vec![]),
        (
            "repo:x@a..b repo:x@a..b",
            vec!["repo:x@a..b searches b only".to_owned()],
        ),
        // 3: don't fire
        ("\"\\.go\"", vec![]),
        ("/a.*b/", vec![]),
        ("file:\\.go$", vec![]),
        ("patterntype:regexp a.*b", vec![]),
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
        // 5
        ("type:File", vec![bad_type("File")]),
        ("type:structural", vec![]),
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
        ("repo:x@v1.8.0", vec![]),
        ("a.*b", vec![keyword("a.*b")]),
        ("import.*;", vec![]),
        ("^foo", vec![keyword("^foo")]),
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
