//! The command line itself: help, versions and usage errors.

use insta::assert_snapshot;

use crate::common::case::Case;
use crate::common::sego;

/// A closed stdout is not an error and nothing more is written.
#[test]
fn closed_pipe() {
    assert_snapshot!(Case::new(&["--help"], Vec::new()).stdout_closed().ran());
}

/// A range is not a commit; no request is made.
#[test]
fn commit_range() {
    assert_snapshot!(sego(&["show", "example.com/acme/widget@v1...v2"]));
}

/// A commit takes no LINES; no request is made.
#[test]
fn commit_with_lines() {
    assert_snapshot!(sego(&["show", "example.com/acme/widget@v1", "3-9"]));
}

/// diff --help prints diff's synopses, operands, options, text and JSON fields.
#[test]
fn diff_help() {
    assert_snapshot!(sego(&["diff", "--help"]));
}

/// --recursive belongs to ls; diff names it and prints its own synopsis.
#[test]
fn diff_recursive() {
    assert_snapshot!(sego(&["diff", "example.com/acme/widget@v1", "--recursive"]));
}

/// diff takes A...B; a two-dot range is a usage error and no request is made.
#[test]
fn diff_two_dot_range() {
    assert_snapshot!(sego(&["diff", "example.com/acme/widget@a..b"]));
}

/// diff needs a revision or a range; no request is made.
#[test]
fn diff_without_revision() {
    assert_snapshot!(sego(&["diff", "example.com/acme/widget"]));
}

/// --help prints the commands, addresses, output, examples, options and exit
/// codes.
#[test]
fn general_help() {
    assert_snapshot!(sego(&["--help"]));
}

/// -h before a command name prints that command's help.
#[test]
fn help_flag_before_command() {
    assert_snapshot!(sego(&["-h", "show"]));
}

/// No option takes a value.
#[test]
fn json_with_value() {
    assert_snapshot!(sego(&["show", "--json=1", "example.com/acme/widget:a.go"]));
}

/// --limit is an unknown option.
#[test]
fn limit_option_is_unknown() {
    assert_snapshot!(sego(&["show", "example.com/acme/widget:a.go", "--limit", "5"]));
}

/// log refuses an empty PATH operand, as diff does, and makes no request.
#[test]
fn log_empty_path() {
    assert_snapshot!(sego(&["log", "example.com/acme/widget@v1", ""]));
}

/// log --help prints log's synopses, operands, options, text and JSON fields.
#[test]
fn log_help() {
    assert_snapshot!(sego(&["log", "--help"]));
}

/// --recursive belongs to ls; log names it and prints its own synopsis.
#[test]
fn log_recursive() {
    assert_snapshot!(sego(&["log", "--recursive", "example.com/acme/widget"]));
}

/// log takes A..B; a three-dot range is a usage error and no request is made.
#[test]
fn log_three_dot_range() {
    assert_snapshot!(sego(&["log", "example.com/acme/widget@a...b"]));
}

/// log takes one path; :PATH together with a PATH operand is a usage error.
#[test]
fn log_two_paths() {
    assert_snapshot!(sego(&["log", "example.com/acme/widget:src", "lib"]));
}

/// ls --help prints ls's synopsis, addresses, options, text and JSON fields.
#[test]
fn ls_help() {
    assert_snapshot!(sego(&["ls", "--help"]));
}

/// ls needs an address; no request is made.
#[test]
fn ls_no_address() {
    assert_snapshot!(sego(&["ls"]));
}

/// A range revision is only valid in log and diff; no request is made.
#[test]
fn ls_range() {
    assert_snapshot!(sego(&["ls", "example.com/acme/widget@a..b:src"]));
}

/// No command at all.
#[test]
fn no_command() {
    assert_snapshot!(sego(&[]));
}

/// An option given twice is a usage error.
#[test]
fn option_given_twice() {
    assert_snapshot!(sego(&["show", "--json", "example.com/acme/widget:a.go", "--json"]));
}

/// An empty TEXT operand is refused and no request is made.
#[test]
fn refs_empty_text() {
    assert_snapshot!(sego(&["refs", "example.com/acme/widget", ""]));
}

/// refs --help prints refs's synopsis, operands, options, text and JSON fields.
#[test]
fn refs_help() {
    assert_snapshot!(sego(&["refs", "--help"]));
}

/// refs names a repository; a revision is refused and no request is made.
#[test]
fn refs_revision() {
    assert_snapshot!(sego(&["refs", "example.com/acme/widget@v1"]));
}

/// search --help prints the query grammar, options, defaults, text and JSON.
#[test]
fn search_help() {
    assert_snapshot!(sego(&["search", "--help"]));
}

/// A search with no query text is a usage error.
#[test]
fn search_without_query() {
    assert_snapshot!(sego(&["search"]));
}

/// REPO: names the root, which show cannot print; no request is made.
#[test]
fn show_empty_path() {
    assert_snapshot!(sego(&["show", "example.com/acme/widget:"]));
}

/// show --help prints show's synopses, addresses, options, text and JSON fields.
#[test]
fn show_help() {
    assert_snapshot!(sego(&["show", "--help"]));
}

/// --recursive belongs to ls; show names it and prints its own synopsis.
#[test]
fn show_recursive() {
    assert_snapshot!(sego(&["show", "--recursive", "example.com/acme/widget:a.go"]));
}

/// show REPO names neither a file nor a commit; no request is made.
#[test]
fn show_repository_alone() {
    assert_snapshot!(sego(&["show", "example.com/acme/widget"]));
}

/// A single dash before the command is an option, not query text.
#[test]
fn single_dash_before_command() {
    assert_snapshot!(sego(&["-x", "search", "widget"]));
}

/// An unknown command is named and the commands listed, with no suggestion.
#[test]
fn unknown_command() {
    assert_snapshot!(sego(&["shwo", "example.com/acme/widget:a.go"]));
}

/// A help flag does not excuse an unknown command.
#[test]
fn unknown_command_help() {
    assert_snapshot!(sego(&["shwo", "--help"]));
}

/// A version flag does not excuse an unknown command.
#[test]
fn unknown_command_version() {
    assert_snapshot!(sego(&["shwo", "--version"]));
}

/// The control characters of an unknown command are written as escapes, and a newline as a space.
#[test]
fn unknown_command_with_control_characters() {
    assert_snapshot!(sego(&["sh\u{1b}wo\0\n\r\t\u{7f}\u{202e}"]));
}

/// An unknown option is named and nothing else is said about it.
#[test]
fn unknown_option() {
    assert_snapshot!(sego(&["show", "--verbose", "example.com/acme/widget:a.go"]));
}

/// A usage error written to a closed stderr exits quietly.
#[test]
fn usage_error_with_closed_stderr() {
    assert_snapshot!(Case::new(&["shwo"], Vec::new()).stderr_closed().ran());
}

/// --version prints the version.
#[test]
fn version() {
    assert_snapshot!(sego(&["--version"]));
}

/// A count of zero is refused before any request is sent.
#[test]
fn a_count_of_zero_is_refused() {
    assert_snapshot!(sego(&["log", "-n", "0", "example.com/acme/widget"]));
}

#[test]
fn a_skip_that_is_not_a_whole_number_is_refused() {
    assert_snapshot!(sego(&["log", "--skip", "-1", "example.com/acme/widget"]));
}

#[test]
fn follow_refuses_a_skip() {
    assert_snapshot!(sego(&["log", "--follow", "--skip", "1", "example.com/acme/widget", "a.go"]));
}

#[test]
fn follow_refuses_a_count_past_its_limit() {
    assert_snapshot!(sego(&["log", "--follow", "-n", "20001", "example.com/acme/widget", "a.go"]));
}

#[test]
fn a_count_belongs_to_log() {
    assert_snapshot!(sego(&["show", "-n", "1", "example.com/acme/widget:a.go"]));
}
