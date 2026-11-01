use super::recording::ServerResponse;

/// A command line to run, and what answers it: the case's own responses in
/// order, for what sourcegraph.com can't be made to give, or else the recordings.
#[derive(Clone, Debug)]
pub struct Case {
    pub args: Vec<String>,
    pub responses: Vec<ServerResponse>,
    /// `SEGO_CACHE_DIR`, when not an empty directory of the case's own.
    pub cache_dir: Option<String>,
    pub stdout_closed: bool,
    pub stderr_closed: bool,
}

impl Case {
    pub fn new(args: &[&str], responses: Vec<ServerResponse>) -> Case {
        Case {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            responses,
            cache_dir: None,
            stdout_closed: false,
            stderr_closed: false,
        }
    }

    pub fn cache_dir(self, dir: &str) -> Case {
        Case { cache_dir: Some(dir.to_owned()), ..self }
    }

    /// Runs the case with stdout's reader gone.
    pub fn stdout_closed(self) -> Case {
        Case { stdout_closed: true, ..self }
    }

    /// Runs the case with stderr's reader gone.
    pub fn stderr_closed(self) -> Case {
        Case { stderr_closed: true, ..self }
    }

    /// The command line, for a failure message.
    pub fn describe(&self) -> String {
        self.args.join(" ")
    }
}
