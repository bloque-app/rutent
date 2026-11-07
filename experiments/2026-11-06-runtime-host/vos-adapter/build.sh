#!/bin/sh
# Builds what the first run's VOS passes need, at VOS 1aadf44 (the checkout VOS_PIN names, from fetch-vos.sh pin),
# into OUT: the vos binary with VOS's fault injection (a test build), VOS's default agent program, the journal actor
# and the adapter's runner, exp1-vos. The re-score's R-07a and R-07b run this build's export-file pass.
# Usage: VOS_PIN=<checkout> build.sh OUT
# Needs the toolchains VOS pins at the commit: nightly-2025-07-14 for the node and the adapter, nightly-2026-03-20
# with rust-src for the guests.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
vos=$(cd "${VOS_PIN:?VOS_PIN names the checkout of VOS at 1aadf44}" && pwd -P)
test "$(git -C "$vos" rev-parse HEAD)" = 1aadf44fa2cb1eaedc015ee82343ac118cc89f1e
mkdir -p "$1"
out=$(cd "$1" && pwd -P)
# The node, with the faults VOS_FAULTS names. Its own workspace, toolchain and lockfile.
(cd "$vos" && CARGO_TARGET_DIR="$out/vos-target" cargo build --release -p vos-cli --features faults)
# VOS's default agent program, which `vos node run` loads.
(cd "$vos/agent/programs" && CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$out/agent-target" \
    cargo build --release -p vos-agent --target ../../pvm/guest/riscv64em-vos.json)
# The journal and the adapter name VOS by its URL at the commit; this points cargo's fetch at the checkout.
export GIT_CONFIG_COUNT=1
export GIT_CONFIG_KEY_0="url.file://$vos.insteadOf"
export GIT_CONFIG_VALUE_0="https://github.com/virto-network/vos"
export CARGO_NET_GIT_FETCH_WITH_CLI=true
# The guest's linker script is the checkout's; cargo's config can't name a variable, so it goes on the command line.
(cd "$here/journal" && CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$out/journal-target" \
    CARGO_TARGET_RISCV64EM_VOS_RUSTFLAGS="-Zunstable-options -Clink-arg=-T$vos/pvm/guest/pvm.ld" \
    cargo build --release --locked --target "$vos/pvm/guest/riscv64em-vos.json")
# The adapter behind its flag, the `vos` feature.
(cd "$here/adapter" && CARGO_TARGET_DIR="$out/adapter-target" cargo build --release --locked --features vos)
echo "built into $out"
