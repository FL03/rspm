/*
    Appellation: clob <module>
    Created At: 2026.07.12:08:21:53
    Contrib: @FL03
*/
#![cfg(feature = "clob")]
#[doc(inline)]
pub use self::prelude::*;

pub mod client;
mod consts;
pub mod error;
mod fee_metadata;
mod operations;
mod positions;
mod settings;
mod submission;

mod utils {
    #[doc(inline)]
    pub use self::prelude::*;

    mod helpers;

    mod prelude {
        pub use super::helpers::*;
    }
}

pub(crate) mod prelude {
    pub use super::client::*;
    pub use super::consts::*;
    pub use super::error::*;
    pub use super::fee_metadata::*;
    pub use super::positions::*;
    pub use super::settings::*;
    pub use super::submission::*;
    pub use super::utils::*;
}
