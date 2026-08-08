#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# usage
usage() {
    cat <<EOF
Usage: $(basename "$0") [flags]

Steps:
  -b, --build      Run nix build
  -a, --all        Enable everything: --check --build

Flags:
  --no-dirty       Disallow a dirty workspace

  -h, --help       Show this message
EOF
}


# --- [STEPS] ---
DO_ALL=false
DO_BUILD=true

# --- [VARIABLES] ---
ENABLE_EXPERIMENTAL_FEATURES=true
ALLOW_DIRTY=true
OUTLINK=.nix

while [[ $# -gt 0 ]]; do
    case "$1" in
        -b|--build)                         DO_BUILD=true                         ;;
        --no-build)                         DO_BUILD=false                        ;;
        --dirty)                            ALLOW_DIRTY=true                      ;;
        --no-dirty)                         ALLOW_DIRTY=false                     ;;
        --disable-experimental-features)    ENABLE_EXPERIMENTAL_FEATURES=false    ;;
        --experimental)                     ENABLE_EXPERIMENTAL_FEATURES=true     ;;
        --disable-experimental-features)    ENABLE_EXPERIMENTAL_FEATURES=false    ;;
        -a|--all)                           DO_ALL=true                           ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown flag: $1" >&2; usage >&2; exit 1 ;;
    esac
    shift
done

NIX_FLAGS=()

$ENABLE_EXPERIMENTAL_FEATURES && NIX_FLAGS+=(--extra-experimental-features 'nix-command flakes')
$ALLOW_DIRTY && NIX_FLAGS+=(--allow-dirty)

step() { echo "→ $*"; }

if $DO_BUILD; then
  step "build"
  nix "${NIX_FLAGS[@]}" build  --out-link $OUTLINK "$@"
fi