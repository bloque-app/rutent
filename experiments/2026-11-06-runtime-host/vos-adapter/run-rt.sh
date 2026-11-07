#!/bin/sh
# Runs passes of the re-score with what build-rt.sh built in BUILD, writing each pass's report to OUT/<pass>.md and
# its stores under OUT/s/<n>. OUT must be new. Keep OUT's path short: each node's API socket is in its data
# directory, and a unix socket's path is limited.
# Usage: run-rt.sh RUN_ID BUILD OUT PASS...
# R-07a and R-07b need R07_STREAM and R07_DATA, from build.sh's export-file pass at the first run's commit.
set -eu
id=$1
build=$(cd "$2" && pwd -P)
out=$3
shift 3
if [ -e "$out" ]; then
    echo "run-rt.sh: $out exists" >&2
    exit 1
fi
mkdir -p "$out/s"
export VOS_BIN="$build/bin/vos"
export VOS_FAULTS_BIN="$build/bin/vos-faults"
export VOS_AGENT_ELF="$build/elf/vos-agent.elf"
export PROGRAM_ELF="$build/elf/rutent-scored.elf"
export PROGRAM_NEXT_ELF="$build/elf/rutent-next.elf"
export PROGRAM_NEXT_FORMAT_ELF="$build/elf/rutent-next-format.elf"
n=0
failed=0
for pass in "$@"; do
    n=$((n + 1))
    echo "pass $pass"
    if ! "$build/rt-target/release/exp1-rt" "$id" "$pass" "$out/s/$n" > "$out/$pass.md" 2> "$out/$pass.err"; then
        echo "pass $pass did not complete" >&2
        failed=1
    fi
done
echo "reports in $out"
exit "$failed"
