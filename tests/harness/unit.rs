#[expect(dead_code, reason = "the cases target uses the rest of the module")]
mod case;
mod drift;
mod literals;
#[expect(dead_code, reason = "the cases target uses the rest of the module")]
mod live;
mod shell;
