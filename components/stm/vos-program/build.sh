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
# with rust-src for the guests. With STEP=guest only the agent and the program are built, which needs no node. With
# STEP=node only the node is built. With PORTABLE=1 the node is built without native codegen (`T-010-08`, `G-37`):
# VOS's .cargo/config.toml pins target-cpu=native for x86_64 Linux, and RUSTFLAGS here replaces it with the baseline
# x86-64; the build fails if rustc still ran with target-cpu=native. The JIT stays on, since it is a runtime backend.
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
    if [ "${PORTABLE:-0}" = 1 ]; then
        # RUSTFLAGS replaces every rustflag of VOS's .cargo/config.toml, not only target-cpu=native. At the pin that
        # file has one for x86_64 Linux, `-C target-cpu=native`, and none under [build] (read through the GitHub
        # contents API at 5c78975). Keep it so: if the file gains any other flag, this build must carry it over.
        awk '/^\[/ { in_x86 = ($0 == "[target.x86_64-unknown-linux-gnu]"); in_build = ($0 == "[build]") }
             /^rustflags/ && in_build { bad = 1 }
             /^rustflags/ && in_x86 { n++; if ($0 != "rustflags = [\"-C\", \"target-cpu=native\"]") bad = 1 }
             END { exit (bad || n != 1) }' "$vos/.cargo/config.toml" || {
            echo "build.sh: VOS's .cargo/config.toml holds rustflags other than target-cpu=native for x86_64; carry them into RUSTFLAGS" >&2
            return 1
        }
        (cd "$vos" && RUSTFLAGS="-C target-cpu=x86-64" CARGO_TARGET_DIR="$out/vos-target" \
            cargo build -v --release --locked -p vos-cli --features faults) > "$out/node-build.log" 2>&1 ||
            { tail -30 "$out/node-build.log" >&2; return 1; }
        if grep -q 'target-cpu=native' "$out/node-build.log"; then
            echo "build.sh: PORTABLE=1 but rustc ran with target-cpu=native" >&2
            return 1
        fi
        grep -q 'target-cpu=x86-64' "$out/node-build.log" || {
            echo "build.sh: PORTABLE=1 but no rustc ran with target-cpu=x86-64" >&2
            return 1
        }
        echo "the node was built without native codegen (rustc ran with target-cpu=x86-64)" >> "$out/build-times.txt"
    else
        (cd "$vos" && CARGO_TARGET_DIR="$out/vos-target" cargo build --release --locked -p vos-cli --features faults)
    fi
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
if [ "${STEP:-all}" != node ]; then
    timed "the default agent" agent
    cp "$out/agent-target/riscv64em-vos/release/vos-agent.elf" "$out/elf/"
    timed "the program" program stm
    timed "the program with the suite's probe" program probe --features probe-machine
fi
if [ "${STEP:-all}" = node ]; then
    timed "the node" node
    cp "$out/vos-target/release/vos" "$out/bin/vos"
elif [ "${STEP:-all}" = all ]; then
    timed "the node" node
    cp "$out/vos-target/release/vos" "$out/bin/vos"
    timed "the client's tests" sh -c "cd '$here/client' && CARGO_TARGET_DIR='$out/client-target' \
        cargo test --release --locked --features vos --no-run"
fi
(cd "$out" && { sha256sum bin/vos elf/*.elf 2>/dev/null || shasum -a 256 bin/vos elf/*.elf; } > digests.txt) || true
cat "$out/build-times.txt"
echo "built into $out"
