//! External compile and exact-type-identity contract for the temporary SDK export.
//!
//! Run each capability lane independently:
//! `cargo test -p rspm --no-default-features --features sdk --test sdk_reexport_contract`
//! `cargo test -p rspm --no-default-features --features clob --test sdk_reexport_contract`
//! `cargo test -p rspm --no-default-features --features clob,ws --test sdk_reexport_contract`
#![cfg(feature = "sdk")]

use static_assertions::assert_type_eq_all;

assert_type_eq_all!(
    rspm::polymarket::auth::Credentials,
    polymarket_client_sdk_v2::auth::Credentials,
);
assert_type_eq_all!(
    rspm::polymarket::error::Error,
    polymarket_client_sdk_v2::error::Error,
);
assert_type_eq_all!(
    rspm::polymarket::types::Address,
    polymarket_client_sdk_v2::types::Address,
);
assert_type_eq_all!(
    rspm::polymarket::types::Decimal,
    polymarket_client_sdk_v2::types::Decimal,
);
assert_type_eq_all!(
    rspm::polymarket::types::U256,
    polymarket_client_sdk_v2::types::U256,
);
assert_type_eq_all!(
    rspm::polymarket::Result<()>,
    polymarket_client_sdk_v2::Result<()>,
);

#[cfg(feature = "clob")]
assert_type_eq_all!(
    rspm::polymarket::clob::types::SignatureType,
    polymarket_client_sdk_v2::clob::types::SignatureType,
);
#[cfg(feature = "clob")]
assert_type_eq_all!(
    rspm::polymarket::clob::types::response::OrderBookSummaryResponse,
    polymarket_client_sdk_v2::clob::types::response::OrderBookSummaryResponse,
);
#[cfg(feature = "clob")]
assert_type_eq_all!(
    rspm::polymarket::clob::types::Side,
    polymarket_client_sdk_v2::clob::types::Side,
);
#[cfg(feature = "clob")]
assert_type_eq_all!(
    rspm::polymarket::clob::types::OrderStatusType,
    polymarket_client_sdk_v2::clob::types::OrderStatusType,
);
#[cfg(feature = "clob")]
assert_type_eq_all!(
    rspm::polymarket::clob::types::TraderSide,
    polymarket_client_sdk_v2::clob::types::TraderSide,
);

#[cfg(all(feature = "clob", feature = "ws"))]
assert_type_eq_all!(
    rspm::polymarket::clob::ws::BookUpdate,
    polymarket_client_sdk_v2::clob::ws::BookUpdate,
);
#[cfg(all(feature = "clob", feature = "ws"))]
assert_type_eq_all!(
    rspm::polymarket::clob::ws::Client,
    polymarket_client_sdk_v2::clob::ws::Client,
);
#[cfg(all(feature = "clob", feature = "ws"))]
assert_type_eq_all!(
    rspm::polymarket::clob::ws::WsError,
    rspm::polymarket::ws::WsError,
    polymarket_client_sdk_v2::ws::WsError,
);
#[cfg(all(feature = "clob", feature = "ws"))]
assert_type_eq_all!(
    rspm::polymarket::clob::ws::types::response::TradeMessageStatus,
    polymarket_client_sdk_v2::clob::ws::types::response::TradeMessageStatus,
);
#[cfg(all(feature = "clob", feature = "ws"))]
assert_type_eq_all!(
    rspm::polymarket::clob::ws::types::response::TradeMessage,
    polymarket_client_sdk_v2::clob::ws::types::response::TradeMessage,
);
#[cfg(all(feature = "clob", feature = "ws"))]
assert_type_eq_all!(
    rspm::polymarket::clob::ws::types::response::OrderMessage,
    polymarket_client_sdk_v2::clob::ws::types::response::OrderMessage,
);

const ADDRESS_VIA_RSPM: rspm::polymarket::types::Address =
    rspm::polymarket::types::address!("0x1111111111111111111111111111111111111111");
const ADDRESS_VIA_SDK: polymarket_client_sdk_v2::types::Address =
    polymarket_client_sdk_v2::types::address!("0x1111111111111111111111111111111111111111");

// This compile-only witness cannot execute because it requires live authority
// material. Its parameter and result annotations pin the owned RSPM boundary.
#[cfg(feature = "clob")]
#[allow(dead_code)]
async fn rspm_owned_boundaries_accept_and_return_exported_types(
    client: &rspm::ClobClient,
    config: rspm::ClobConfig,
    credentials: rspm::polymarket::auth::Credentials,
    signer: rspm::PrivateKeySigner,
    funder: Option<rspm::polymarket::types::Address>,
    signature_type: rspm::polymarket::clob::types::SignatureType,
) {
    let _authority: rspm::Result<rspm::ClobClient> =
        rspm::ClobClient::from_authenticated_authority(
            config,
            credentials,
            signer,
            funder,
            signature_type,
        )
        .await;
    let _midpoint: core::result::Result<
        rspm::polymarket::types::Decimal,
        rspm::ClobOperationError,
    > = client.midpoint("1").await;
    let _order_book: core::result::Result<
        rspm::polymarket::clob::types::response::OrderBookSummaryResponse,
        rspm::ClobOperationError,
    > = client.order_book("1").await;
}

/// [REGRESSION] The public export must expose the base SDK from an external
/// crate without changing type or macro identity.
#[test]
fn sdk_export_preserves_base_types_values_and_address_macro() {
    let address_from_rspm: polymarket_client_sdk_v2::types::Address = ADDRESS_VIA_RSPM;
    let address_round_trip: rspm::polymarket::types::Address = address_from_rspm;
    let config: Option<&polymarket_client_sdk_v2::ContractConfig> =
        rspm::polymarket::contract_config(rspm::polymarket::POLYGON, false);
    let rspm_error: rspm::Error =
        rspm::polymarket::error::Error::validation("identity contract").into();

    assert_eq!(address_round_trip, ADDRESS_VIA_SDK);
    assert_eq!(rspm::polymarket::POLYGON, polymarket_client_sdk_v2::POLYGON);
    assert!(config.is_some());
    assert!(matches!(rspm_error, rspm::Error::PolymarketError(_)));
}

/// [EVAL] Enabling only `clob` must expose the upstream CLOB vocabulary through
/// the same crate identity.
#[cfg(feature = "clob")]
#[test]
fn clob_export_preserves_sdk_type_identity() {
    let signature: polymarket_client_sdk_v2::clob::types::SignatureType =
        rspm::polymarket::clob::types::SignatureType::Proxy;
    let side: polymarket_client_sdk_v2::clob::types::Side =
        rspm::polymarket::clob::types::Side::Buy;
    let converted_side: rspm::polymarket::clob::types::Side = rspm::ClobSide::Buy.into();

    assert_eq!(
        signature,
        polymarket_client_sdk_v2::clob::types::SignatureType::Proxy
    );
    assert_eq!(side, polymarket_client_sdk_v2::clob::types::Side::Buy);
    assert_eq!(
        converted_side,
        polymarket_client_sdk_v2::clob::types::Side::Buy
    );
}

/// [EVAL] CLOB WebSocket types and the patch-owned connection counters must be
/// reachable through the public export when `clob,ws` is enabled.
#[cfg(all(feature = "clob", feature = "ws"))]
#[test]
fn websocket_export_preserves_sdk_type_identity() {
    let counters: [fn() -> usize; 4] = [
        rspm::polymarket::ws::connection::active_connected_sockets,
        rspm::polymarket::ws::connection::active_connection_tasks,
        rspm::polymarket::ws::connection::active_heartbeat_tasks,
        rspm::polymarket::ws::connection::active_reconnection_tasks,
    ];

    assert!(counters.into_iter().all(|counter| counter() == 0));
}
