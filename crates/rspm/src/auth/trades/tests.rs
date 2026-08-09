use super::*;
use crate::{auth::TERMINAL_CURSOR, canonical_nonnegative_decimal_text};

#[test]
fn trades_query_covers_every_public_filter_and_cursor() {
    let request = AuthenticatedTradesRequest {
        id: Some("trade-id".to_owned()),
        maker_address: Some(Address::ZERO),
        market: Some(B256::ZERO),
        asset_id: Some(U256::from(7)),
        before: Some(20),
        after: Some(10),
    };

    assert_eq!(
        request.query(Some("cursor==")).expect("bounded request"),
        concat!(
            "id=trade-id&",
            "maker_address=0x0000000000000000000000000000000000000000&",
            "market=0x0000000000000000000000000000000000000000000000000000000000000000&",
            "asset_id=7&before=20&after=10&next_cursor=cursor%3D%3D"
        )
    );
}

#[test]
fn negative_time_or_malformed_cursor_is_rejected() {
    assert!(
        AuthenticatedTradesRequest::default()
            .with_after(-1)
            .query(None)
            .is_err()
    );
    assert!(
        AuthenticatedTradesRequest::default()
            .query(Some("not a cursor"))
            .is_err()
    );
    for invalid_id in ["trade:maker", "trade id", "trade\nforged", "trädë"] {
        assert!(
            AuthenticatedTradesRequest {
                id: Some(invalid_id.to_owned()),
                ..AuthenticatedTradesRequest::default()
            }
            .query(None)
            .is_err()
        );
    }
}

/// [REGRESSION][EVAL] Delimiter-bearing components could make two
/// different venue tuples collapse to the same canonical execution key.
/// The shared closed alphabet rejects both collision shapes and every
/// whitespace/control variant before canonicalization or logging.
#[test]
fn execution_identity_components_reject_delimiter_collisions_and_controls() {
    let first = format!("{}:maker:{}", "trade", "maker:order");
    let second = format!("{}:maker:{}", "trade:maker", "order");
    assert_eq!(first, second, "fixture must demonstrate the old collision");
    for invalid in [
        "maker:order",
        "trade:maker",
        "trade order",
        "trade\torder",
        "trade\nforged-log",
        "trade\rforged-log",
        "trade/order",
        "trade?order",
        "trädë",
    ] {
        assert!(!venue_identifier_is_valid(invalid), "accepted {invalid:?}");
    }
    for valid in [
        "trade-id",
        "order_id",
        "0x0123456789abcdef",
        "550e8400-e29b-41d4-a716-446655440000",
    ] {
        assert!(venue_identifier_is_valid(valid), "rejected {valid}");
    }
}

/// [REGRESSION][EVAL] Post-decode identity validation attaches only the
/// exact numeric item/field path. Neither a nonzero index nor a nested
/// maker index may collapse to an unsafe `data[]` placeholder, and hostile
/// identity text remains unrepresentable in the error.
#[test]
fn response_identity_failures_are_exactly_indexed_and_redacted() {
    let maker_order = serde_json::json!({
        "order_id": "maker-order",
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "maker_address": "0x1111111111111111111111111111111111111111",
        "matched_amount": "1",
        "price": "0.5",
        "fee_rate_bps": "0",
        "asset_id": "1",
        "outcome": "NO",
        "side": "SELL"
    });
    let trade = serde_json::json!({
        "id": "trade-id",
        "taker_order_id": "taker-order",
        "market": format!("0x{}", "0".repeat(64)),
        "asset_id": "1",
        "side": "BUY",
        "size": "1",
        "fee_rate_bps": "0",
        "price": "0.5",
        "status": "CONFIRMED",
        "match_time": "1",
        "last_update": "1",
        "outcome": "YES",
        "bucket_index": 0,
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "maker_address": "0x0000000000000000000000000000000000000000",
        "maker_orders": [maker_order.clone(), maker_order],
        "trader_side": "TAKER"
    });
    let base = serde_json::json!({
        "data": [trade.clone(), trade],
        "next_cursor": "",
        "limit": 100,
        "count": 2
    });
    for (pointer, invalid, expected_path) in [
        ("/data/1/id", "trade:maker", "data[1].id"),
        (
            "/data/1/taker_order_id",
            "order\nforged",
            "data[1].taker_order_id",
        ),
        (
            "/data/1/maker_orders/1/order_id",
            "maker\rforged",
            "data[1].maker_orders[1].order_id",
        ),
    ] {
        let mut value = base.clone();
        *value.pointer_mut(pointer).expect("fixture pointer") = serde_json::json!(invalid);
        let body = serde_json::to_vec(&value).expect("fixture encoding");
        let error = AuthenticatedTradePage::decode_json(&body)
            .expect_err("invalid identity must reject complete page");
        assert_eq!(error.error_class(), "response_schema_decode");
        assert_eq!(error.response_path(), Some(expected_path));
        assert!(!format!("{error:?}").contains(invalid));
    }
}

#[test]
fn current_official_statuses_decode_and_unknown_status_rejects() {
    for (wire, expected) in [
        (
            "TRADE_STATUS_MATCHED_NOT_BROADCASTED",
            AuthenticatedTradeStatus::MatchedNotBroadcasted,
        ),
        ("TRADE_STATUS_MATCHED", AuthenticatedTradeStatus::Matched),
        ("TRADE_STATUS_MINED", AuthenticatedTradeStatus::Mined),
        (
            "TRADE_STATUS_CONFIRMED",
            AuthenticatedTradeStatus::Confirmed,
        ),
        ("TRADE_STATUS_RETRYING", AuthenticatedTradeStatus::Retrying),
        ("TRADE_STATUS_FAILED", AuthenticatedTradeStatus::Failed),
    ] {
        assert_eq!(
            serde_json::from_str::<AuthenticatedTradeStatus>(&format!("\"{wire}\""))
                .expect("documented status"),
            expected
        );
    }
    assert!(serde_json::from_str::<AuthenticatedTradeStatus>("\"TRADE_STATUS_NEW\"").is_err());
}

#[test]
fn canonical_decimal_lexeme_rejects_ambiguous_or_signed_text() {
    for accepted in ["0", "0.0", "1", "12.345", "999999999999999999"] {
        assert!(canonical_nonnegative_decimal_text(accepted), "{accepted}");
    }
    for rejected in ["", "-1", "+1", "1_0", "01", "00.1", ".1", "1.", "1e2"] {
        assert!(!canonical_nonnegative_decimal_text(rejected), "{rejected}");
    }
}

#[test]
fn page_invariants_accept_both_terminal_forms_and_reject_gaps() {
    for terminal in ["", TERMINAL_CURSOR] {
        let body = format!(r#"{{"data":[],"next_cursor":"{terminal}","limit":100,"count":0}}"#);
        AuthenticatedTradePage::decode_json(body.as_bytes()).expect("valid terminal page");
    }
    let populated = br#"{"data":[{"id":"trade","taker_order_id":"order","market":"0x0000000000000000000000000000000000000000000000000000000000000000","asset_id":"1","side":"BUY","size":"1","fee_rate_bps":"0","price":"0.5","status":"CONFIRMED","match_time":"1","last_update":"1","outcome":"NO","bucket_index":0,"owner":"ffffffff-ffff-ffff-ffff-ffffffffffff","maker_address":"0x0000000000000000000000000000000000000000","maker_orders":[],"trader_side":"TAKER"}],"next_cursor":"","limit":100,"count":1}"#;
    let populated = AuthenticatedTradePage::decode_json(populated)
        .expect("nonempty final page with empty cursor is terminal");
    assert_eq!(populated.data[0].outcome, "NO");
    assert_eq!(populated.data[0].side, AuthenticatedVenueSide::Buy);
    for body in [
        br#"{"data":[],"next_cursor":"MTAw","limit":100,"count":0}"#.as_slice(),
        br#"{"data":[],"next_cursor":"not a cursor","limit":100,"count":0}"#.as_slice(),
        br#"{"data":[],"next_cursor":"","limit":0,"count":0}"#.as_slice(),
        br#"{"data":[],"next_cursor":"","limit":100,"count":1}"#.as_slice(),
    ] {
        assert!(AuthenticatedTradePage::decode_json(body).is_err());
    }
}

/// Zero-valued execution economics are transport evidence, not a schema
/// failure. The engine must durably quarantine them before any positive
/// lifecycle validation so a later authenticated replay can enrich the
/// same immutable execution identity.
#[test]
fn zero_execution_economics_survive_authenticated_rest_decode() {
    let body = br#"{"data":[{"id":"trade","taker_order_id":"order","market":"0x0000000000000000000000000000000000000000000000000000000000000000","asset_id":"1","side":"BUY","size":"0","fee_rate_bps":"0","price":"0","status":"CONFIRMED","match_time":"1","last_update":"1","outcome":"YES","bucket_index":0,"owner":"ffffffff-ffff-ffff-ffff-ffffffffffff","maker_address":"0x0000000000000000000000000000000000000000","maker_orders":[{"order_id":"maker-order","owner":"ffffffff-ffff-ffff-ffff-ffffffffffff","maker_address":"0x1111111111111111111111111111111111111111","matched_amount":"0","price":"0","fee_rate_bps":"0","asset_id":"2","outcome":"NO","side":"SELL"}],"trader_side":"TAKER"}],"next_cursor":"","limit":100,"count":1}"#;
    let page = AuthenticatedTradePage::decode_json(body).expect("retain raw zero evidence");
    assert_eq!(page.data[0].size, Decimal::ZERO);
    assert_eq!(page.data[0].price, Decimal::ZERO);
    assert_eq!(page.data[0].maker_orders[0].matched_amount, Decimal::ZERO);
    assert_eq!(page.data[0].maker_orders[0].price, Decimal::ZERO);
}

/// [REGRESSION][EVAL] Newly introduced venue fields may carry execution
/// evidence. Until that evidence has an explicit typed contract, every
/// object boundary rejects it instead of silently advancing recovery. The
/// class stays `request_failed`, not `response_schema_decode`: an
/// unrecognized field name is exactly the unsafe-path case
/// `authenticated_response_path_is_safe` exists to redact (see
/// `authenticated_trade_diagnostic_accepts_only_owned_structural_paths` in
/// `error.rs`, whose own fixture includes `data[0].unknown_field`) — the
/// venue-controlled key text must never reach a rendered diagnostic.
#[test]
fn unknown_page_trade_or_maker_evidence_rejects_the_complete_page() {
    let maker = serde_json::json!({
        "order_id": "maker-order",
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "maker_address": "0x1111111111111111111111111111111111111111",
        "matched_amount": "1",
        "price": "0.5",
        "fee_rate_bps": "0",
        "asset_id": "2",
        "outcome": "NO",
        "side": "SELL"
    });
    let trade = serde_json::json!({
        "id": "trade",
        "taker_order_id": "order",
        "market": format!("0x{}", "0".repeat(64)),
        "asset_id": "1",
        "side": "BUY",
        "size": "1",
        "fee_rate_bps": "0",
        "price": "0.5",
        "status": "CONFIRMED",
        "match_time": "1",
        "last_update": "1",
        "outcome": "YES",
        "bucket_index": 0,
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "maker_address": "0x0000000000000000000000000000000000000000",
        "maker_orders": [maker],
        "trader_side": "TAKER"
    });
    let page = serde_json::json!({
        "data": [trade],
        "next_cursor": "",
        "limit": 100,
        "count": 1
    });

    for (pointer, field) in [
        ("", "new_page_evidence"),
        ("/data/0", "new_trade_evidence"),
        ("/data/0/maker_orders/0", "new_maker_evidence"),
    ] {
        let mut hostile = page.clone();
        hostile
            .pointer_mut(pointer)
            .expect("fixture object")
            .as_object_mut()
            .expect("fixture is object")
            .insert(field.to_owned(), serde_json::json!("must-not-be-shed"));
        let body = serde_json::to_vec(&hostile).expect("encode hostile page");
        let error = AuthenticatedTradePage::decode_json(&body)
            .expect_err("unknown execution evidence must reject the page");
        assert_eq!(error.error_class(), "request_failed");
        assert!(!format!("{error:?}").contains("must-not-be-shed"));
    }
}
