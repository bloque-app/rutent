#!/usr/bin/env bash
# Builds the plain wallet (`T-016-02`): its two parts, each a component the platform admits as a signed release.
#
#   CARGO_TARGET_DIR=<a build directory for the guest only> ./build.sh <output directory>
#
# It needs: cargo and rustc with the wasm32-unknown-unknown target (rustup). It writes `plain-wallet-sandboxed.wasm`
# and `plain-wallet-outside.wasm` to the output directory, and `built` there when it is done. The core modules are
# wrapped into components by the module guests' tool (`../../module-guests/tool`): $RUTENT_GUEST_TOOL names a built
# one, else it is built here. It downloads nothing beyond the crates the two `Cargo.lock`s pin.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR to a build directory for the guest only}"
out="${1:?usage: build.sh <output directory>}"
target="$CARGO_TARGET_DIR"

say() { printf '==> %s\n' "$*" >&2; }

mkdir -p "$out"
rm -f "$out/built"

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
    echo "the wasm32-unknown-unknown target is missing: rustup target add wasm32-unknown-unknown" >&2
    exit 1
fi

tool="${RUTENT_GUEST_TOOL:-}"
if [[ -z "$tool" || ! -x "$tool" ]]; then
    say "building the module guests' tool"
    cargo build --quiet --locked --manifest-path "$here/../../module-guests/Cargo.toml" -p rutent-wasm-guests
    tool="$target/debug/rutent-wasm-guests"
fi

for part in sandboxed outside; do
    say "building the $part part"
    cargo build --quiet --release --locked --manifest-path "$here/Cargo.toml" \
        --target wasm32-unknown-unknown --features "$part"
    "$tool" componentize "$target/wasm32-unknown-unknown/release/rutent_plain_wallet.wasm" \
        "$out/plain-wallet-$part.wasm"
done

say "the plain wallet, by SHA-256"
"$tool" digest "$out"/plain-wallet-*.wasm | sed "s|$out/||" >&2
touch "$out/built"
