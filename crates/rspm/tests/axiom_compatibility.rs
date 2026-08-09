//! Downstream compatibility gate for Axiom's authenticated RSPM boundary.

fn type_is_public<T>() {}

#[cfg(feature = "gamma")]
#[test]
fn gamma_base_remains_available_at_the_crate_root() {
    assert_eq!(rspm::GAMMA_BASE, "https://gamma-api.polymarket.com");
}

#[test]
fn venue_constraint_facade_remains_available() {
    assert_eq!(rspm::venue::venue_min_bump(2.0, 5.0, 0.1, 0.05), Some(5.0));
}

#[cfg(feature = "clob")]
#[test]
fn authority_types_remain_available_at_the_crate_root() {
    type_is_public::<rspm::AuthenticatedCredentialAuthority>();
    type_is_public::<rspm::AuthenticatedCredentialIdentity>();
    type_is_public::<rspm::AuthenticatedProtocolAuthority>();
    type_is_public::<rspm::AuthenticatedProtocolCacheIdentity>();
}

#[cfg(feature = "clob")]
#[test]
fn authenticated_rest_types_remain_available_under_clob() {
    type_is_public::<rspm::clob::AuthenticatedBalanceSnapshot>();
    type_is_public::<rspm::clob::AuthenticatedOrder>();
    type_is_public::<rspm::clob::AuthenticatedOrderPage>();
    type_is_public::<rspm::clob::AuthenticatedOrderStatus>();
    type_is_public::<rspm::clob::AuthenticatedOrdersRequest>();
    type_is_public::<rspm::clob::AuthenticatedTrade>();
    type_is_public::<rspm::clob::AuthenticatedTradePage>();
    type_is_public::<rspm::clob::AuthenticatedTradeStatus>();
    type_is_public::<rspm::clob::AuthenticatedTraderSide>();
    type_is_public::<rspm::clob::AuthenticatedTradesRequest>();
    type_is_public::<rspm::clob::AuthenticatedVenueSide>();
}

#[cfg(all(feature = "clob", feature = "ws"))]
#[test]
fn authenticated_websocket_types_remain_available_under_clob() {
    type_is_public::<rspm::clob::AuthenticatedUserAuthenticationState>();
    type_is_public::<rspm::clob::AuthenticatedUserConnectionState>();
    type_is_public::<rspm::clob::AuthenticatedUserEvent>();
    type_is_public::<rspm::clob::AuthenticatedUserEventBatch>();
    type_is_public::<rspm::clob::AuthenticatedUserFrameEncoding>();
    type_is_public::<rspm::clob::AuthenticatedUserFrameGap>();
    type_is_public::<rspm::clob::AuthenticatedUserOrder>();
    type_is_public::<rspm::clob::AuthenticatedUserOrderStatus>();
    type_is_public::<rspm::clob::AuthenticatedUserOrderType>();
    type_is_public::<rspm::clob::AuthenticatedUserRawFrame>();
    type_is_public::<rspm::clob::AuthenticatedUserRecoveryToken>();
    type_is_public::<rspm::clob::AuthenticatedUserSubscriptionState>();
    type_is_public::<rspm::clob::AuthenticatedUserTrade>();
    type_is_public::<rspm::clob::AuthenticatedUserTradeStatus>();
    type_is_public::<rspm::clob::AuthenticatedUserWs>();
    type_is_public::<rspm::clob::AuthenticatedUserWsConfig>();
    type_is_public::<rspm::clob::AuthenticatedUserWsState>();
}

#[test]
fn outcome_and_order_action_types_remain_distinct_at_the_crate_root() {
    type_is_public::<rspm::Side>();
    type_is_public::<rspm::ClobSide>();
    type_is_public::<rspm::OrderReq>();

    let request = rspm::OrderReq::new("no-token", 0.5, 1.0, rspm::ClobSide::Buy);
    let action: rspm::ClobSide = request.side;
    assert_eq!(action, rspm::ClobSide::Buy);
}

/// [REGRESSION][EVAL] Downstream policy handling must remain exhaustive so a
/// new order policy cannot acquire an implicit execution fallback.
#[test]
fn order_type_root_export_remains_exhaustive_with_canonical_wire_tags() {
    for (policy, expected) in [
        (rspm::OrderType::GtcMaker, "GTC"),
        (rspm::OrderType::FakTaker, "FAK"),
        (rspm::OrderType::Fok, "FOK"),
    ] {
        let wire = match policy {
            rspm::OrderType::GtcMaker => "GTC",
            rspm::OrderType::FakTaker => "FAK",
            rspm::OrderType::Fok => "FOK",
        };
        assert_eq!(wire, expected);
        assert_eq!(policy.as_wire_str(), expected);
    }
}
