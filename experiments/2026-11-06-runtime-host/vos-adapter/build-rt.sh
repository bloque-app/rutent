#!/bin/sh
# Builds what the re-score's passes need, at VOS 5c78975 (the checkout VOS_TIP names, from fetch-vos.sh tip), into
# OUT, on the machine that runs them (VOS pins target-cpu=native for x86_64 Linux, so a binary is never moved to
# another CPU):
#   bin/vos          the scored node, built without VOS's faults feature
#   bin/vos-faults   the faults build, for R-01 and the hold_commits rows (H-02, H-06, H-11) only
#   elf/vos-agent.elf, and the rutent program three ways: as scored, `next` (R-06a) and `next-format` (R-06b)
#   rt-target/release/exp1-rt
# Usage: VOS_TIP=<checkout> build-rt.sh OUT
# Needs the toolchains VOS pins at the commit: nightly-2025-07-14 for the node and the client, nightly-2026-03-20
# with rust-src for the guests. Each step's wall time goes to OUT/build-times.txt.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
vos=$(cd "${VOS_TIP:?VOS_TIP names the checkout of VOS at 5c78975}" && pwd -P)
test "$(git -C "$vos" rev-parse HEAD)" = 5c7897582688ea93f8d02506bee8ea25da6b1458
mkdir -p "$1/bin" "$1/elf"
out=$(cd "$1" && pwd -P)
: > "$out/build-times.txt"
timed() {
    what=$1
    shift
    start=$(date +%s)
    "$@"
    echo "$what: $(( $(date +%s) - start )) s" >> "$out/build-times.txt"
}
node() {
    (cd "$vos" && CARGO_TARGET_DIR="$out/vos-target" cargo build --release --locked -p vos-cli "$@")
}
timed "the faults node" node --features faults
cp "$out/vos-target/release/vos" "$out/bin/vos-faults"
timed "the scored node" node
cp "$out/vos-target/release/vos" "$out/bin/vos"
agent() {
    (cd "$vos/agent/programs" && CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$out/agent-target" \
        cargo build --release --locked -p vos-agent --target ../../pvm/guest/riscv64em-vos.json)
}
timed "the default agent" agent
cp "$out/agent-target/riscv64em-vos/release/vos-agent.elf" "$out/elf/"
# The program and the client name VOS by its URL at the commit; this points cargo's fetch at the checkout.
export GIT_CONFIG_COUNT=1
export GIT_CONFIG_KEY_0="url.file://$vos.insteadOf"
export GIT_CONFIG_VALUE_0="https://github.com/virto-network/vos"
export CARGO_NET_GIT_FETCH_WITH_CLI=true
program() {
    name=$1
    shift
    (cd "$here/program" && CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$out/program-target-$name" \
        CARGO_TARGET_RISCV64EM_VOS_RUSTFLAGS="-Zunstable-options -Clink-arg=-T$vos/pvm/guest/pvm.ld" \
        cargo build --release --locked --target "$vos/pvm/guest/riscv64em-vos.json" -Zjson-target-spec \
        -Zbuild-std=core,alloc,compiler_builtins -Zbuild-std-features=compiler-builtins-mem "$@")
    cp "$out/program-target-$name/riscv64em-vos/release/rutent-vos-program.elf" "$out/elf/rutent-$name.elf"
}
timed "the program" program scored
timed "the program, next" program next --features next
timed "the program, next-format" program next-format --features next-format
rt() {
    (cd "$here/rt" && CARGO_TARGET_DIR="$out/rt-target" cargo build --release --locked --features vos)
}
timed "exp1-rt" rt
sha256sum "$out/bin/vos" "$out/bin/vos-faults" "$out/elf/"*.elf "$out/rt-target/release/exp1-rt" 2>/dev/null \
    > "$out/digests.txt" || shasum -a 256 "$out/bin/vos" "$out/bin/vos-faults" "$out/elf/"*.elf \
    "$out/rt-target/release/exp1-rt" > "$out/digests.txt"
cat "$out/build-times.txt"
echo "built into $out"
