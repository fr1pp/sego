#[expect(dead_code, reason = "the cases target uses the rest of the module")]
mod case;
#[allow(dead_code, reason = "the cases target uses the rest of the module")]
mod check;
mod drift;
#[allow(dead_code, reason = "the cases target uses the rest of the module")]
mod hostile;
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
