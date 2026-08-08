/*
    Appellation: ws <module>
    Created At: 2026.08.08:07:51:15
    Contrib: @FL03
*/
#[doc(inline)]
pub use self::prelude::*;

mod connection;
mod endpoint;
mod events;
mod frame;
mod retirement;
mod session;
mod state;
mod state_cell;
mod wire;

pub(crate) mod prelude {
    pub use super::connection::*;
    pub use super::endpoint::*;
    pub use super::events::*;
    pub use super::frame::*;
    pub use super::retirement::*;
    pub use super::session::*;
    pub use super::state::*;
    pub use super::state_cell::*;
    pub use super::wire::*;
}
