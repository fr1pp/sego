use std::borrow::Cow;
use std::ffi::OsString;
use std::fmt;

use lexopt::Parser;

use crate::cmd::COMMANDS;
use crate::diagnostic::{Diagnostic, Outcome};
use crate::escape::{command_line, shell_word};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    pub command: Option<String>,
    pub json: bool,
    pub depth: Option<u32>,
    pub help: bool,
    pub version: bool,
    pub operands: Vec<String>,
}

impl Cli {
    /// The invocation as one shell command line: `sego`, the command, the
    /// operands, then the options. When an operand starts with `--`, the
    /// options go first and `--` comes before the operands.
    pub fn command_line(&self) -> String {
        let command = self.command.as_deref().map(shell_word);
        let mut options = Vec::new();
        if self.json {
            options.push("--json".to_owned());
        }
        if let Some(depth) = self.depth {
            options.push(format!("--depth {depth}"));
        }
        if self.help {
            options.push("--help".to_owned());
        }
        if self.version {
            options.push("--version".to_owned());
        }
        let mut head = vec!["sego"];
        head.extend(command.as_deref());
        let operands: Vec<&str> =
            self.operands.iter().map(String::as_str).collect();
        if operands.iter().any(|operand| operand.starts_with("--")) {
            head.extend(options.iter().map(String::as_str));
            command_line(&head, &operands)
        } else {
            let mut line = command_line(&head, &operands);
            for option in &options {
                line.push(' ');
                line.push_str(option);
            }
            line
        }
    }

    /// `sego <command> --help` for a known command, else `sego --help`.
    pub fn help_hint(&self) -> String {
        match self.command.as_deref() {
            Some(command) if COMMANDS.contains(&command) => {
                format!("sego {command} --help")
            }
            Some(_) | None => "sego --help".to_owned(),
        }
    }
}

/// Parses the arguments after the program name. Options are long-form only
/// and may appear anywhere before `--`; any token starting with a single
/// `-` is an operand. The first operand is the command.
///
/// Parsing stops at the first error, whose hint is the help of the command
/// read so far.
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Cli, UsageError> {
    let mut parser = Parser::from_args(args);
    let count = remaining(&mut parser);
    let mut cli = Cli::default();
    parse_into(&mut parser, &mut cli).map_err(|error| {
        let position = count - remaining(&mut parser);
        UsageError::new(message(error, position)).with_hint(cli.help_hint())
    })?;
    Ok(cli)
}

/// Whether `--json` is among the options, so that even a usage error can
/// be written as a document.
pub fn wants_json(args: &[OsString]) -> bool {
    args.iter().take_while(|arg| *arg != "--").any(|arg| arg == "--json")
}

fn parse_into(
    parser: &mut Parser,
    cli: &mut Cli,
) -> Result<(), lexopt::Error> {
    use lexopt::prelude::*;

    loop {
        let arg = if let Some(operand) = single_dash(parser) {
            Value(operand)
        } else if let Some(arg) = parser.next()? {
            arg
        } else {
            return Ok(());
        };
        match arg {
            Long("json") => cli.json = true,
            Long("help") => cli.help = true,
            Long("version") => cli.version = true,
            Long("depth") => {
                cli.depth = Some(parser.value()?.parse_with(depth)?);
            }
            Value(command) if cli.command.is_none() => {
                cli.command = Some(command.string()?);
            }
            Value(operand) => cli.operands.push(operand.string()?),
            _ => return Err(arg.unexpected()),
        }
    }
}

/// The next argument, whole, if it starts with a single `-`. sego has no
/// short options, and search negations such as `-file:_test` are operands,
/// so these never reach `Parser::next`, which would split them.
fn single_dash(parser: &mut Parser) -> Option<OsString> {
    parser.try_raw_args()?.next_if(|arg| {
        let bytes = arg.as_encoded_bytes();
        bytes.starts_with(b"-") && !bytes.starts_with(b"--")
    })
}

/// The number of arguments the parser has yet to take.
fn remaining(parser: &mut Parser) -> usize {
    parser.try_raw_args().map_or(0, |raw| raw.as_slice().len())
}

/// `--depth`'s value, a positive number. Its error is the whole message.
fn depth(value: &str) -> Result<u32, String> {
    match value.parse() {
        Ok(depth) if depth > 0 => Ok(depth),
        Ok(_) | Err(_) => Err(format!(
            "invalid --depth {}; expected a number from 1 to {}",
            shell_word(value),
            u32::MAX
        )),
    }
}

/// The message for `error`, raised at the argument at 1-based `position`,
/// with any echoed input as shell words.
fn message(error: lexopt::Error, position: usize) -> String {
    match error {
        lexopt::Error::MissingValue { option } => {
            let option = option.as_deref().unwrap_or("option");
            format!("{} needs a value", option_word(option))
        }
        lexopt::Error::UnexpectedOption(option) => {
            format!("unknown option {}", option_word(&option))
        }
        lexopt::Error::UnexpectedArgument(operand) => format!(
            "unexpected operand {}",
            shell_word(&operand.to_string_lossy())
        ),
        lexopt::Error::UnexpectedValue { option, .. } => {
            format!("{} takes no value", option_word(&option))
        }
        lexopt::Error::NonUnicodeValue(_) => {
            format!("argument {position} is not valid UTF-8")
        }
        lexopt::Error::ParsingFailed { error, .. }
        | lexopt::Error::Custom(error) => error.to_string(),
    }
}

/// `option`, spelled with its dashes, as a shell word: bare when its name
/// needs no quoting, although `shell_word` quotes any leading `-`.
fn option_word(option: &str) -> Cow<'_, str> {
    let name = option.trim_start_matches('-');
    if shell_word(name) == name {
        Cow::Borrowed(option)
    } else {
        shell_word(option)
    }
}

/// An invocation that can't run as written.
#[derive(Debug)]
pub struct UsageError {
    message: String,
    hints: Vec<String>,
}

impl UsageError {
    pub fn new(message: impl Into<String>) -> Self {
        UsageError { message: message.into(), hints: Vec::new() }
    }

    pub fn with_hint(mut self, hint: String) -> Self {
        self.hints.push(hint);
        self
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Diagnostic for UsageError {
    fn code(&self) -> &'static str {
        "usage"
    }

    fn outcome(&self) -> Outcome {
        Outcome::Invalid
    }

    fn hints(&self) -> Vec<String> {
        self.hints.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use expect_test::expect;

    use super::*;
    use crate::diagnostic::{Diagnostic, Outcome};

    fn parse_strs(args: &[&str]) -> Result<Cli, UsageError> {
        parse(args.iter().map(OsString::from))
    }

    fn cli(command: &str, operands: &[&str]) -> Cli {
        Cli {
            command: Some(command.into()),
            operands: operands.iter().map(|&s| s.into()).collect(),
            ..Cli::default()
        }
    }

    #[test]
    fn options_may_follow_operands() {
        assert_eq!(
            parse_strs(&["read", "x", "--json"]).unwrap(),
            Cli { json: true, ..cli("read", &["x"]) }
        );
    }

    #[test]
    fn single_dash_tokens_are_operands() {
        assert_eq!(
            parse_strs(&["search", "-file:_test", "x"]).unwrap(),
            cli("search", &["-file:_test", "x"])
        );
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(
            parse_strs(&["search", "--", "--json"]).unwrap(),
            cli("search", &["--json"])
        );
    }

    #[test]
    fn depth_takes_a_positive_number() {
        let expected = Cli { depth: Some(2), ..cli("list", &["x"]) };
        let spaced = parse_strs(&["list", "x", "--depth", "2"]).unwrap();
        let joined = parse_strs(&["list", "x", "--depth=2"]).unwrap();
        assert_eq!((spaced, joined), (expected.clone(), expected));
        for depth in ["0", "x", "99999999999"] {
            let error =
                parse_strs(&["list", "x", "--depth", depth]).unwrap_err();
            assert_eq!(
                (error.code(), error.outcome()),
                ("usage", Outcome::Invalid),
                "{depth}"
            );
        }
    }

    #[test]
    fn errors_hint_the_help_of_the_command_read_so_far() {
        let invocations: &[&[&str]] = &[
            &["read", "x", "a", "--jsno"],
            &["--jsno", "read", "x"],
            &["read", "x/y", "--weird.go", "1-2"],
            &["search", "--weird", "x"],
            &["search", "--a b"],
            &["search", "--=x"],
            &["list", "x", "--depth"],
            &["list", "x", "--depth", "0"],
            &["read", "x", "--depth=x"],
            &["--depth", "99999999999", "list", "x"],
            &["--json=1", "search", "x"],
            &["search", "x", "--help=x"],
        ];
        let errors: Vec<String> = invocations
            .iter()
            .map(|argv| {
                let error = parse_strs(argv).unwrap_err();
                format!("{error} | {}\n", error.hints().join(" | "))
            })
            .collect();
        expect![[r#"
            unknown option --jsno | sego read --help
            unknown option --jsno | sego --help
            unknown option --weird.go | sego read --help
            unknown option --weird | sego search --help
            unknown option '--a b' | sego search --help
            unknown option '--' | sego search --help
            --depth needs a value | sego list --help
            invalid --depth 0; expected a number from 1 to 4294967295 | sego list --help
            invalid --depth x; expected a number from 1 to 4294967295 | sego read --help
            invalid --depth 99999999999; expected a number from 1 to 4294967295 | sego --help
            --json takes no value | sego --help
            --help takes no value | sego search --help
        "#]]
        .assert_eq(&errors.concat());
    }

    #[cfg(unix)]
    #[test]
    fn operands_must_be_utf8() {
        use std::os::unix::ffi::OsStringExt;

        let bad = || OsString::from_vec(vec![0x66, 0xff]);
        let error = parse([OsString::from("read"), bad()]).unwrap_err();
        assert_eq!(error.code(), "usage");
        assert_eq!(error.to_string(), "argument 2 is not valid UTF-8");
        let mut joined = OsString::from("--depth=");
        joined.push(bad());
        let invocations = [
            vec![bad()],
            vec!["list".into(), "x".into(), "--depth".into(), bad()],
            vec!["list".into(), joined],
        ];
        let errors = invocations.map(|args| {
            let error = parse(args).unwrap_err();
            format!("{error} | {}", error.hints().join(" | "))
        });
        assert_eq!(
            errors,
            [
                "argument 1 is not valid UTF-8 | sego --help",
                "argument 4 is not valid UTF-8 | sego list --help",
                "argument 2 is not valid UTF-8 | sego list --help",
            ]
        );
    }
}
