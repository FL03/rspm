//! Standalone dependency-boundary regression for the owned SDK patch.

const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");
const RSPM_CLIENT: &str = include_str!("../src/clob/client.rs");
const SDK_CLIENT: &str =
    include_str!("../../../patches/polymarket_client_sdk_v2/src/clob/client.rs");

/// [REGRESSION] The security boundary must compile from an RSPM clone without
/// relying on Axiom's root `[patch]`, and its POST future must stop at the
/// venue's initial response.
#[test]
fn standalone_sdk_patch_owns_the_initial_post_boundary() {
    assert!(
        WORKSPACE_MANIFEST
            .contains("polymarket_client_sdk_v2 = { path = \"patches/polymarket_client_sdk_v2\" }"),
        "the standalone workspace must activate its vendored SDK patch"
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
