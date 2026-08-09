//! Compile-time contract for the SDK-independent market-primitives profile.
//!
//! Run with:
//! `cargo test -p rspm --no-default-features --features alloc,std --test feature_boundary`

use rspm::{BookSnapshot, ClobSide, Market, MarketSnapshot, OrderType, Side, venue_min_bump};

const LIB: &str = include_str!("../src/lib.rs");
const MANIFEST: &str = include_str!("../Cargo.toml");
const AUTHENTICATED_FRAME: &str = include_str!("../src/auth/ws/frame.rs");
const CLOB_CLIENT: &str = include_str!("../src/clob/client.rs");
const CLOB_TRADE: &str = include_str!("../src/types/clob_trade.rs");

#[test]
fn market_primitives_do_not_require_the_external_sdk() {
    let book = BookSnapshot::new(0.40, 0.41, 0.59, 0.60, 7);
    let market = Market {
        slug: "btc-updown-5m".into(),
        question: "Will BTC close up?".into(),
        clob_token_ids: vec!["yes-token".into(), "no-token".into()],
        outcome_prices: vec!["0.40".into(), "0.60".into()],
        ..Market::default()
    };
    let snapshot = MarketSnapshot::from(market);

    assert!((book.spread() - 0.01_f64).abs() < f64::EPSILON);
    assert_eq!(book.timestamp(), 7);
    assert_eq!(snapshot.slug, "btc-updown-5m");
    assert_eq!(snapshot.token_ids, ["yes-token", "no-token"]);
    assert_eq!(Side::Yes.to_string(), "YES");
    assert_eq!(ClobSide::Sell.to_string(), "SELL");
    assert_eq!(OrderType::FakTaker.as_wire_str(), "FAK");
    assert_eq!(venue_min_bump(2.0, 5.0, 0.10, 0.05), Some(5.0));
}

/// [REGRESSION][EVAL] Client modules and the public SDK compatibility export
/// must remain behind the capability that supplies their dependencies and
/// behavior.
#[test]
fn client_modules_are_gated_by_their_owning_capabilities() {
    for (capability, declaration) in [
        (
            "sdk",
            "#[cfg(feature = \"sdk\")]\npub extern crate polymarket_client_sdk_v2 as polymarket;",
        ),
        ("clob", "#[cfg(feature = \"clob\")]\npub mod auth;"),
        ("clob", "#[cfg(feature = \"clob\")]\npub mod clob;"),
        ("gamma", "#[cfg(feature = \"gamma\")]\npub mod gamma;"),
        ("clob", "#[cfg(feature = \"clob\")]\npub mod retry;"),
    ] {
        assert!(
            LIB.contains(declaration),
            "{capability} must own declaration `{declaration}`"
        );
    }

    assert!(
        !LIB.contains("pub use crate::polymarket"),
        "the transitional SDK crate export must stay out of rspm::prelude"
    );
}

/// [REGRESSION][EVAL] Credential and protocol-identity hashing belongs to
/// CLOB, while Axiom evidence-key and payload-digest composition stays out of
/// RSPM's authenticated raw-frame surface.
#[test]
fn clob_owns_transport_hashing_without_private_frame_policy() {
    let clob = MANIFEST
        .split_once("\nclob = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a clob feature block");
    assert!(
        clob.contains("\"sha2\""),
        "clob must activate its direct credential and protocol-identity hashing dependency"
    );

    for profile in ["alloc", "std", "sdk", "gamma"] {
        let body = MANIFEST
            .split_once(&format!("\n{profile} = ["))
            .and_then(|(_, suffix)| suffix.split_once("\n]"))
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("rspm must retain a {profile} feature block"));
        assert!(
            !body.contains("\"sha2\"") && !body.contains("frame-identity"),
            "the {profile} boundary must not activate CLOB hashing or removed frame policy"
        );
    }

    for (surface, source) in [
        ("manifest", MANIFEST),
        ("crate root", LIB),
        ("authenticated frame", AUTHENTICATED_FRAME),
    ] {
        for removed in [
            "frame-identity",
            "authenticated_frame_identity",
            "AuthenticatedPrivateFrameIdentity",
            "authenticated_private_frame_payload_digest_v1",
            "private_frame_evidence_key_v1",
        ] {
            assert!(
                !source.contains(removed),
                "{surface} retains removed private-frame policy `{removed}`"
            );
        }
    }
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
    let alloc = MANIFEST
        .split_once("\nalloc = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain an alloc feature block");
    let std = MANIFEST
        .split_once("\nstd = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a std feature block");
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
    let tracing = MANIFEST
        .split_once("\ntracing = [")
        .and_then(|(_, suffix)| suffix.split_once("\n]"))
        .map(|(body, _)| body)
        .expect("rspm must retain a tracing feature block");

    assert!(
        MANIFEST.contains("polymarket_client_sdk_v2 = { optional = true, workspace = true }"),
        "the external SDK dependency must remain optional"
    );
    assert!(
        std.contains("\"alloc\""),
        "std must include the alloc primitives required by every SDK lane"
    );
    for (profile, body) in [("alloc", alloc), ("std", std)] {
        assert!(
            !body.contains("\"sdk\"") && !body.contains("polymarket_client_sdk_v2"),
            "the {profile} primitive profile must remain SDK-free"
        );
    }
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
    assert!(
        tracing.contains("polymarket_client_sdk_v2?/tracing")
            && !tracing.contains("dep:polymarket_client_sdk_v2"),
        "tracing must weak-forward SDK telemetry without activating the optional SDK"
    );
}
