use std::fmt;
use std::io::{self, BufWriter, ErrorKind, Write};

use crate::diagnostic::{Diagnostic, Outcome};

/// stdout for answers: buffered, with every write checked. A closed pipe
/// means the reader has gone, so writing stops quietly from then on.
pub struct Output<W: Write> {
    /// `None` once the pipe has closed.
    inner: Option<BufWriter<W>>,
}

impl<W: Write> Output<W> {
    pub fn new(inner: W) -> Self {
        Output { inner: Some(BufWriter::with_capacity(64 * 1024, inner)) }
    }

    pub fn write(&mut self, text: &str) -> Result<(), OutputError> {
        let result = match &mut self.inner {
            Some(writer) => writer.write_all(text.as_bytes()),
            None => Ok(()),
        };
        self.check(result)
    }

    pub fn flush(&mut self) -> Result<(), OutputError> {
        let result = match &mut self.inner {
            Some(writer) => writer.flush(),
            None => Ok(()),
        };
        self.check(result)
    }

    fn check(&mut self, result: io::Result<()>) -> Result<(), OutputError> {
        match result {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::BrokenPipe => {
                // Discards what is still buffered, which dropping the
                // `BufWriter` would try to write.
                if let Some(writer) = self.inner.take() {
                    drop(writer.into_parts());
                }
                Ok(())
            }
            Err(error) => Err(OutputError(error)),
        }
    }
}

/// Writes to stderr, ignoring failure: there is nowhere left to report it.
pub fn to_stderr(text: &str) {
    let _ = io::stderr().write_all(text.as_bytes());
}

#[derive(Debug)]
pub struct OutputError(io::Error);

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cannot write to stdout")
    }
}

impl Diagnostic for OutputError {
    fn code(&self) -> &'static str {
        "output"
    }

    fn outcome(&self) -> Outcome {
        Outcome::Unanswered
    }

    fn causes(&self) -> Vec<String> {
        vec![self.0.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// Records every write offered to it, and fails writes or flushes on
    /// demand.
    #[derive(Default)]
    struct Pipe {
        received: Rc<RefCell<Vec<u8>>>,
        write_error: Option<ErrorKind>,
        flush_error: Option<ErrorKind>,
    }

    impl Write for Pipe {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.received.borrow_mut().extend_from_slice(bytes);
            self.write_error.map_or(Ok(bytes.len()), |kind| Err(kind.into()))
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flush_error.map_or(Ok(()), |kind| Err(kind.into()))
        }
    }

    /// More than the buffer holds, so writing it reaches the writer at once.
    fn oversized() -> String {
        "x".repeat(64 * 1024 + 1)
    }

    #[test]
    fn nothing_reaches_the_writer_before_flush() {
        let pipe = Pipe::default();
        let received = Rc::clone(&pipe.received);
        let mut output = Output::new(pipe);
        output.write("one\n").unwrap();
        output.write("two\n").unwrap();
        assert!(received.borrow().is_empty());
        output.flush().unwrap();
        assert_eq!(*received.borrow(), b"one\ntwo\n");
    }

    #[test]
    fn a_closed_pipe_on_write_stops_quietly() {
        let pipe = Pipe {
            write_error: Some(ErrorKind::BrokenPipe),
            ..Pipe::default()
        };
        let received = Rc::clone(&pipe.received);
        let mut output = Output::new(pipe);
        output.write("first\n").unwrap();
        output.write(&oversized()).unwrap();
        output.write("later\n").unwrap();
        output.flush().unwrap();
        drop(output);
        assert_eq!(*received.borrow(), b"first\n");
    }

    #[test]
    fn a_closed_pipe_on_flush_stops_quietly() {
        let pipe = Pipe {
            flush_error: Some(ErrorKind::BrokenPipe),
            ..Pipe::default()
        };
        let received = Rc::clone(&pipe.received);
        let mut output = Output::new(pipe);
        output.write("first\n").unwrap();
        output.flush().unwrap();
        output.write("later\n").unwrap();
        output.flush().unwrap();
        drop(output);
        assert_eq!(*received.borrow(), b"first\n");
    }

    #[test]
    fn other_write_errors_are_output_errors() {
        let pipe = Pipe {
            write_error: Some(ErrorKind::StorageFull),
            ..Pipe::default()
        };
        let mut output = Output::new(pipe);
        let error = output.write(&oversized()).unwrap_err();
        assert_eq!(
            (error.code(), error.outcome(), error.to_string()),
            ("output", Outcome::Unanswered, "cannot write to stdout".into())
        );
        assert_eq!(error.causes(), ["no storage space"]);
    }
}
