/*
    Appellation: polymarket <tests>
    Contrib: @FL03
*/
use rspm::{Market, OrderReq, TickSize};

// ─── TickSize ─────────────────────────────────────────────────────────────────

#[test]
fn tick_size_default_is_cent() {
    assert_eq!(TickSize::default(), TickSize::Cent);
}

#[test]
fn tick_size_values() {
    assert!((TickSize::Cent.value() - 0.01).abs() < f64::EPSILON);
    assert!((TickSize::Millicent.value() - 0.001).abs() < f64::EPSILON);
}

#[test]
fn tick_size_round_cent() {
    let ts = TickSize::Cent;
    // 0.155 rounds to 0.16 (nearest cent)
    let rounded = ts.round(0.155);
    assert!((rounded - 0.16).abs() < 1e-10, "got {rounded}");
    // already on-tick
    assert!((ts.round(0.25) - 0.25).abs() < 1e-10);
}

#[test]
fn tick_size_round_millicent() {
    let ts = TickSize::Millicent;
    let rounded = ts.round(0.1234);
    assert!((rounded - 0.123).abs() < 1e-10, "got {rounded}");
}

#[test]
fn tick_size_is_valid() {
    let cent = TickSize::Cent;
    assert!(cent.is_valid(0.25));
    assert!(cent.is_valid(0.10));
    assert!(!cent.is_valid(0.255));

    let mc = TickSize::Millicent;
    assert!(mc.is_valid(0.123));
    assert!(!mc.is_valid(0.1234));
}

// ─── OrderReq ────────────────────────────────────────────────────────────────

#[test]
fn order_req_new_roundtrips() {
    use rspm::types::Side;
    let req = OrderReq::new("token_abc", 0.65, 100.0, Side::Yes);
    assert_eq!(req.token_id, "token_abc");
    assert!((req.price - 0.65).abs() < f64::EPSILON);
    assert!((req.size - 100.0).abs() < f64::EPSILON);
    assert_eq!(req.side, Side::Yes);
}

#[test]
fn order_req_sides_are_typed() {
    use rspm::types::Side;
    let buy = OrderReq::new("t", 0.5, 10.0, Side::Yes);
    assert!(buy.side.is_yes());

    let sell = OrderReq::new("t", 0.5, 10.0, Side::No);
    assert!(sell.side.is_no());
}

// ─── PolymarketQdbRow ────────────────────────────────────────────────────────

#[test]
fn market_default() {
    let m = Market::default();
    assert!(m.slug.is_empty());
    assert!(!m.active);
    assert!(!m.closed);
    assert!(m.end_date_iso.is_none());
}
