//! Compile-time contract for the SDK-independent market-primitives profile.
//!
//! Run with:
//! `cargo test -p rspm --no-default-features --features alloc,std --test feature_boundary`

use rspm::BookSnapshot;

const MANIFEST: &str = include_str!("../Cargo.toml");
const CLOB_CLIENT: &str = include_str!("../src/clob/client.rs");
const CLOB_TRADE: &str = include_str!("../src/types/clob_trade.rs");
const CMP_DRIFT_MANIFEST: &str = include_str!("../../../cmp/drift/Cargo.toml");
const CMP_LAG_MANIFEST: &str = include_str!("../../../cmp/lag/Cargo.toml");
const CMP_MAKR_MANIFEST: &str = include_str!("../../../cmp/makr/Cargo.toml");
const CMP_QUAD_MANIFEST: &str = include_str!("../../../cmp/quad/Cargo.toml");
const CMP_STUB_MANIFEST: &str = include_str!("../../../cmp/svc-stub/Cargo.toml");

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

#[test]
fn cmp_rspm_profiles_do_not_activate_the_external_sdk() {
    for (name, manifest) in [
        ("cmp/lag", CMP_LAG_MANIFEST),
        ("cmp/makr", CMP_MAKR_MANIFEST),
    ] {
        let rspm_line = manifest
            .lines()
            .find(|line| line.trim_start().starts_with("rspm ="))
            .unwrap_or_else(|| panic!("{name} must declare its rspm capability explicitly"));
        for forbidden in [
            "\"sdk\"",
            "\"clob\"",
            "\"ws\"",
            "\"gamma\"",
            "\"reqwest\"",
            "\"async\"",
        ] {
            assert!(
                !rspm_line.contains(forbidden),
                "{name} must remain data-plane-free, but its rspm profile contains {forbidden}: \
                 {rspm_line}"
            );
        }
    }
}

#[test]
fn cmp_strategy_manifests_have_no_data_plane_dependencies() {
    for (name, manifest) in [
        ("cmp/drift", CMP_DRIFT_MANIFEST),
        ("cmp/lag", CMP_LAG_MANIFEST),
        ("cmp/makr", CMP_MAKR_MANIFEST),
        ("cmp/quad", CMP_QUAD_MANIFEST),
        ("cmp/svc-stub", CMP_STUB_MANIFEST),
    ] {
        let dependencies = manifest
            .split_once("[dependencies]")
            .and_then(|(_, suffix)| suffix.split_once("\n[").map(|(section, _)| section))
            .unwrap_or_else(|| panic!("{name} must have a dependencies table"));

        for forbidden in [
            "reqwest",
            "hyper",
            "ureq",
            "tungstenite",
            "tokio",
            "sqlx",
            "rusqlite",
            "questdb",
            "polymarket_client_sdk_v2",
        ] {
            assert!(
                !dependencies
                    .lines()
                    .any(|line| line.trim_start().starts_with(&format!("{forbidden} ="))),
                "{name} strategy model must not declare data-plane dependency {forbidden}"
            );
        }
    }
}

#[test]
fn cmp_strategy_sources_have_no_data_plane_calls() {
    let cmp_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cmp");
    let forbidden = [
        "reqwest::",
        "hyper::",
        "ureq::",
        "tungstenite::",
        "tokio::net",
        "std::net::",
        "sqlx::",
        "rusqlite::",
        "polymarket_client_sdk_v2::",
    ];

    let mut pending = vec![cmp_root];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        {
            let entry = entry.expect("read cmp source entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
                for needle in forbidden {
                    assert!(
                        !source.contains(needle),
                        "{} strategy source must not contain data-plane call {needle}",
                        path.display()
                    );
                }
            }
        }
    }
}
