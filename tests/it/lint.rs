use sego::internal::search::lint::{Filter, Op, Token, tokenize};

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

#[test]
fn tokens() {
    let rows: Vec<(&str, Vec<Token>)> = vec![
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
        ("/a b/", vec![pattern("/a b/")]),
        ("/a.*b/", vec![pattern("/a.*b/")]),
        ("/a or b/", vec![pattern("/a or b/")]),
        ("/a/b/", vec![pattern("/a/b/")]),
        ("/a/b c", vec![term("/a/b"), term("c")]),
        ("/a/b/ c", vec![pattern("/a/b/"), term("c")]),
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
    ];
    for (query, expected) in rows {
        assert_eq!(tokenize(query), expected, "{query:?}");
    }
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
