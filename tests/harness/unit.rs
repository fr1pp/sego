#![allow(
    dead_code,
    reason = "the modules are shared with the cases target, which uses the rest"
)]

// Declares a module's `tests` child. The `cases` and `run` targets build some
// of the same modules and declare nothing, so each module's tests run once,
// here.
macro_rules! module_tests {
    () => {
        #[cfg(test)]
        mod tests;
    };
}

mod case;
mod check;
mod drift;
mod hostile;
mod literals;
mod live;
mod record;
mod replay;
mod run;
mod shell;
