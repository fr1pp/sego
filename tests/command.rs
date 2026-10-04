use std::ffi::{OsStr, OsString};
use std::process::{self, Output, Stdio};

/// Asserts `command.run().transcript()` against an inline snapshot:
/// `check!(sego().arg("--version"), expect![[...]])`, with `expect`
/// imported from `expect_test`.
macro_rules! check {
    ($command:expr, $expect:expr $(,)?) => {
        $expect.assert_eq(&$command.run().transcript())
    };
}

pub struct Command {
    program: String,
    args: Vec<OsString>,
    env: Vec<(String, OsString)>,
}

impl Command {
    pub fn bin(path: &str) -> Command {
        Command { program: path.to_owned(), args: Vec::new(), env: Vec::new() }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    pub fn args(
        mut self,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Self {
        self.args.extend(args.into_iter().map(|arg| arg.as_ref().to_owned()));
        self
    }

    pub fn env(mut self, key: &str, value: impl AsRef<OsStr>) -> Self {
        self.env.push((key.to_owned(), value.as_ref().to_owned()));
        self
    }

    pub fn run(&self) -> Run {
        let output = self.command().output();
        collect(output.unwrap_or_else(|error| self.cannot_run(&error)))
    }

    pub fn run_with_closed_stdout(&self) -> Run {
        let mut child = self
            .command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| self.cannot_run(&error));
        drop(child.stdout.take());
        collect(child.wait_with_output().expect("wait for the child"))
    }

    fn command(&self) -> process::Command {
        let mut command = process::Command::new(&self.program);
        command.args(&self.args).stdin(Stdio::null());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    fn cannot_run(&self, error: &std::io::Error) -> ! {
        panic!("cannot run {}: {error}", self.program)
    }
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// Each stream is followed by `[EOF]`, so a missing final newline shows.
    pub fn transcript(&self) -> String {
        let Run { code, stdout, stderr } = self;
        format!(
            "exit {code}\n\
             --- stdout\n\
             {stdout}[EOF]\n\
             --- stderr\n\
             {stderr}[EOF]\n"
        )
    }
}

fn collect(output: Output) -> Run {
    let Output { status, stdout, stderr } = output;
    let Some(code) = status.code() else {
        panic!("no exit code: {status}");
    };
    Run {
        code,
        stdout: String::from_utf8(stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(stderr).expect("stderr is UTF-8"),
    }
}
