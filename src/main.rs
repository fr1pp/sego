use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    if env::args_os().skip(1).eq(["--version"]) {
        println!("sego {}", env!("CARGO_PKG_VERSION"));
        ExitCode::SUCCESS
    } else {
        eprintln!("error: not implemented");
        ExitCode::from(2)
    }
}
