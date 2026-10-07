mod address;
mod error;
mod escape;
mod output;
mod quote;
mod upstream;

#[doc(hidden)]
pub mod internal {
    pub mod address {
        pub use crate::address::*;
    }
    pub mod error {
        pub use crate::error::*;
    }
    pub mod escape {
        pub use crate::escape::*;
    }
    pub mod output {
        pub use crate::output::*;
    }
    pub mod quote {
        pub use crate::quote::*;
    }
    pub mod upstream {
        pub use crate::upstream::*;
    }
}
