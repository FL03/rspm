use polymarket_client_sdk_v2::types::{Address, B256, U256};
use rspm::auth::{AuthenticatedTradePage, AuthenticatedTradeStatus, AuthenticatedTradesRequest};
use serde_json::{Value, json};

fn maker_order(fee_rate_bps: Option<Value>, order_id: &str) -> Value {
    let mut maker = json!({
        "order_id": order_id,
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "maker_address": "0x1111111111111111111111111111111111111111",
        "matched_amount": "4",
        "price": "0.4",
        "asset_id": "2",
        "outcome": "NO",
        "side": "SELL"
    });
    if let Some(value) = fee_rate_bps {
        maker
            .as_object_mut()
            .expect("maker fixture must be an object")
            .insert("fee_rate_bps".to_owned(), value);
    }
    maker
}

fn trade_page(makers: Vec<Value>) -> Value {
    json!({
        "data": [{
            "id": "trade-fee-contract",
            "taker_order_id": "counterparty-taker",
            "market": format!("0x{}", "3".repeat(64)),
            "asset_id": "1",
            "side": "BUY",
            "size": "4",
            "fee_rate_bps": "30",
            "price": "0.6",
            "status": "TRADE_STATUS_CONFIRMED",
            "match_time": "1700000000",
            "last_update": "1700000010",
            "outcome": "YES",
            "bucket_index": 0,
            "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "maker_address": "0x0000000000000000000000000000000000000000",
            "maker_orders": makers,
            "transaction_hash": "",
            "trader_side": "MAKER"
        }],
        "next_cursor": "LTE=",
        "limit": 100,
        "count": 1
    })
}

fn decode(page: Value) -> Result<AuthenticatedTradePage, rspm::auth::AuthenticatedEndpointError> {
    let body = serde_json::to_vec(&page).expect("fixture JSON must serialize");
    AuthenticatedTradePage::decode_json(&body)
}

fn decoded_fee(page: Value, maker_index: usize) -> Option<String> {
    decode(page)
        .expect("fixture must decode")
        .data
        .first()
        .expect("one trade")
        .maker_orders
        .get(maker_index)
        .expect("maker index")
        .fee_rate_bps
        .map(|fee| fee.to_string())
}

#[test]
fn null_maker_order_collection_remains_an_empty_page_leg_set() {
    let mut page = trade_page(Vec::new());
    page["data"][0]["maker_orders"] = Value::Null;
    let decoded = decode(page).expect("venue null maker collection is an empty collection");
    assert!(decoded.data[0].maker_orders.is_empty());
}

#[test]
fn current_official_trade_response_status_is_not_downgraded_to_unknown() {
    let decoded = decode(trade_page(Vec::new())).expect("official response fixture must decode");
    assert_eq!(decoded.data[0].status, AuthenticatedTradeStatus::Confirmed);
}

#[test]
fn blank_optional_fee_is_absence_not_zero() {
    let page = trade_page(vec![maker_order(Some(json!("")), "maker-blank")]);
    assert_eq!(decoded_fee(page, 0), None);
}

#[test]
fn absent_optional_fee_is_absence_not_zero() {
    let page = trade_page(vec![maker_order(None, "maker-absent")]);
    assert_eq!(decoded_fee(page, 0), None);
}

#[test]
fn integer_fee_text_remains_exact_decimal() {
    let page = trade_page(vec![maker_order(Some(json!("30")), "maker-integer")]);
    assert_eq!(decoded_fee(page, 0).as_deref(), Some("30"));
}

#[test]
fn quoted_zero_is_present_exact_fee_evidence() {
    let page = trade_page(vec![maker_order(Some(json!("0")), "maker-zero")]);
    assert_eq!(decoded_fee(page, 0).as_deref(), Some("0"));
}

#[test]
fn high_precision_fee_text_round_trips_exactly() {
    let exact = "12.34567890123456789012345678";
    let page = trade_page(vec![maker_order(Some(json!(exact)), "maker-precise")]);
    assert_eq!(decoded_fee(page, 0).as_deref(), Some(exact));
}

#[test]
fn malformed_nonempty_fee_rejects_the_entire_page() {
    let page = trade_page(vec![maker_order(
        Some(json!("not-a-decimal")),
        "maker-malformed",
    )]);
    let error = decode(page).expect_err("malformed text must reject the whole page");
    assert_eq!(
        error.response_path(),
        Some("data[0].maker_orders[0].fee_rate_bps")
    );
}

#[test]
fn noncanonical_decimal_lexemes_reject_the_entire_page() {
    for invalid in ["-1", "+1", "1_0", "01", "00.1", ".1", "1.", "1e2"] {
        let page = trade_page(vec![maker_order(
            Some(json!(invalid)),
            "maker-noncanonical",
        )]);
        let error = decode(page).expect_err("noncanonical fee text must reject");
        assert_eq!(
            error.response_path(),
            Some("data[0].maker_orders[0].fee_rate_bps"),
            "lexeme {invalid}"
        );
    }
}

#[test]
fn unrepresentable_precision_rejects_instead_of_rounding() {
    let page = trade_page(vec![maker_order(
        Some(json!("0.12345678901234567890123456789")),
        "maker-overprecision",
    )]);
    let error = decode(page).expect_err("unrepresentable exact fee must reject");
    assert_eq!(
        error.response_path(),
        Some("data[0].maker_orders[0].fee_rate_bps")
    );
}

#[test]
fn null_and_non_string_fee_values_reject_the_entire_page() {
    for invalid in [Value::Null, json!(30), json!(true), json!([]), json!({})] {
        let page = trade_page(vec![maker_order(Some(invalid), "maker-invalid-type")]);
        let error = decode(page).expect_err("non-string fee evidence must reject");
        assert_eq!(
            error.response_path(),
            Some("data[0].maker_orders[0].fee_rate_bps")
        );
    }
}

#[test]
fn mixed_blank_and_exact_fee_representations_decode_without_zero_coercion() {
    let page = trade_page(vec![
        maker_order(Some(json!("")), "maker-blank"),
        maker_order(Some(json!("0.125000")), "maker-exact"),
    ]);
    let decoded = decode(page).expect("allowed mixed page");
    assert_eq!(decoded.data[0].maker_orders[0].fee_rate_bps, None);
    assert_eq!(
        decoded.data[0].maker_orders[1]
            .fee_rate_bps
            .map(|fee| fee.to_string())
            .as_deref(),
        Some("0.125000")
    );
}

#[test]
fn recovery_retry_decodes_to_identical_exact_evidence() {
    let page = trade_page(vec![maker_order(
        Some(json!("0.000000000000000000000000001")),
        "maker-retry",
    )]);
    let first = decode(page.clone()).expect("first recovery decode");
    let retry = decode(page).expect("retry recovery decode");
    assert_eq!(first, retry);
    assert_ne!(first.data[0].maker_orders[0].fee_rate_bps, Some(0.into()));
}

#[test]
fn malformed_decode_error_never_contains_authenticated_payload_values() {
    let mut page = trade_page(vec![maker_order(
        Some(json!("secret-shaped-invalid-decimal")),
        "maker-redacted",
    )]);
    page["data"][0]["error_msg"] = json!("Bearer should-never-be-rendered");
    let rendered = decode(page)
        .expect_err("malformed fee must reject")
        .to_string();
    assert!(rendered.contains("data[0].maker_orders[0].fee_rate_bps"));
    assert!(!rendered.contains("Bearer"));
    assert!(!rendered.contains("should-never-be-rendered"));
    assert!(!rendered.contains("secret-shaped-invalid-decimal"));
}

#[test]
fn trailing_authenticated_payload_text_is_rejected_and_redacted() {
    let page = trade_page(vec![maker_order(Some(json!("30")), "maker-valid")]);
    let mut body = serde_json::to_vec(&page).expect("fixture JSON must serialize");
    body.extend_from_slice(b" Bearer should-never-be-rendered");

    let rendered = AuthenticatedTradePage::decode_json(&body)
        .expect_err("trailing payload text must reject the whole page")
        .to_string();
    assert!(!rendered.contains("Bearer"));
    assert!(!rendered.contains("should-never-be-rendered"));
}

#[test]
fn authenticated_trade_debug_is_structural_and_redacted() {
    let page = decode(trade_page(vec![maker_order(
        Some(json!("30")),
        "private-maker-order",
    )]))
    .expect("fixture decodes");
    let rendered = format!("{page:?} {:?}", page.data[0]);
    for private in [
        "trade-fee-contract",
        "counterparty-taker",
        "private-maker-order",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
        &"3".repeat(64),
    ] {
        assert!(!rendered.contains(private), "leaked {private}");
    }
}

#[test]
fn authenticated_trade_request_debug_redacts_every_identity_filter() {
    let request = AuthenticatedTradesRequest {
        id: Some("private-trade-id".to_owned()),
        maker_address: Some(Address::repeat_byte(0x11)),
        market: Some(B256::repeat_byte(0x22)),
        asset_id: Some(U256::from(123_456_u64)),
        before: Some(20),
        after: Some(10),
    };
    let rendered = format!("{request:?}");
    for private in [
        "private-trade-id",
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        "123456",
    ] {
        assert!(!rendered.contains(private), "leaked {private}");
    }
}

/// Price above the venue's [0, 1] probability bound is the only trade- or
/// maker-leg economics domain violation that decode itself rejects. Zero is
/// deliberately absent from this table: zero-valued size/price/matched-amount
/// are raw transport evidence the venue may legitimately emit, and rejecting
/// them at decode would defeat the durable-quarantine contract exercised by
/// `authenticated_trades::tests::zero_execution_economics_survive_authenticated_rest_decode`.
/// Negative lexemes are unrepresentable earlier, at the canonical-decimal-text
/// gate (`noncanonical_decimal_lexemes_reject_the_entire_page`), so they are
/// not repeated here either.
#[test]
fn out_of_range_trade_economics_reject_the_page() {
    for (pointer, value) in [
        ("/data/0/price", json!("1.000001")),
        ("/data/0/maker_orders/0/price", json!("1.000001")),
    ] {
        let mut page = trade_page(vec![maker_order(Some(json!("0")), "maker-domain")]);
        *page
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("fixture path {pointer}")) = value;
        assert!(decode(page).is_err(), "pointer {pointer}");
    }
}

#[test]
fn empty_or_oversized_trade_and_order_ids_reject_the_page() {
    for pointer in [
        "/data/0/id",
        "/data/0/taker_order_id",
        "/data/0/maker_orders/0/order_id",
    ] {
        for invalid in [String::new(), "x".repeat(513)] {
            let mut page = trade_page(vec![maker_order(Some(json!("0")), "maker-id")]);
            *page
                .pointer_mut(pointer)
                .unwrap_or_else(|| panic!("fixture path {pointer}")) = json!(invalid);
            assert!(decode(page).is_err(), "pointer {pointer}");
        }
    }
}
