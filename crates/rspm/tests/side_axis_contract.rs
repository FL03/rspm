//! Outcome-token identity and CLOB trade action are orthogonal axes.
#![cfg(feature = "clob")]

use polymarket_client_sdk_v2::clob::types::Side as SdkSide;
use rspm::{ClobClient, ClobSide, OrderReq, Side};
use static_assertions::{assert_impl_all, assert_not_impl_any};

assert_impl_all!(ClobSide: Into<SdkSide>);
assert_not_impl_any!(Side: Into<ClobSide>, Into<SdkSide>);
assert_not_impl_any!(ClobSide: Into<Side>);
assert_not_impl_any!(SdkSide: Into<Side>);

const OUTCOME_SOURCE: &str = include_str!("../src/types/side.rs");

// A compile-only contract for the public submission boundary. This function is
// intentionally never executed because construction requires venue credentials.
#[allow(dead_code)]
async fn submit_fak_accepts_only_the_canonical_action(client: &ClobClient) {
    let _ = client
        .submit_fak("no-token", ClobSide::Buy, 0.5, 1.0, || async { None })
        .await;
}

/// [REGRESSION] No convenience API may claim that YES means BUY or NO means
/// SELL. Those equivalences made buying NO and selling YES unrepresentable.
#[test]
fn outcome_vocabulary_rejects_trade_actions() {
    for action in ["buy", "BUY", "sell", "SELL"] {
        assert!(
            Side::from_str(action).is_err(),
            "trade action must not parse as a market outcome: {action}"
        );
    }
    assert!(!OUTCOME_SOURCE.contains("pub fn is_buy"));
    assert!(!OUTCOME_SOURCE.contains("pub fn is_sell"));
}

/// [EVAL] The public request boundary must represent the complete Cartesian
/// product: either action can apply to either outcome token.
#[test]
fn every_trade_action_is_representable_for_every_outcome() {
    let mut observed = Vec::new();
    for (outcome, token_id) in [(Side::Yes, "yes-token"), (Side::No, "no-token")] {
        for action in [ClobSide::Buy, ClobSide::Sell] {
            let request = OrderReq::new(token_id, 0.5, 1.0, action);
            observed.push((outcome, request.side));
            assert_eq!(request.token_id, token_id);
        }
    }

    assert_eq!(
        observed,
        [
            (Side::Yes, ClobSide::Buy),
            (Side::Yes, ClobSide::Sell),
            (Side::No, ClobSide::Buy),
            (Side::No, ClobSide::Sell),
        ]
    );
}

#[test]
fn buy_no_token_emits_sdk_buy() {
    let request = OrderReq::new("no-token", 0.5, 1.0, ClobSide::Buy);
    assert_eq!(request.token_id, "no-token");
    assert_eq!(SdkSide::from(request.side), SdkSide::Buy);
}

#[test]
fn sell_yes_token_emits_sdk_sell() {
    let request = OrderReq::new("yes-token", 0.5, 1.0, ClobSide::Sell);
    assert_eq!(request.token_id, "yes-token");
    assert_eq!(SdkSide::from(request.side), SdkSide::Sell);
}

#[test]
fn sdk_bridge_preserves_only_the_trade_action_axis() {
    assert_eq!(SdkSide::from(ClobSide::Buy), SdkSide::Buy);
    assert_eq!(SdkSide::from(ClobSide::Sell), SdkSide::Sell);
}

#[cfg(feature = "json")]
#[test]
fn order_request_wire_shape_keeps_token_and_action_independent() {
    let buy_no = OrderReq::new("no-token", 0.5, 1.0, ClobSide::Buy);
    let encoded = serde_json::to_value(buy_no).expect("serialize request");
    assert_eq!(encoded["token_id"], "no-token");
    assert_eq!(encoded["side"], "BUY");

    let invalid = serde_json::json!({
        "token_id": "no-token",
        "price": 0.5,
        "size": 1.0,
        "side": "NO"
    });
    assert!(serde_json::from_value::<OrderReq>(invalid).is_err());
}
