/*
    Appellation: rspm <library>
    Created At: 2026.04.01
    Contrib: @FL03
*/
#![allow(
    clippy::needless_doctest_main,
    clippy::non_canonical_clone_impl,
    clippy::non_canonical_partial_ord_impl,
    clippy::should_implement_trait
)]
#![cfg_attr(not(feature = "std"), no_std)]
// compile-time checks
#[cfg(not(any(feature = "std", feature = "alloc")))]
compile_error! { "Either the `std` or `alloc` feature must be enabled." }
// external crates
#[cfg(feature = "alloc")]
extern crate alloc;
extern crate polymarket_client_sdk_v2 as polymarket;

// modules (public)
pub mod auth;
#[cfg(feature = "frame-identity")]
mod authenticated_frame_identity;
pub mod clob;
pub mod consts;
#[cfg(all(feature = "sqlx", feature = "postgres", feature = "json"))]
pub mod dead_letter;
pub mod error;
pub mod fees;
pub mod gamma;
pub mod retry;
#[cfg(feature = "watch")]
pub mod watch;
// modules (inline)
mod impls {
    mod impl_market_snapshot;
}

pub mod types {
    //! Types written to support communications with the [`polymarket`](https://docs.polymarket.com)
    //! api.
    #[doc(inline)]
    pub use self::prelude::*;

    mod address;
    mod aliases;
    mod book_snapshot;
    mod clob_side;
    mod clob_trade;
    mod fill;
    mod market;
    mod order;
    mod position;
    mod request;
    mod resolution;
    mod response;
    mod side;
    mod tick_size;
    mod u256;
    mod version;

    mod prelude {
        pub use super::address::*;
        pub use super::aliases::*;
        pub use super::book_snapshot::*;
        pub use super::clob_side::*;
        pub use super::clob_trade::*;
        pub use super::fill::*;
        pub use super::market::*;
        pub use super::order::*;
        pub use super::position::*;
        pub use super::resolution::*;
        pub use super::response::*;
        pub use super::side::*;
        pub use super::tick_size::*;
        pub use super::u256::*;
        pub use super::version::*;
    }
}

mod utils {
    #[doc(inline)]
    pub use self::prelude::*;

    mod hash;
    mod helpers;
    mod parse;
    mod serialize;
    mod time;
    mod venue;

    mod prelude {
        pub use super::hash::*;
        pub use super::helpers::*;
        pub use super::parse::*;
        #[cfg(feature = "serde")]
        pub use super::serialize::*;
        pub use super::time::*;
        pub use super::venue::*;
    }
}
// re-exports
#[cfg(feature = "frame-identity")]
#[doc(hidden)]
pub use self::authenticated_frame_identity::*;
#[cfg(feature = "clob")]
#[doc(inline)]
pub use self::clob::prelude::*;
#[cfg(all(feature = "sqlx", feature = "postgres", feature = "json"))]
#[doc(inline)]
pub use self::dead_letter::{ClobSubmitContext, DeadLetterRecord, record_dead_letter};
#[cfg(feature = "gamma")]
#[doc(inline)]
pub use self::gamma::GammaClient;
#[doc(inline)]
pub use self::utils::*;
#[cfg(feature = "watch")]
#[doc(inline)]
pub use self::watch::BookWatcher;
#[doc(inline)]
pub use self::{auth::prelude::*, consts::*, error::*, retry::*, types::*};
// prelude
#[doc(hidden)]
pub mod prelude {
    #[cfg(feature = "clob")]
    pub use crate::clob::prelude::*;
    pub use crate::consts::*;
    #[cfg(all(feature = "sqlx", feature = "postgres", feature = "json"))]
    pub use crate::dead_letter::*;
    #[cfg(feature = "gamma")]
    pub use crate::gamma::*;
    pub use crate::retry::*;
    pub use crate::types::*;
    #[cfg(feature = "sdk")]
    pub use crate::utils::*;
    #[cfg(feature = "watch")]
    pub use crate::watch::*;
}
