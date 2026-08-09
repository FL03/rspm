//! Downstream compatibility gate for Axiom's authenticated RSPM boundary.

fn type_is_public<T>() {}

#[test]
fn authority_types_remain_available_at_the_crate_root() {
    type_is_public::<rspm::AuthenticatedCredentialAuthority>();
    type_is_public::<rspm::AuthenticatedCredentialIdentity>();
    type_is_public::<rspm::AuthenticatedProtocolAuthority>();
    type_is_public::<rspm::AuthenticatedProtocolCacheIdentity>();
}

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

#[cfg(feature = "ws")]
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
