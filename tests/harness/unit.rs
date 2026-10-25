#[expect(dead_code, reason = "the cases target uses the rest of the module")]
mod case;
mod drift;
mod literals;
#[expect(dead_code, reason = "the cases target uses the rest of the module")]
mod live;
#[allow(dead_code, reason = "the cases target uses the rest of the module")]
mod record;
#[allow(dead_code, reason = "the cases target uses the rest of the module")]
mod replay;
#[allow(dead_code, reason = "the cases target uses the rest of the module")]
mod run;
mod shell;
