use std::ffi::OsString;
use std::fmt;

use lexopt::{Arg, Parser};

use crate::cmd::COMMANDS;
use crate::diagnostic::{Diagnostic, Outcome};
use crate::escape::{command_line, shell_word};

const OPTIONS: &[&str] = &["json", "depth", "help", "version"];

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
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Cli, UsageError> {
    let args: Vec<OsString> = args.into_iter().collect();
    parse_reading(&args, Unknown::Error)
}

/// What the parser makes of a `--` token that names no option.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unknown {
    Error,
    Operand,
}

fn parse_reading(
    args: &[OsString],
    unknown: Unknown,
) -> Result<Cli, UsageError> {
    let mut parser = Parser::from_args(args);
    let mut cli = Cli::default();
    loop {
        let operand = parser.try_raw_args().and_then(|mut raw| {
            raw.next_if(|arg| {
                let bytes = arg.as_encoded_bytes();
                bytes.starts_with(b"-") && !bytes.starts_with(b"--")
            })
        });
        let value = match operand {
            Some(operand) => operand,
            None => match parser.next() {
                Ok(None) => break,
                Ok(Some(Arg::Value(value))) => value,
                Ok(Some(Arg::Long("json"))) => {
                    cli.json = true;
                    continue;
                }
                Ok(Some(Arg::Long("help"))) => {
                    cli.help = true;
                    continue;
                }
                Ok(Some(Arg::Long("version"))) => {
                    cli.version = true;
                    continue;
                }
                Ok(Some(Arg::Long("depth"))) => {
                    let value = match parser.value() {
                        Ok(value) => value,
                        Err(error) => return Err(lexopt_error(error, &cli)),
                    };
                    let value = text(value, position(&mut parser, args))?;
                    cli.depth = Some(depth(&value)?);
                    continue;
                }
                Ok(Some(Arg::Long(name))) => {
                    let name = name.to_owned();
                    let attached = parser.optional_value();
                    let at = position(&mut parser, args);
                    if unknown == Unknown::Operand {
                        args[at - 1].clone()
                    } else {
                        let error = UsageError::new(format!(
                            "unknown option {}",
                            option_word(&name)
                        ));
                        let hint = match did_you_mean(&name, OPTIONS) {
                            Some(option) => {
                                corrected(args, at, option, attached)
                            }
                            None => literal(args),
                        };
                        let hint = hint.unwrap_or_else(|| cli.help_hint());
                        return Err(error.with_hint(hint));
                    }
                }
                Ok(Some(Arg::Short(c))) => {
                    let message = format!(
                        "unknown option {}",
                        shell_word(&format!("-{c}"))
                    );
                    return Err(
                        UsageError::new(message).with_hint(cli.help_hint())
                    );
                }
                Err(error) => return Err(lexopt_error(error, &cli)),
            },
        };
        let value = text(value, position(&mut parser, args))?;
        match cli.command {
            None => cli.command = Some(value),
            Some(_) => cli.operands.push(value),
        }
    }
    Ok(cli)
}

/// Whether `--json` is among the options, so that even a usage error can
/// be written as a document.
pub fn wants_json(args: &[OsString]) -> bool {
    args.iter().take_while(|arg| *arg != "--").any(|arg| arg == "--json")
}

/// The 1-based position of the last argument the parser took.
fn position(parser: &mut Parser, args: &[OsString]) -> usize {
    let remaining =
        parser.try_raw_args().map_or(0, |raw| raw.as_slice().len());
    args.len() - remaining
}

fn text(value: OsString, position: usize) -> Result<String, UsageError> {
    value.into_string().map_err(|_| {
        UsageError::new(format!("argument {position} is not valid UTF-8"))
    })
}

fn depth(value: &str) -> Result<u32, UsageError> {
    match value.parse() {
        Ok(depth) if depth > 0 => Ok(depth),
        Ok(_) | Err(_) => Err(UsageError::new(format!(
            "invalid --depth {}; expected a number from 1 to {}",
            shell_word(value),
            u32::MAX
        ))
        .with_hint("sego list --help".to_owned())),
    }
}

/// `--<name>` as a shell word.
fn option_word(name: &str) -> String {
    let option = format!("--{name}");
    if shell_word(name) == name {
        option
    } else {
        shell_word(&option).into_owned()
    }
}

/// The command line with the unknown option at `position` replaced by
/// `option`, if that is an invocation that runs.
fn corrected(
    args: &[OsString],
    position: usize,
    option: &str,
    attached: Option<OsString>,
) -> Option<String> {
    let mut token = OsString::from(format!("--{option}"));
    if let Some(attached) = attached {
        token.push("=");
        token.push(attached);
    }
    let mut args = args.to_vec();
    args[position - 1] = token;
    let cli = parse(args).ok()?;
    crate::cmd::validate(&cli).is_ok().then(|| cli.command_line())
}

/// The command line with every unknown option read as an operand, after
/// `--`, if that is an invocation that runs: a path or query word that
/// starts with `--`, pasted without one.
fn literal(args: &[OsString]) -> Option<String> {
    let cli = parse_reading(args, Unknown::Operand).ok()?;
    crate::cmd::validate(&cli).is_ok().then(|| cli.command_line())
}

fn lexopt_error(error: lexopt::Error, cli: &Cli) -> UsageError {
    let message = match error {
        lexopt::Error::MissingValue { option } => {
            format!("{} needs a value", option.as_deref().unwrap_or("option"))
        }
        lexopt::Error::UnexpectedValue { option, .. } => {
            format!("{option} takes no value")
        }
        error @ (lexopt::Error::UnexpectedOption(_)
        | lexopt::Error::UnexpectedArgument(_)
        | lexopt::Error::ParsingFailed { .. }
        | lexopt::Error::NonUnicodeValue(_)
        | lexopt::Error::Custom(_)) => error.to_string(),
    };
    UsageError::new(message).with_hint(cli.help_hint())
}

/// The candidate nearest `input`, if it is at most two edits away.
pub fn did_you_mean<'a>(
    input: &str,
    candidates: &[&'a str],
) -> Option<&'a str> {
    candidates
        .iter()
        .map(|&candidate| (distance(input, candidate), candidate))
        .filter(|&(edits, _)| edits <= 2)
        .min_by_key(|&(edits, _)| edits)
        .map(|(_, candidate)| candidate)
}

/// The Levenshtein distance between `a` and `b`, in characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitution = diagonal + usize::from(ca != cb);
            diagonal = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(diagonal + 1);
        }
    }
    row[b.len()]
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
    fn unknown_options_suggest_a_correction() {
        let error = parse_strs(&["read", "x", "a", "--jsno"]).unwrap_err();
        assert_eq!(error.to_string(), "unknown option --jsno");
        assert_eq!(error.hints(), ["sego read x a --json"]);
        let error = parse_strs(&["read", "x", "--jsno"]).unwrap_err();
        assert_eq!(error.hints(), ["sego read --help"]);
    }

    #[test]
    fn unknown_options_that_run_as_operands_hint_the_command_with_dashes() {
        let at = "github.com/gorilla/mux@a7962380ca08";
        let hints = |args: &[&str]| parse_strs(args).unwrap_err().hints();
        let error =
            parse_strs(&["read", at, "--weird.go", "213-214"]).unwrap_err();
        assert_eq!(error.to_string(), "unknown option --weird.go");
        assert_eq!(
            error.hints(),
            [format!("sego read -- {at} --weird.go 213-214")]
        );
        assert_eq!(
            hints(&["read", "x/y", "--a=b.go", "--json"]),
            ["sego read --json -- x/y --a=b.go"]
        );
        assert_eq!(
            hints(&["search", "--weird", "x", "--odd"]),
            ["sego search -- --weird x --odd"]
        );
        assert_eq!(
            hints(&["read", "x/y", "--weird.go", "0"]),
            ["sego read --help"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn operands_must_be_utf8() {
        use std::os::unix::ffi::OsStringExt;

        let args =
            [OsString::from("read"), OsString::from_vec(vec![0x66, 0xff])];
        let error = parse(args).unwrap_err();
        assert_eq!(error.code(), "usage");
        assert_eq!(error.to_string(), "argument 2 is not valid UTF-8");
    }
}
