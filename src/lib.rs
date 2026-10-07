mod error;
mod escape;

#[doc(hidden)]
pub mod internal {
    pub mod error {
        pub use crate::error::*;
    }
    pub mod escape {
        pub use crate::escape::*;
    }
}
