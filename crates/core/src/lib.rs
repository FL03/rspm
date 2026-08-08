/*
    appellation: rspm-core <library>
    authors: @FL03
*/
//! The core modules of the rspm project
#![allow(
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::module_inception,
    clippy::needless_doctest_main
)]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "nightly", feature(allocator_api))]

#[cfg(not(all(feature = "alloc", feature = "std")))]
compile_error! {
    "Either the `alloc` or `std` feature must be enabled for the crate to compile."
}

#[cfg(feature = "alloc")]
extern crate alloc;

#[doc(inline)]
pub use self::{
    error::{Error, Result},
    traits::prelude::*,
};

pub mod error;

#[macro_use]
pub(crate) mod macros {
    #[macro_use]
    pub(crate) mod seal;
}

pub mod traits {
    //! the core traits for the rspm project
    //!
    #[doc(inline)]
    pub mod field;

    pub(crate) mod prelude {
        #[allow(unused_imports)]
        pub use crate::error::{Error, Result};
    }
}

#[doc(hidden)]
pub mod prelude {
    pub use crate::traits::prelude::*;
}
