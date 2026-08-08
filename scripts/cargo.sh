#!/usr/bin/env bash
# Sprint cleanup utility — run after clearing context between sprints.
# For builds, use: cargo xtask
#
# Defaults: fmt + check + clippy + fix + doc + build + update, with --locked.

set -euo pipefail
cd "$(dirname "$0")/.."

usage() {
    cat <<EOF
Usage: $(basename "$0") [flags]

Sprint-context reset utility. Formats, lints, optionally cleans and docs.
For builds use: cargo xtask

Steps (all default on except clean and test):
  --bench          Run cargo criterion
  -b, --build      Run cargo build
  -c, --clean      Run cargo clean (expensive; skipped by default)
  -d, --doc        Generate docs  (target/doc/axiom/all.html)
  -u, --update     Run cargo update
  -f, --fmt        Format the workspace
  -l, --lint       Run clippy
  -t, --test       Run tests
  -a, --all        Enable everything: --clean --doc --update --fmt --lint --test

  --no-bench   Skip the benchmarks
  --no-build   Skip the build step
  --no-check   Skip cargo check
  --no-clippy  Skip cargo clippy
  --no-fix     Skip cargo fix
  --no-fmt     Skip cargo fmt
  --no-update  Skip cargo update

Cargo resolution flags (applied to all package-resolution commands):
  --locked         Require Cargo.lock to be up-to-date (default)
  --unlocked       Disable --locked
  --offline        Disable network access (default)
  --online         Enable network access 
  --frozen         Require Cargo.lock + no network (supersedes --locked + --offline)
  --no-frozen      Disable --frozen

  -h, --help       Show this message
EOF
}

# Steps
DO_BENCH=true
DO_BUILD=true
DO_CHECK=true
DO_CLEAN=false
DO_CLIPPY=true
DO_DOC=true
DO_FMT=true
DO_FIX=true
DO_TEST=true
DO_UPDATE=true

# Cargo resolution flags — --locked on by default; --offline and --frozen off

ALL_TARGETS=true
DO_LOCKED=true
DO_OFFLINE=false
DO_FROZEN=false
DO_VERBOSE=false
DO_RELEASE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bench)       DO_BENCH=true     ;;
        --no-bench)    DO_BENCH=false    ;;
        -b|--build)    DO_BUILD=true     ;;
        --no-build)    DO_BUILD=false    ;;
        -c|--clean)    DO_CLEAN=true     ;;
        -d|--doc)      DO_DOC=true       ;;
        -u|--update)   DO_UPDATE=true    ;;
        --no-update)   DO_UPDATE=false   ;;
        -f|--fmt)      DO_FMT=true       ;;
        --no-fmt)      DO_FMT=false      ;;
        -l|--lint)     DO_CLIPPY=true    ;;
        --no-lint)     DO_CLIPPY=false   ;;
        -t|--test)     DO_TEST=true      ;;
        --check)       DO_CHECK=true     ;;
        --no-check)    DO_CHECK=false    ;;
        --clippy)      DO_CLIPPY=true    ;;
        --no-clippy)   DO_CLIPPY=false   ;;
        --fix)         DO_FIX=true       ;;
        --no-fix)      DO_FIX=false      ;;
        --locked)      DO_LOCKED=true    ;;
        --no-locked)   DO_LOCKED=false   ;;
        --offline)     DO_OFFLINE=true   ;;
        --no-offline)  DO_OFFLINE=false  ;;
        --frozen)      DO_FROZEN=true    ;;
        --no-frozen)   DO_FROZEN=false   ;;
        -r|--release)  DO_RELEASE=true   ;;
        --no-verbose)  DO_VERBOSE=false  ;;
        --verbose)     DO_VERBOSE=true   ;;
        --no-verbose)  DO_VERBOSE=false  ;;
        -a|--all)
          DO_CHECK=true
          DO_CLIPPY=true
          DO_FIX=true
          DO_CLEAN=true
          DO_BUILD=true
          DO_DOC=true
          DO_UPDATE=true
          DO_FMT=true
          DO_TEST=true
            ;;
        -h|--help) usage; exit 0        ;;
        *) echo "Unknown flag: $1" >&2; usage >&2; exit 1 ;;
    esac
    shift
done

# Build global flags for package-resolution commands (check, clippy, fix, doc, build, test).
# --frozen supersedes --locked + --offline; they are mutually exclusive in practice.
CARGO_FLAGS=()

$ALL_TARGETS && CARGO_FLAGS+=(--all-targets)
$DO_VERBOSE && CARGO_FLAGS+=(--verbose)
$DO_RELEASE && CARGO_FLAGS+=(--release)

if $DO_FROZEN; then
    CARGO_FLAGS+=(--frozen)
else
    $DO_LOCKED  && CARGO_FLAGS+=(--locked)
    $DO_OFFLINE && CARGO_FLAGS+=(--offline)
fi

step() { echo "→ cargo $*"; }

if $DO_CLEAN; then
    step "clean"
    cargo clean
fi

# cargo update intentionally skips CARGO_FLAGS — its purpose is to fetch new versions.
if $DO_UPDATE; then
    step "update"
    cargo update --verbose
fi

# cargo fmt does not support resolution flags.
if $DO_FMT; then
    step "fmt"
    cargo fmt --all
fi

if $DO_CHECK; then
    step "check"
    cargo check "${CARGO_FLAGS[@]}" --workspace --features full
fi

if $DO_CLIPPY; then
    step "clippy"
    cargo clippy "${CARGO_FLAGS[@]}" --allow-dirty --fix --workspace -- -D warnings
    cargo clippy "${CARGO_FLAGS[@]}" --allow-dirty --fix --workspace --features full -- -D warnings
fi

if $DO_FIX; then
    step "fix"
    cargo fix "${CARGO_FLAGS[@]}" --allow-dirty --workspace --features full
fi

if $DO_DOC; then
    step "doc"
    cargo doc --document-private-items --frozen --no-deps --workspace
    echo "   open target/doc/axiom/all.html"
fi

if $DO_BUILD; then
    step "build"
    cargo build "${CARGO_FLAGS[@]}" --workspace
    cargo build "${CARGO_FLAGS[@]}" --workspace --features full
fi

if $DO_BENCH; then
    step "bench"
    cargo criterion run --benches --no-fail-fast --workspace --features full
    echo "   open target/criterion/reports/index.html"
fi

if $DO_TEST; then
    step "test"
    cargo nextest run "${CARGO_FLAGS[@]}" --no-fail-fast --workspace
    cargo nextest run "${CARGO_FLAGS[@]}" --no-fail-fast --workspace --features full
fi

echo "✓ done"
