#!/usr/bin/env bash
# Builds the simulated providers (`T-024-01`): the sandboxed part of each of the four roles and the one outside part,
# each a component the platform admits as a signed release.
#
#   CARGO_TARGET_DIR=<a build directory for the guests only> ./build.sh <output directory>
#
# It needs: cargo and rustc with the wasm32-unknown-unknown target (rustup). It writes `sim-{role}-sandboxed.wasm`
# for each role and `sim-outside.wasm` to the output directory, and `built` there when it is done. The core modules
# are wrapped into components by the module guests' tool: $RUTENT_GUEST_TOOL names a built one, else it is built
# here. It downloads nothing beyond the crates `../Cargo.lock` pins.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
manifest="$here/../Cargo.toml"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR to a build directory for the guests only}"
out="${1:?usage: build.sh <output directory>}"
target="$CARGO_TARGET_DIR"
roles=(reserve exchange ramp verifier)

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
    cargo build --quiet --locked --manifest-path "$manifest" -p rutent-wasm-guests
    tool="$target/debug/rutent-wasm-guests"
fi

for role in "${roles[@]}"; do
    say "building the $role's sandboxed part"
    cargo build --quiet --release --locked --manifest-path "$manifest" -p rutent-sim-provider \
        --target wasm32-unknown-unknown --features "sandboxed,$role"
    "$tool" componentize "$target/wasm32-unknown-unknown/release/rutent_sim_provider.wasm" \
        "$out/sim-$role-sandboxed.wasm"
done

say "building the outside part"
cargo build --quiet --release --locked --manifest-path "$manifest" -p rutent-sim-provider \
    --target wasm32-unknown-unknown --features outside
"$tool" componentize "$target/wasm32-unknown-unknown/release/rutent_sim_provider.wasm" "$out/sim-outside.wasm"

touch "$out/built"
