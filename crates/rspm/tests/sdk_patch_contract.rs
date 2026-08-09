//! Standalone dependency-boundary regression for the owned SDK patch.

const SDK_DEPENDENCY: &str =
    r#"polymarket_client_sdk_v2 = { path = "patches/polymarket_client_sdk_v2", version = "0.7" }"#;
const SDK_EXCLUSION: &str = r#"exclude = ["patches/polymarket_client_sdk_v2"]"#;
const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");
const README: &str = include_str!("../../../README.md");
const RSPM_MANIFEST: &str = include_str!("../Cargo.toml");
const RSPM_CLIENT: &str = include_str!("../src/clob/client.rs");
const SDK_CLIENT: &str =
    include_str!("../../../patches/polymarket_client_sdk_v2/src/clob/client.rs");

fn manifest_table<'a>(manifest: &'a str, header: &str) -> &'a str {
    let (_, remainder) = manifest
        .split_once(header)
        .expect("requested manifest table exists");
    remainder
        .split_once("\n[")
        .map_or(remainder, |(table, _)| table)
}

/// [REGRESSION] crates.io publication must stay disabled while RSPM calls an
/// SDK method supplied only by the owned path patch.
#[test]
fn patch_only_initial_post_contract_blocks_publication() {
    let package = manifest_table(RSPM_MANIFEST, "[package]");
    assert!(
        package.lines().any(|line| line == "publish = false"),
        "publishing would replace the path-owned SDK with registry 0.7, which lacks post_order_initial"
    );
    assert!(
        RSPM_CLIENT.contains("guard.post_order_initial(signed)"),
        "RSPM submission must call the patch-only initial-response primitive"
    );
}

/// [REGRESSION] The standalone workspace must select one excluded, canonical
/// vendored SDK source directly, and RSPM's POST future must stop at the
/// venue's initial response.
#[test]
fn standalone_sdk_patch_owns_the_initial_post_boundary() {
    let workspace = manifest_table(WORKSPACE_MANIFEST, "[workspace]");
    assert!(
        workspace.lines().any(|line| line == SDK_EXCLUSION),
        "the vendored SDK must be excluded from workspace auto-enrollment"
    );

    let workspace_dependencies = manifest_table(WORKSPACE_MANIFEST, "[workspace.dependencies]");
    assert!(
        workspace_dependencies
            .lines()
            .any(|line| line == SDK_DEPENDENCY),
        "the canonical SDK path and compatible version must be a direct workspace dependency"
    );
    assert_eq!(
        WORKSPACE_MANIFEST
            .lines()
            .filter(|line| { line.trim_start().starts_with("polymarket_client_sdk_v2 =") })
            .count(),
        1,
        "the standalone workspace must declare exactly one SDK source"
    );
    assert!(
        !WORKSPACE_MANIFEST.contains("[patch.crates-io]"),
        "the standalone workspace must not rely on a registry patch table"
    );
    assert!(
        RSPM_CLIENT.contains("guard.post_order_initial(signed)"),
        "RSPM submission must call the initial-response primitive"
    );

    let initial = SDK_CLIENT
        .split("pub async fn post_order_initial")
        .nth(1)
        .expect("vendored SDK exposes post_order_initial")
        .split("pub async fn post_order")
        .next()
        .expect("post_order follows the initial primitive");
    assert!(initial.contains("crate::request"));
    assert!(!initial.contains("resolve_transaction_hashes"));
    assert!(!initial.contains("defer_exec"));
}

/// [REGRESSION][EVAL] Standalone setup documentation must identify the same
/// excluded, direct path-backed SDK authority enforced by the manifests.
#[test]
fn readme_pins_the_direct_sdk_patch_topology() {
    assert!(
        README.contains("excludes `patches/polymarket_client_sdk_v2`"),
        "README must state that the vendored SDK is excluded from workspace membership"
    );
    assert!(
        README.lines().any(|line| line == SDK_DEPENDENCY),
        "README must pin the canonical direct SDK path and compatible version"
    );
    assert!(
        !README.contains("[patch.crates-io]"),
        "README must not claim the removed registry patch topology"
    );
}

/// [REGRESSION][EVAL] Publication-disabled RSPM documentation must use only
/// compatible path-backed dependency examples and make registry absence plain.
#[test]
fn readme_pins_path_only_rspm_consumption() {
    assert!(README.contains("RSPM is path-only while `publish = false`"));
    assert!(
        README.matches(r#"path = "crates/rspm""#).count() >= 2,
        "every standalone dependency example must select the local RSPM crate"
    );
    assert!(
        README.matches(r#"version = "0.0.0""#).count() >= 2,
        "every standalone dependency example must pin the compatible workspace version"
    );
    for forbidden in [
        r#"version = "0.0.x""#,
        "crates.io/crates/rspm",
        "docs.rs/rspm",
    ] {
        assert!(
            !README.contains(forbidden),
            "publication-disabled README retains registry claim `{forbidden}`"
        );
    }
}

/// [EVAL] The public SDK method must continue enriching ordinary callers while
/// the RSPM authority boundary uses the narrower primitive.
#[test]
fn initial_and_enriched_post_contracts_remain_distinct() {
    let enriched = SDK_CLIENT
        .split("pub async fn post_order(&self")
        .nth(1)
        .expect("vendored SDK exposes post_order")
        .split("pub async fn post_orders")
        .next()
        .expect("post_orders follows post_order");
    assert!(enriched.contains("self.post_order_initial(order).await?"));
    assert!(enriched.contains("resolve_transaction_hashes"));
}
