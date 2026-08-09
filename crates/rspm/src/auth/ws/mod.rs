/*
    Appellation: ws <module>
    Created At: 2026.08.08:07:51:15
    Contrib: @FL03
*/
#[doc(inline)]
pub use self::prelude::*;

use self::client::{
    EVENT_CHANNEL_CAPACITY, MAX_DROPPED_RANGES, MAX_DURABLE_OUT_OF_ORDER, MAX_FRAME_BYTES,
    MAX_PENDING_RAW_FRAMES, RETIREMENT_RESPONSE_MARGIN, SESSION_SHUTDOWN_TIMEOUT,
    SOCKET_CLOSE_TIMEOUT,
};
use crate::auth::{
    AuthenticatedCredentialAuthority, AuthenticatedCredentialIdentity, AuthenticatedTraderSide,
    AuthenticatedVenueSide,
};

#[cfg(test)]
use futures::{SinkExt as _, StreamExt as _};
#[cfg(test)]
use polymarket::{
    auth::{ApiKey, Credentials},
    types::Decimal,
};
#[cfg(test)]
use std::{sync::Arc, time::Duration};
#[cfg(test)]
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
#[cfg(test)]
use tokio_tungstenite::tungstenite::Message;
mod client;
mod connection;
mod endpoint;
mod events;
mod frame;
mod retirement;
mod session;
mod state;
mod state_cell;
mod wire;

#[cfg(test)]
use self::{
    endpoint::{
        UserAuthentication, loopback_user_endpoint, official_user_endpoint, subscription_payload,
        user_endpoint,
    },
    retirement::{
        RetirementState, close_socket_within, drain_failed_retirements_for_test,
        process_custodian_transfer_count, retirement_custodian,
        transfer_to_disconnected_custodian_for_test, try_reserve_retirement_slot,
    },
    state_cell::StateCell,
};

pub(crate) mod prelude {
    pub use super::client::*;
    pub use super::events::*;
    pub use super::frame::*;
    pub use super::retirement::drain_authenticated_user_ws_retirements;
    pub use super::session::*;
    pub use super::state::*;
}

#[cfg(test)]
mod tests;
