#!/bin/sh
# Runs every pass of an EXP-1 VOS run with what build.sh built in BUILD, writing each pass's report to
# OUT/<pass>.md and its stores under OUT/<n>, the pass's number. OUT must be new. Keep OUT's path short: each store
# holds the node's API socket, and a unix socket's path is limited (under 104 bytes on macOS).
# Usage: run.sh RUN_ID BUILD OUT
set -eu
id=$1
build=$(cd "$2" && pwd -P)
out=$3
if [ -e "$out" ]; then
    echo "run.sh: $out exists" >&2
    exit 1
fi
mkdir -p "$out"
export VOS_BIN="$build/vos-target/release/vos"
export VOS_AGENT_ELF="$build/agent-target/riscv64em-vos/release/vos-agent.elf"
export JOURNAL_ELF="$build/journal-target/riscv64em-vos/release/rutent-vos-journal.elf"
n=0
for pass in sizes compaction session-expiry vos-to-ref ref-to-vos compacted control-no-hold; do
    n=$((n + 1))
    echo "pass $pass"
    "$build/adapter-target/release/exp1-vos" "$id" "$pass" "$out/$n" > "$out/$pass.md"
done
echo "reports in $out"
