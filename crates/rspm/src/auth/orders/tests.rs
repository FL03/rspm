use super::*;
use crate::auth::TERMINAL_CURSOR;

fn order(status: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "synthetic-order",
        "status": status,
        "owner": "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "maker_address": "0x2222222222222222222222222222222222222222",
        "market": format!("0x{}", "0".repeat(64)),
        "asset_id": "7",
        "side": "BUY",
        "original_size": "10.0",
        "size_matched": "2.5",
        "price": "0.45",
        "associate_trades": [],
        "outcome": "YES",
        "created_at": 1_705_322_096,
        "expiration": "1705708800",
        "order_type": "GTC"
    })
}

fn page(data: Vec<serde_json::Value>, count: u64, limit: u64, cursor: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "data": data,
        "limit": limit,
        "count": count,
        "next_cursor": cursor
    }))
    .expect("fixture")
}

#[test]
fn current_statuses_decode_and_unknown_rejects_page() {
    for status in [
        "ORDER_STATUS_LIVE",
        "ORDER_STATUS_INVALID",
        "ORDER_STATUS_CANCELED_MARKET_RESOLVED",
        "ORDER_STATUS_CANCELED",
        "ORDER_STATUS_MATCHED",
        "ORDER_STATUS_DELAYED",
        "ORDER_STATUS_UNMATCHED",
    ] {
        AuthenticatedOrderPage::decode_json(&page(vec![order(status)], 1, 100, TERMINAL_CURSOR))
            .expect("documented status");
    }
    let error = AuthenticatedOrderPage::decode_json(&page(
        vec![order("ORDER_STATUS_NEW")],
        1,
        100,
        TERMINAL_CURSOR,
    ))
    .expect_err("unknown status must reject page");
    assert_eq!(error.response_path(), Some("data[0].status"));
}

#[test]
fn delayed_and_unmatched_aliases_decode_atomically_as_complete_pages() {
    for status in [
        "ORDER_STATUS_DELAYED",
        "DELAYED",
        "delayed",
        "ORDER_STATUS_UNMATCHED",
        "UNMATCHED",
        "unmatched",
    ] {
        let decoded = AuthenticatedOrderPage::decode_json(&page(
            vec![order(status), order("ORDER_STATUS_LIVE")],
            2,
            100,
            TERMINAL_CURSOR,
        ))
        .expect("official nonterminal status must preserve the complete page");
        assert_eq!(decoded.data.len(), 2);
        assert!(matches!(
            decoded.data[0].status,
            AuthenticatedOrderStatus::Delayed | AuthenticatedOrderStatus::Unmatched
        ));
    }
}

#[test]
fn size_price_and_expiration_require_quoted_exact_values() {
    for field in ["original_size", "size_matched", "price", "expiration"] {
        let mut value = order("ORDER_STATUS_LIVE");
        value[field] = serde_json::json!(1);
        assert!(
            AuthenticatedOrderPage::decode_json(&page(vec![value], 1, 100, TERMINAL_CURSOR))
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn order_economics_reject_zero_negative_and_overfilled_values() {
    for (field, invalid) in [
        ("original_size", "0"),
        ("size_matched", "-1"),
        ("size_matched", "11"),
        ("price", "0"),
        ("price", "1.000001"),
    ] {
        let mut value = order("ORDER_STATUS_LIVE");
        value[field] = serde_json::json!(invalid);
        assert!(
            AuthenticatedOrderPage::decode_json(&page(vec![value], 1, 100, TERMINAL_CURSOR))
                .is_err(),
            "{field}={invalid}"
        );
    }
}

#[test]
fn page_invariants_fail_closed() {
    AuthenticatedOrderPage::decode_json(&page(vec![order("ORDER_STATUS_LIVE")], 1, 100, ""))
        .expect("nonempty final page with empty cursor is terminal");
    for body in [
        page(vec![order("ORDER_STATUS_LIVE")], 0, 100, TERMINAL_CURSOR),
        page(vec![order("ORDER_STATUS_LIVE")], 2, 100, TERMINAL_CURSOR),
        page(vec![order("ORDER_STATUS_LIVE")], 1, 0, TERMINAL_CURSOR),
        page(Vec::new(), 0, 100, "MTAw"),
        page(Vec::new(), 0, 100, "not a cursor"),
    ] {
        assert!(AuthenticatedOrderPage::decode_json(&body).is_err());
    }
}

#[test]
fn request_serializes_only_documented_filters() {
    let request = AuthenticatedOrdersRequest {
        order_id: Some("order-id".to_owned()),
        market: Some(B256::ZERO),
        asset_id: Some(U256::from(7)),
    };
    assert_eq!(
        request.query(Some("MTAw")).expect("bounded request"),
        concat!(
            "id=order-id&",
            "market=0x0000000000000000000000000000000000000000000000000000000000000000&",
            "asset_id=7&next_cursor=MTAw"
        )
    );
}

#[test]
fn authenticated_order_debug_redacts_identity_and_cursor_values() {
    let decoded = AuthenticatedOrderPage::decode_json(&page(
        vec![order("ORDER_STATUS_LIVE")],
        1,
        100,
        "cHJpdmF0ZQ==",
    ))
    .expect("valid nonterminal page");
    let request = AuthenticatedOrdersRequest {
        order_id: Some("private-order-filter".to_owned()),
        market: Some(B256::repeat_byte(0x22)),
        asset_id: Some(U256::from(123_456_u64)),
    };
    let rendered = format!("{decoded:?} {:?} {request:?}", decoded.data[0]);
    for private in [
        "synthetic-order",
        "cHJpdmF0ZQ==",
        "private-order-filter",
        "2222222222222222222222222222222222222222",
        "123456",
    ] {
        assert!(!rendered.contains(private), "leaked {private}");
    }
}

#[test]
fn terminal_order_path_is_validated_and_exact() {
    assert_eq!(
        authenticated_order_path("0xabc_DEF-123").expect("safe order id"),
        "/data/order/0xabc_DEF-123"
    );
    for rejected in [
        "",
        "../orders",
        "order/id",
        "order id",
        "order?id=1",
        "order:maker",
        "order\nforged",
        "ördër",
    ] {
        assert!(authenticated_order_path(rejected).is_err(), "{rejected}");
    }
}

#[test]
fn order_response_rejects_collision_and_control_id_without_echoing_it() {
    for invalid in ["order:maker", "order\tforged", "order\nforged", "ördër"] {
        let mut value = order("ORDER_STATUS_LIVE");
        value["id"] = serde_json::json!(invalid);
        let error =
            AuthenticatedOrderPage::decode_json(&page(vec![value], 1, 100, TERMINAL_CURSOR))
                .expect_err("invalid order identity must reject complete page");
        assert!(!format!("{error:?}").contains(invalid));
    }
}

#[test]
fn dedicated_order_decode_accepts_terminal_canceled_and_matched() {
    for status in [
        "ORDER_STATUS_CANCELED",
        "ORDER_STATUS_MATCHED",
        "ORDER_STATUS_DELAYED",
        "ORDER_STATUS_UNMATCHED",
    ] {
        AuthenticatedOrder::decode_json(&serde_json::to_vec(&order(status)).expect("fixture"))
            .expect("documented terminal order");
    }
}

/// [REGRESSION][EVAL] Account-order inventory is reconciliation evidence.
/// New page or order fields reject atomically until their retention
/// contract is explicit. The class stays `request_failed`, not
/// `response_schema_decode`: an unrecognized field name is exactly the
/// unsafe-path case `authenticated_response_path_is_safe` exists to redact
/// (see `authenticated_trade_diagnostic_accepts_only_owned_structural_paths`
/// in `error.rs`, whose own fixture includes `data[0].unknown_field`) — the
/// venue-controlled key text must never reach a rendered diagnostic.
#[test]
fn unknown_page_or_order_evidence_rejects_atomically() {
    let base_order = order("ORDER_STATUS_LIVE");
    let mut unknown_order = base_order.clone();
    unknown_order.as_object_mut().expect("order object").insert(
        "new_execution_evidence".to_owned(),
        serde_json::json!("must-not-be-shed"),
    );
    let order_error = AuthenticatedOrderPage::decode_json(&page(
        vec![base_order, unknown_order],
        2,
        100,
        TERMINAL_CURSOR,
    ))
    .expect_err("unknown nested order evidence must reject the complete page");
    assert_eq!(order_error.error_class(), "request_failed");
    assert!(!format!("{order_error:?}").contains("must-not-be-shed"));

    let hostile_page = serde_json::json!({
        "data": [order("ORDER_STATUS_LIVE")],
        "limit": 100,
        "count": 1,
        "next_cursor": TERMINAL_CURSOR,
        "new_page_evidence": "must-not-be-shed"
    });
    let page_error = AuthenticatedOrderPage::decode_json(
        &serde_json::to_vec(&hostile_page).expect("encode hostile page"),
    )
    .expect_err("unknown page evidence must reject the complete page");
    assert_eq!(page_error.error_class(), "request_failed");
    assert!(!format!("{page_error:?}").contains("must-not-be-shed"));
}
