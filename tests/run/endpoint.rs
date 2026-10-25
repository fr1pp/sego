use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::process::ExitCode;

use sego::{Endpoint, Env, Transport, run};

use crate::replay::Replay;

#[test]
fn a_valid_command_connects_once_to_the_environment_endpoint() {
    let server = Replay::answering(Vec::new());
    let connects = Cell::new(0);
    let url = RefCell::new(String::new());
    let connect = |endpoint: Endpoint| -> Box<dyn Transport> {
        connects.set(connects.get() + 1);
        *url.borrow_mut() = endpoint.url("/x");
        Box::new(server.clone())
    };
    let env = Env {
        endpoint: Some(OsString::from("http://127.0.0.1:9/")),
        cache_dir: Some(OsString::from("off")),
    };

    let code = run(
        ["show", "example.com/x/y:a"].map(OsString::from).into(),
        &env,
        &connect,
        &mut Vec::new(),
        &mut Vec::new(),
    );

    assert_eq!(code, ExitCode::from(1));
    assert_eq!(connects.get(), 1);
    assert_eq!(*url.borrow(), "http://127.0.0.1:9/x");
}
