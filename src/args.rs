use std::borrow::Cow;
use std::ffi::OsString;

use lexopt::Parser;

use crate::escape::shell_word;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cli {
    pub command: Option<String>,
    pub json: bool,
    pub depth: Option<u32>,
    pub help: bool,
    pub version: bool,
    pub operands: Vec<String>,
}

/// Parses the arguments after the program name. Options are long-form only
/// and may appear anywhere before `--`; any token starting with a single
/// `-` is an operand. The first operand is the command.
///
/// Parsing stops at the first error, which names the command read so
/// far.
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Cli, ParseError> {
    let mut parser = Parser::from_args(args);
    let count = remaining(&mut parser);
    let mut cli = Cli::default();
    parse_into(&mut parser, &mut cli).map_err(|error| {
        let position = count - remaining(&mut parser);
        ParseError {
            message: message(error, position),
            command: cli.command.clone(),
        }
    })?;
    Ok(cli)
}

/// Arguments that don't parse: the message, and the command name read
/// before the error, if there was one.
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub command: Option<String>,
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

/// The number of arguments the parser has yet to take, or 0 while an
/// option's value is pending. The count is used only for `NonUnicodeValue`,
/// which is raised after any pending value has been taken.
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Cli, ParseError> {
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
            assert_eq!(error.command.as_deref(), Some("list"), "{depth}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn operands_must_be_utf8() {
        use std::os::unix::ffi::OsStringExt;

        let bad = || OsString::from_vec(vec![0x66, 0xff]);
        let error = parse([OsString::from("read"), bad()]).unwrap_err();
        assert_eq!(error.message, "argument 2 is not valid UTF-8");
        let mut joined = OsString::from("--depth=");
        joined.push(bad());
        let invocations = [
            vec![bad()],
            vec!["list".into(), "x".into(), "--depth".into(), bad()],
            vec!["list".into(), joined],
        ];
        let errors = invocations.map(|args| {
            let ParseError { message, command } = parse(args).unwrap_err();
            (message, command)
        });
        let error = |message: &str, command: Option<&str>| {
            (message.to_owned(), command.map(str::to_owned))
        };
        assert_eq!(
            errors,
            [
                error("argument 1 is not valid UTF-8", None),
                error("argument 4 is not valid UTF-8", Some("list")),
                error("argument 2 is not valid UTF-8", Some("list")),
            ]
        );
    }
}
