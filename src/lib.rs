mod error;
mod escape;
mod quote;

#[doc(hidden)]
pub mod internal {
    pub mod error {
        pub use crate::error::*;
    }
    pub mod escape {
        pub use crate::escape::*;
    }
    pub mod quote {
        pub use crate::quote::*;
    }
}
