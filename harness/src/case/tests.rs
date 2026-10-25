use std::path::Path;

use serde_json::json;

use super::{
    Case, Exchange, Fault, Headers, Request, Response, Served, Stamp,
};

fn parse(text: &str) -> Case {
    Case::parse(text, Path::new("x.case")).unwrap_or_else(|e| panic!("{e}"))
}

const EXCHANGE: &str = r#"--- request Refs {"first":101,"repo0":"example.com/a/b"}
--- query
query Refs($repo0: String!) {
  r0: repository(name: $repo0) { name }
}
--- response 200 application/json
{
  "data": {
    "r0": {
      "name": "example.com/a/b"
    }
  }
}
"#;

fn with_exchange(extra: &str) -> String {
    format!("args: refs example.com/a/b\nexit: 0\n{extra}{EXCHANGE}")
}

#[test]
fn a_case_file_parses() {
    let case = parse(
        "\
# a comment
args: show 'a b'
env: SEGO_ENDPOINT=http://x
env: K=V=W
exit: 2
stdout: closed
--- stdout
one
two
--- stderr
err
",
    );
    assert_eq!(case.args, [b"show".to_vec(), b"a b".to_vec()]);
    assert_eq!(
        case.env,
        [
            ("SEGO_ENDPOINT".to_owned(), "http://x".to_owned()),
            ("K".to_owned(), "V=W".to_owned())
        ]
    );
    assert_eq!(case.exit, 2);
    assert!(case.stdout_closed);
    assert_eq!(case.stdout, "one\ntwo\n");
    assert_eq!(case.stderr, "err\n");
    assert_eq!(case.exchanges.len(), 0);
}

#[test]
fn missing_sections_are_empty_and_bad_cases_are_refused() {
    let case = parse("args: --version\nexit: 0\n");
    assert_eq!((case.stdout.as_str(), case.stderr.as_str()), ("", ""));
    assert!(!case.stdout_closed);
    for bad in [
        "exit: 0\n",
        "args: x\n",
        "args: x\nexit: nope\n",
        "args: x\nexit: 0\nwhat: 1\n",
        "args: x\nexit: 0\n--- stdin\n",
        "args: x\nexit: 0\nenv: NOEQUALS\n",
        "args: x\nexit: 0\nstdout: open\n",
        "args: 'x\nexit: 0\n",
        "args: x\nexit: 0\n--- stdout\n--- stdout\n",
        "args: x\nexit: 0\n--- query\nq\n",
        "args: x\nexit: 0\n--- response\n",
        "args: x\nexit: 0\n--- response ok\n",
        "args: x\nexit: 0\n--- request\n--- response 200\n",
        "args: x\nexit: 0\n--- request Op {oops\n--- response 200\n",
        "args: x\nexit: 0\n--- request Op\n--- query\nq\n",
    ] {
        assert!(Case::parse(bad, Path::new("x")).is_err(), "{bad:?}");
    }
}

#[test]
fn only_exact_markers_start_a_section() {
    let case = parse(
        "\
args: diff
exit: 0
--- stdout
--- a/x
+++ b/x
--- stderr
--- stdoutish
--- requested
",
    );
    assert_eq!(case.stdout, "--- a/x\n+++ b/x\n");
    assert_eq!(case.stderr, "--- stdoutish\n--- requested\n");
}

#[test]
fn an_exchange_holds_the_request_the_query_and_the_response() {
    let case = parse(&with_exchange(""));
    let [exchange] = &case.exchanges[..] else {
        panic!("{:?}", case.exchanges);
    };
    let Some(Request::Graphql { operation, variables, query }) =
        &exchange.request
    else {
        panic!("{:?}", exchange.request);
    };
    assert_eq!(operation, "Refs");
    assert_eq!(variables, &json!({"first": 101, "repo0": "example.com/a/b"}));
    assert_eq!(
        query,
        "query Refs($repo0: String!) {\n  r0: repository(name: $repo0) { name \
         }\n}\n"
    );
    assert_eq!(
        exchange.response.served,
        Served::Reply {
            status: 200,
            content_type: Some("application/json".to_owned()),
            headers: Headers::default(),
        }
    );
    assert_eq!(exchange.response.stamp, None);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&exchange.response.body)
            .unwrap_or_else(|e| panic!("{e}")),
        json!({"data": {"r0": {"name": "example.com/a/b"}}})
    );
}

#[test]
fn a_recorded_response_carries_its_stamp() {
    let text = with_exchange("").replace(
        "--- response 200 application/json\n",
        "--- response 200 application/json; charset=utf-8 recorded \
         2026-10-07 https://sourcegraph.com\n",
    );
    let case = parse(&text);
    let response = &case.exchanges[0].response;
    assert_eq!(
        response.served,
        Served::Reply {
            status: 200,
            content_type: Some("application/json; charset=utf-8".to_owned()),
            headers: Headers::default(),
        }
    );
    assert_eq!(
        response.stamp,
        Some(Stamp {
            date: "2026-10-07".to_owned(),
            endpoint: "https://sourcegraph.com".to_owned(),
            live_values: false,
        })
    );
}

#[test]
fn a_recorded_response_can_be_marked_to_compare_values_live() {
    let line = "200 application/json recorded 2026-10-07 \
                https://sourcegraph.com live: values";
    let text = with_exchange("").replace("200 application/json", line);
    let case = parse(&text);
    assert_eq!(
        case.exchanges[0].response.stamp,
        Some(Stamp {
            date: "2026-10-07".to_owned(),
            endpoint: "https://sourcegraph.com".to_owned(),
            live_values: true,
        })
    );
    assert_eq!(case.to_text(), text);
}

#[test]
fn live_values_needs_a_recording_and_nothing_else_follows_the_stamp() {
    for line in [
        "200 application/json live: values",
        "network error live: values",
        "200 recorded 2026-10-07 https://sourcegraph.com live: shape",
        "200 recorded 2026-10-07 https://sourcegraph.com live:values",
        "200 recorded 2026-10-07 https://sourcegraph.com live: values extra",
        "200 recorded 2026-10-07 https://sourcegraph.com extra",
        "200 recorded 2026-10-07 live: values",
    ] {
        let text = format!(
            "args: x\nexit: 0\n--- request GET /r\n--- response {line}\n"
        );
        assert!(Case::parse(&text, Path::new("x")).is_err(), "{line}");
    }
}

#[test]
fn the_response_line_takes_a_bare_status_and_the_transport_faults() {
    for (line, served) in [
        (
            "200 text/plain",
            Served::Reply {
                status: 200,
                content_type: Some("text/plain".to_owned()),
                headers: Headers::default(),
            },
        ),
        (
            "502",
            Served::Reply {
                status: 502,
                content_type: None,
                headers: Headers::default(),
            },
        ),
        ("network error", Served::Fault(Fault::Network)),
        ("redirect", Served::Fault(Fault::Redirect)),
        ("html challenge", Served::Fault(Fault::HtmlChallenge)),
    ] {
        let case = parse(&format!(
            "args: x\nexit: 0\n--- request GET /r\n--- response {line}\n"
        ));
        assert_eq!(case.exchanges[0].response.served, served, "{line}");
    }
}

#[test]
fn the_response_line_takes_the_headers_sego_reads() {
    for (line, status, content_type, headers) in [
        (
            "502 retry-after: 30",
            502,
            None,
            Headers {
                retry_after: Some("30".to_owned()),
                ..Headers::default()
            },
        ),
        (
            "302 location: https://sourcegraph.com/sign-in",
            302,
            None,
            Headers {
                location: Some("https://sourcegraph.com/sign-in".to_owned()),
                ..Headers::default()
            },
        ),
        (
            "403 text/html; charset=UTF-8 cf-mitigated: challenge",
            403,
            Some("text/html; charset=UTF-8"),
            Headers {
                cf_mitigated: Some("challenge".to_owned()),
                ..Headers::default()
            },
        ),
        (
            "429 application/json retry-after: 30 location: /x",
            429,
            Some("application/json"),
            Headers {
                location: Some("/x".to_owned()),
                retry_after: Some("30".to_owned()),
                cf_mitigated: None,
            },
        ),
    ] {
        let text = format!(
            "args: x\nexit: 0\n--- request GET /r\n--- response {line}\n"
        );
        let case = parse(&text);
        assert_eq!(
            case.exchanges[0].response.served,
            Served::Reply {
                status,
                content_type: content_type.map(str::to_owned),
                headers
            },
            "{line}"
        );
        assert_eq!(case.to_text(), text);
    }
}

#[test]
fn a_header_needs_a_value_and_cannot_repeat_or_follow_a_fault() {
    for line in [
        "502 retry-after:",
        "502 retry-after: 30 retry-after: 60",
        "502 retry-after: 30 extra",
        "502 retry-after: 30 text/plain",
        "network error retry-after: 30",
        "redirect location: /x",
        "502 retry-after: 30 recorded 2026-10-07 https://sourcegraph.com",
    ] {
        let text = format!(
            "args: x\nexit: 0\n--- request GET /r\n--- response {line}\n"
        );
        assert!(Case::parse(&text, Path::new("x")).is_err(), "{line}");
    }
}

#[test]
fn a_response_is_written_with_its_headers_after_its_content_type() {
    let mut case = parse("args: x\nexit: 0\n");
    let exchange = Exchange {
        request: None,
        response: Response {
            served: Served::Reply {
                status: 429,
                content_type: Some("text/html".to_owned()),
                headers: Headers {
                    location: Some("/x".to_owned()),
                    retry_after: Some("30".to_owned()),
                    cf_mitigated: Some("challenge".to_owned()),
                },
            },
            stamp: None,
            body: String::new(),
        },
    };
    case.set_exchange(0, &exchange).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        "args: x\nexit: 0\n--- response 429 text/html location: /x \
         retry-after: 30 cf-mitigated: challenge\n"
    );
    assert_eq!(case.exchanges, [exchange]);
}

#[test]
fn a_fault_cannot_be_recorded() {
    let text = "args: x\nexit: 0\n--- request GET /r\n--- response network \
                error recorded 2026-10-07 https://sourcegraph.com\n";
    assert!(Case::parse(text, Path::new("x")).is_err());
}

#[test]
fn a_raw_read_is_a_request_with_a_method_and_a_path() {
    let case = parse(
        "args: x\nexit: 0\n--- request GET /github.com/a/b/-/raw/c\n--- \
         response 200 text/plain\n",
    );
    assert_eq!(
        case.exchanges[0].request,
        Some(Request::Raw {
            method: "GET".to_owned(),
            path: "/github.com/a/b/-/raw/c".to_owned()
        })
    );
}

#[test]
fn a_request_without_variables_has_null_variables() {
    let case = parse(
        "args: x\nexit: 0\n--- request Op\n--- query\nq\n--- response 200\n",
    );
    assert_eq!(
        case.exchanges[0].request,
        Some(Request::Graphql {
            operation: "Op".to_owned(),
            variables: serde_json::Value::Null,
            query: "q\n".to_owned()
        })
    );
}

#[test]
fn a_response_alone_is_an_exchange_without_a_request() {
    let case =
        parse("args: x\nexit: 0\n--- response 200 application/json\n{}\n");
    assert_eq!(case.exchanges[0].request, None);
}

#[test]
fn a_case_file_round_trips_byte_for_byte() {
    for text in [
        "args: x\nexit: 0\n",
        "# c\n\nargs:   x   'y z'\n# between\nexit: 1\nstdout: closed\n",
        "args: x\nexit: 0\n--- stdout\na\n\n\n--- stderr\nb",
        "args: x\nexit: 0\n--- stdout\n--- stderr\n",
        "args: x\nexit: 0\n--- stdout",
        "args: x\r\nexit: 0\r\n--- stdout\r\na\r\n",
        &with_exchange("--- stdout\nout\n--- stderr\nerr\n"),
        &with_exchange("")
            .replace("200 application/json", "200 application/json  "),
        "args: x\nexit: 0\n--- request GET /r\n--- response 404\n",
    ] {
        assert_eq!(parse(text).to_text(), text);
    }
}

fn graphql(
    operation: &str,
    variables: &serde_json::Value,
    query: &str,
) -> Request {
    Request::Graphql {
        operation: operation.to_owned(),
        variables: variables.clone(),
        query: query.to_owned(),
    }
}

#[test]
fn bless_rewrites_the_exit_code_and_both_streams() {
    let mut case =
        parse("# keep\nargs: x\nexit: 0\n--- stdout\nold\n--- stderr\nold\n");
    case.bless(1, Some("new\nout\n"), "sego: error: e\n", &[])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        "# keep\nargs: x\nexit: 1\n--- stdout\nnew\nout\n--- stderr\nsego: \
         error: e\n"
    );
}

#[test]
fn bless_adds_a_missing_stream_in_order_and_drops_an_empty_one() {
    let mut case = parse("args: x\nexit: 0\n--- stdout\nold\n");
    case.bless(0, Some("old\n"), "note\n", &[])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        "args: x\nexit: 0\n--- stdout\nold\n--- stderr\nnote\n"
    );
    case.bless(0, Some(""), "note\n", &[]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(case.to_text(), "args: x\nexit: 0\n--- stderr\nnote\n");
    let mut case =
        parse("args: x\nexit: 0\n--- request GET /r\n--- response 200\n");
    case.bless(0, Some("out\n"), "", &[]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        "args: x\nexit: 0\n--- stdout\nout\n--- request GET /r\n--- response 200\n"
    );
}

#[test]
fn bless_leaves_a_closed_stdout_alone() {
    let text = "args: x\nexit: 0\nstdout: closed\n--- stdout\nkept\n";
    let mut case = parse(text);
    case.bless(0, None, "", &[]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(case.to_text(), text);
}

const RESPONSE: &str =
    "--- response 200 application/json\n{\n  \"data\": null\n}\n";

fn refs_request(repo: &str, query: &str) -> Request {
    graphql("Refs", &json!({"first": 101, "repo0": repo}), query)
}

#[test]
fn bless_rewrites_the_request_and_query_of_an_exchange_without_a_stamp() {
    let text = format!(
        "# unrelated\nargs: x\nexit: 0\n--- request Refs {{}}\n--- query\nq\n{RESPONSE}"
    );
    let mut case = parse(&text);
    let sent = refs_request("example.com/c/d", "query Refs { r0 }");
    case.bless(0, Some(""), "", &[sent]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        format!(
            "# unrelated\nargs: x\nexit: 0\n--- request Refs \
             {{\"first\":101,\"repo0\":\"example.com/c/d\"}}\n--- \
             query\nquery Refs {{ r0 }}\n{RESPONSE}"
        )
    );
}

#[test]
fn bless_writes_the_request_of_a_response_written_alone() {
    let mut case = parse(&format!("args: x\nexit: 0\n{RESPONSE}"));
    let sent = refs_request("example.com/c/d", "query Refs {\n r0\n}\n");
    case.bless(0, Some(""), "", &[sent]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        format!(
            "args: x\nexit: 0\n--- request Refs \
             {{\"first\":101,\"repo0\":\"example.com/c/d\"}}\n--- \
             query\nquery Refs {{\n r0\n}}\n{RESPONSE}"
        )
    );
}

#[test]
fn bless_turns_a_graphql_request_into_a_raw_read_and_back() {
    let text = format!(
        "args: x\nexit: 0\n--- request Op {{}}\n--- query\nq\n{RESPONSE}"
    );
    let mut case = parse(&text);
    let raw = Request::Raw { method: "GET".to_owned(), path: "/r".to_owned() };
    case.bless(0, Some(""), "", &[raw]).unwrap_or_else(|e| panic!("{e}"));
    let blessed = format!("args: x\nexit: 0\n--- request GET /r\n{RESPONSE}");
    assert_eq!(case.to_text(), blessed);
    let sent = graphql("Op", &json!({}), "q\n");
    case.bless(0, Some(""), "", &[sent]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(case.to_text(), text);
}

#[test]
fn bless_keeps_a_request_that_differs_only_in_whitespace() {
    let text = with_exchange("");
    let mut case = parse(&text);
    let sent = refs_request(
        "example.com/a/b",
        "query Refs($repo0: String!) { r0: repository(name: $repo0) { name } }",
    );
    case.bless(0, Some(""), "", &[sent]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(case.to_text(), text);
}

#[test]
fn bless_refuses_a_recorded_exchange_whose_request_changed() {
    let text = with_exchange("").replace(
        "200 application/json\n",
        "200 application/json recorded 2026-10-07 https://sourcegraph.com\n",
    );
    let mut case = parse(&text);
    let sent = refs_request("example.com/c/d", "query Refs { r0 }");
    let error = case
        .bless(0, Some(""), "", &[sent])
        .err()
        .unwrap_or_else(|| panic!("blessed a recorded exchange"));
    assert!(error.contains("just record"), "{error}");
    assert_eq!(case.to_text(), text);
}

#[test]
fn bless_leaves_a_recorded_exchange_whose_request_is_unchanged() {
    let text = with_exchange("").replace(
        "200 application/json\n",
        "200 application/json recorded 2026-10-07 https://sourcegraph.com\n",
    );
    let mut case = parse(&text);
    let sent = refs_request(
        "example.com/a/b",
        "query Refs($repo0: String!) {\n  r0: repository(name: $repo0) { name }\n}\n",
    );
    case.bless(0, Some(""), "", &[sent]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(case.to_text(), text);
}

fn recorded(body: &str) -> Exchange {
    Exchange {
        request: Some(refs_request("example.com/c/d", "query Refs { r0 }\n")),
        response: Response {
            served: Served::Reply {
                status: 200,
                content_type: Some("application/json".to_owned()),
                headers: Headers::default(),
            },
            stamp: Some(Stamp {
                date: "2026-10-07".to_owned(),
                endpoint: "https://sourcegraph.com".to_owned(),
                live_values: false,
            }),
            body: body.to_owned(),
        },
    }
}

#[test]
fn an_exchange_set_after_the_last_is_appended_and_the_rest_is_kept() {
    let before = "# unrelated\nargs: x\nexit: 0\n--- stdout\nout\n";
    let mut case = parse(before);
    case.set_exchange(0, &recorded("{\n  \"data\": null\n}\n"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        format!(
            "{before}--- request Refs \
             {{\"first\":101,\"repo0\":\"example.com/c/d\"}}\n--- query\nquery \
             Refs {{ r0 }}\n--- response 200 application/json recorded \
             2026-10-07 https://sourcegraph.com\n{{\n  \"data\": null\n}}\n"
        )
    );
    assert_eq!(case.exchanges, [recorded("{\n  \"data\": null\n}\n")]);
}

#[test]
fn an_exchange_set_in_place_replaces_only_that_exchange() {
    let text = format!("{}{EXCHANGE}", with_exchange(""));
    let mut case = parse(&text);
    case.set_exchange(0, &recorded("{}\n")).unwrap_or_else(|e| panic!("{e}"));
    let replaced = case.to_text();
    assert!(replaced.ends_with(EXCHANGE), "{replaced}");
    assert!(replaced.starts_with("args: refs example.com/a/b\nexit: 0\n"));
    assert_eq!(case.exchanges.len(), 2);
    assert_eq!(case.exchanges[0], recorded("{}\n"));
}

#[test]
fn an_exchange_cannot_skip_ahead_or_follow_an_unterminated_file() {
    let mut case = parse("args: x\nexit: 0\n");
    assert!(case.set_exchange(1, &recorded("{}\n")).is_err());
    let mut case = parse("args: x\nexit: 0");
    assert!(case.set_exchange(0, &recorded("{}\n")).is_err());
    assert_eq!(case.to_text(), "args: x\nexit: 0");
}

#[test]
fn a_raw_read_is_written_with_its_stamp_and_an_empty_body() {
    let mut case = parse("args: x\nexit: 0\n");
    let exchange = Exchange {
        request: Some(Request::Raw {
            method: "GET".to_owned(),
            path: "/r/x".to_owned(),
        }),
        response: Response {
            served: Served::Reply {
                status: 404,
                content_type: None,
                headers: Headers::default(),
            },
            stamp: Some(Stamp {
                date: "2026-10-07".to_owned(),
                endpoint: "https://sourcegraph.com".to_owned(),
                live_values: false,
            }),
            body: String::new(),
        },
    };
    case.set_exchange(0, &exchange).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        case.to_text(),
        "args: x\nexit: 0\n--- request GET /r/x\n--- response 404 recorded \
         2026-10-07 https://sourcegraph.com\n"
    );
    assert_eq!(case.exchanges, [exchange]);
}
