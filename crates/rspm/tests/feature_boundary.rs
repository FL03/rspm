//! Compile-time contract for the SDK-independent market-primitives profile.
//!
//! Run with:
//! `cargo test -p rspm --no-default-features --features alloc,std --test feature_boundary`

use rspm::BookSnapshot;

const MANIFEST: &str = include_str!("../Cargo.toml");
const CLOB_CLIENT: &str = include_str!("../src/clob/client.rs");
const CLOB_TRADE: &str = include_str!("../src/types/clob_trade.rs");

#[test]
fn book_snapshot_does_not_require_the_external_sdk() {
    let book = BookSnapshot::new(0.40, 0.41, 0.59, 0.60, 7);

    assert!((book.spread() - 0.01_f64).abs() < f64::EPSILON);
    assert_eq!(book.timestamp(), 7);
}

#[test]
fn clob_websocket_paths_require_the_watch_capability() {
    let clob = MANIFEST
        .split_once("\nclob = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a clob feature block");
    let watch = MANIFEST
        .split_once("\nwatch = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a watch feature block");
    let ws = MANIFEST
        .split_once("\nws = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a ws feature block");

    assert!(
        !clob.contains("polymarket_client_sdk_v2/ws"),
        "clob must not activate the SDK websocket transport"
    );
    assert!(
        watch.contains("\"ws\""),
        "watch must activate the dedicated ws capability"
    );
    assert!(
        ws.contains("polymarket_client_sdk_v2?/ws"),
        "ws must own the SDK websocket transport"
    );
    assert!(
        CLOB_CLIENT.contains("#[cfg(feature = \"watch\")]\n    pub fn subscribe_market_trades"),
        "the public market-trade websocket method must require watch"
    );
    assert!(
        CLOB_TRADE.contains("#[cfg(feature = \"watch\")]\nimpl From<polymarket::clob::ws"),
        "the websocket event conversion must require watch"
    );
}

#[test]
fn external_sdk_is_an_explicit_rspm_client_capability() {
    let sdk = MANIFEST
        .split_once("\nsdk = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain an sdk feature block");
    let clob = MANIFEST
        .split_once("\nclob = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a clob feature block");
    let gamma = MANIFEST
        .split_once("\ngamma = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a gamma feature block");

    assert!(
        MANIFEST.contains("polymarket_client_sdk_v2 = { optional = true, workspace = true }"),
        "the external SDK dependency must remain optional"
    );
    assert!(
        sdk.contains("dep:polymarket_client_sdk_v2"),
        "sdk must activate the optional external dependency"
    );
    assert!(
        clob.contains("\"sdk\""),
        "the SDK-backed CLOB client must opt into the sdk capability explicitly"
    );
    assert!(
        !gamma.contains("\"sdk\"") && !gamma.contains("polymarket_client_sdk_v2"),
        "rspm's reqwest-based Gamma client and pure Gamma helpers must not pull in the SDK"
    );
}
