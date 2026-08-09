//! Canonical standalone workspace architecture contract.

use std::path::Path;

const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");
const WORKSPACE_LOCK: &str = include_str!("../../../Cargo.lock");
const REMOVED_PLACEHOLDERS: [&str; 4] = ["core", "types", "gamma", "clob"];

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("rspm manifest must remain at crates/rspm/Cargo.toml")
}

/// [REGRESSION][EVAL] RSPM remains the sole API-compatible production package.
/// Empty generated packages cannot silently become accepted architecture merely
/// because a glob lists them as workspace members.
#[test]
fn workspace_contains_only_the_canonical_rspm_package() {
    let crates = workspace_root().join("crates");
    let mut packages = std::fs::read_dir(&crates)
        .expect("read workspace crates directory")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().join("Cargo.toml").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    packages.sort();

    assert_eq!(packages, ["rspm"], "unexpected workspace package topology");
    assert!(WORKSPACE_MANIFEST.contains("default-members = [\n  \"crates/rspm\",\n]"));
}

#[test]
fn removed_placeholder_packages_leave_no_source_or_lock_identity() {
    for name in REMOVED_PLACEHOLDERS {
        let package = format!("rspm-{name}");
        assert!(
            !workspace_root().join("crates").join(name).exists(),
            "placeholder directory returned: crates/{name}"
        );
        assert!(
            !WORKSPACE_LOCK.contains(&format!("name = \"{package}\"")),
            "placeholder lock package returned: {package}"
        );
    }
}
