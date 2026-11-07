#!/usr/bin/env bash
# Builds EXP-2's PolkaVM binding and stand-ins (build.sh), then runs the module-boundary suite on it and prints
# the report: `CARGO_TARGET_DIR=<dir> ./run.sh <run id>`. Works on x86_64 Linux, where PolkaVM compiles and runs
# each guest in its Linux sandbox, and on macOS, where it interprets.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR explicitly}"
RUN_ID=${1:-EXP-2-pvm-local}

"$HERE/build.sh"

echo "== the machine"
uname -a
if [ "$(uname -s)" = Linux ]; then
    grep -m1 'model name' /proc/cpuinfo || true
    for flag in bmi2 avx2; do grep -qw "$flag" /proc/cpuinfo && echo "cpu has $flag" || echo "cpu lacks $flag"; done
    for s in kernel.unprivileged_userns_clone user.max_user_namespaces vm.unprivileged_userfaultfd \
        kernel.apparmor_restrict_unprivileged_userns; do
        sysctl "$s" 2>/dev/null || echo "$s: not present"
    done
else
    sysctl -n machdep.cpu.brand_string 2>/dev/null || true
    sw_vers 2>/dev/null || true
fi
echo "open files: $(ulimit -n) -> raised for the workers' pipes"
ulimit -n 4096 2>/dev/null || ulimit -n "$(ulimit -Hn)"

echo "== isolation probes (B-08): the user's files, a signal, the network and a write, each refused"
# On Linux this is also the first run of the namespaces, the read-only roots, the rlimits and the admitting
# compiler's seccomp filter (isolation.rs); it needs util-linux's unshare and unprivileged user namespaces.
cargo test --release --locked --manifest-path "$HERE/Cargo.toml" --test isolation

echo "== the binding's tests, the suite's included"
RUTENT_PVM_GUESTS=$CARGO_TARGET_DIR/pvm-guests cargo test --release --locked --manifest-path "$HERE/Cargo.toml" \
    -- --include-ignored

echo "== the suite"
"$CARGO_TARGET_DIR/release/rutent-pvm-suite" --run-id "$RUN_ID" --guests "$CARGO_TARGET_DIR/pvm-guests" "${@:2}"
