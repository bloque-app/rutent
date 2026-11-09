#!/bin/sh
# Builds what the program's tests need into OUT, on the machine that runs them (VOS pins target-cpu=native for
# x86_64 Linux, so a binary is never moved to another CPU):
#   bin/vos                  the node, built with VOS's faults feature: the suite's stop after the program computed
#                            holds every node's commits, which only that build can
#   elf/vos-agent.elf        VOS's default agent, which `vos node run` needs
#   elf/stm.elf              this program, for VOS's guest target
#   elf/probe.elf            the same program built with the suite's probe in place of `Stm` (feature
#                            `probe-machine`), for the suite, which is typed on its probe
#   client-target/           the client's tests, built (not run)
# Usage: VOS_DIR=<checkout from fetch-vos.sh> build.sh OUT
# Toolchains, as VOS pins them at the commit: nightly-2025-07-14 for the node and the client, and nightly-2026-03-20
# with rust-src for the guests. With STEP=guest only the agent and the program are built, which needs no node.
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
. "$here/sources.sh"
sources_check
"$here/nostd.sh" check
vos=$(cd "${VOS_DIR:?VOS_DIR names the checkout from fetch-vos.sh}" && pwd -P)
test "$(git -C "$vos" rev-parse HEAD)" = "$(source_field vos 2)"
# The forks VOS's own lockfile gives are the ones the list pins.
for name in lsm-tree stwo s4; do
    grep -q "$(source_field "$name" 2)" "$vos/Cargo.lock" || {
        echo "build.sh: VOS's Cargo.lock does not pin $name at $(source_field "$name" 2)" >&2
        exit 1
    }
done
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
sources_git_config "$vos"
node() {
    (cd "$vos" && CARGO_TARGET_DIR="$out/vos-target" cargo build --release --locked -p vos-cli --features faults)
}
agent() {
    (cd "$vos/agent/programs" && CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$out/agent-target" \
        cargo build --release --locked -p vos-agent --target ../../pvm/guest/riscv64em-vos.json)
}
program() {
    name=$1
    shift
    (cd "$here" && CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$out/program-target-$name" \
        CARGO_TARGET_RISCV64EM_VOS_RUSTFLAGS="-Zunstable-options -Clink-arg=-T$vos/pvm/guest/pvm.ld" \
        cargo build --release --locked --target "$vos/pvm/guest/riscv64em-vos.json" -Zjson-target-spec \
        -Zbuild-std=core,alloc,compiler_builtins -Zbuild-std-features=compiler-builtins-mem "$@")
    cp "$out/program-target-$name/riscv64em-vos/release/rutent-stm-vos-program.elf" "$out/elf/$name.elf"
}
timed "the default agent" agent
cp "$out/agent-target/riscv64em-vos/release/vos-agent.elf" "$out/elf/"
timed "the program" program stm
timed "the program with the suite's probe" program probe --features probe-machine
if [ "${STEP:-all}" = all ]; then
    timed "the node" node
    cp "$out/vos-target/release/vos" "$out/bin/vos"
    timed "the client's tests" sh -c "cd '$here/client' && CARGO_TARGET_DIR='$out/client-target' \
        cargo test --release --locked --features vos --no-run"
fi
(cd "$out" && { sha256sum bin/vos elf/*.elf 2>/dev/null || shasum -a 256 bin/vos elf/*.elf; } > digests.txt) || true
cat "$out/build-times.txt"
echo "built into $out"
