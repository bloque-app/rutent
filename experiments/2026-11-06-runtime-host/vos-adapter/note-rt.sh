#!/bin/sh
# Writes the run note OUT/<RUN_ID>.md for one candidate of the re-score, from the reports run-rt.sh wrote in
# REPORTS, the runner's facts in REPORTS/../runner.txt and toolchains.txt, and the build's times and digests in
# BUILD. Every note states what the re-score brief's sections 3.2 and 4 ask of it.
# Usage: note-rt.sh RUN_ID CANDIDATE BUILD REPORTS OUT PASS...
set -eu
id=$1
candidate=$2
build=$3
reports=$4
out=$5
shift 5
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
note="$out/$id.md"
{
    echo "---"
    echo "run: $id"
    echo "candidate: $candidate"
    echo "kind: scored run"
    echo "date: $(date -u +%Y-%m-%d)"
    echo "task: T-007-03"
    echo "---"
    echo
    echo "# $id: the re-score, candidate \`$candidate\`"
    echo
    echo "Written by \`vos-adapter/note-rt.sh\` from the workflow \`exp1-vos.yml\`'s reports. The re-score brief's"
    echo "criteria are in \`../decisions.json\`, item \`exp1-rescore-criteria\`, fixed before any run."
    echo
    echo "## Provenance"
    echo
    echo "| Item | Value |"
    echo "|---|---|"
    echo "| This repository's commit | \`${GITHUB_SHA:-unknown}\` |"
    echo "| Workflow run | ${GITHUB_SERVER_URL:-}/${GITHUB_REPOSITORY:-}/actions/runs/${GITHUB_RUN_ID:-} (attempt ${GITHUB_RUN_ATTEMPT:-1}) |"
    echo "| Host contract | $(sed -n 's/^## //p' "$here/../../../contracts/state-machine/host/CHANGELOG.md" | head -1), with \`MAX_SEQ\` |"
    echo "| State-machine contract | $(sed -n 's/^## //p' "$here/../../../contracts/state-machine/CHANGELOG.md" | head -1) |"
    echo "| VOS | \`vos-core\` at \`5c7897582688ea93f8d02506bee8ea25da6b1458\`, fetched by its SHA with \`fetch-vos.sh tip\` |"
    echo "| The state machine | the suite's probe, run inside VOS as the program \`vos-adapter/program\`, built from the generated no_std copies of \`contracts/\` and of conformance's probe (\`nostd.sh\`); \`H-17\` folds with the original \`contracts/\` and \`Probe\`, natively |"
    echo "| The suite | \`conformance/src/host/\` at this commit, its assertions run unmodified: only the state machine's build target differs |"
    echo "| Separate files of the no_std port | $(grep -v '^#' "$here/program/SEPARATE" | grep -v '^$' | sed 's/.*/`&`/' | paste -sd, - | sed 's/,/, /g'); the serialiser is not replaced (serde_json, alloc-only) |"
    echo "| The \`pb\` candidate | \`components/stm\` behind \`purpose-built-store\`, unchanged; its rows' harness code is in \`vos-adapter/rt/src/pb.rs\` |"
    echo "| Command | \`build-rt.sh\`, then \`run-rt.sh $id <build> <out> $*\` |"
    echo
    echo "### The runner"
    echo
    echo '```text'
    cat "$reports/../runner.txt" 2>/dev/null || echo "not recorded"
    echo '```'
    echo
    echo "### Toolchains, build times and digests"
    echo
    echo '```text'
    cat "$reports/../toolchains.txt" 2>/dev/null || echo "not recorded"
    cat "$build/build-times.txt" 2>/dev/null || true
    sed "s|  $build/|  |" "$build/digests.txt" 2>/dev/null || true
    echo '```'
    echo
    echo "## How each fault was reached"
    echo
    echo "| Fault | How |"
    echo "|---|---|"
    echo "| Kill and reopen | \`kill -9\` of every node's process, then each started again on its own data directory |"
    echo "| Stop after received | The input held by the client, nothing sent; every node killed |"
    echo "| Stop after computed | Faults build: every node's faults file set to \`hold_commits\` and each node's \`generation\` seen; the submit sent; once the leader's \`refine\` stage counted it (the program computed), every node killed. Held, no node can have made it durable. On the scored build nothing is held: the same kill lands after the compute, unheld, which is not \`H-02\`, \`H-06\` or \`H-11\`'s test |"
    echo "| Stop after durable | The program answered the submit as recorded, so it is committed; every node killed before the client returns |"
    echo "| Stop during an import | Every node killed between two chunks' calls |"
    echo "| Leader killed mid-submit (\`R-01\`) | \`kill -9\` of the leader alone, each node its own OS process |"
    echo "| A replica lost or replaced (\`R-02\`, \`R-03\`) | VOS's \`replace\` at the leader, the new node's own key, its data never copied |"
    echo "| The last batch damaged (\`R-04\`) | The node killed, the newest file of fjall's journal in its store cut by 8 bytes or its last byte flipped, the node started again |"
    echo
    echo "## Passes"
    echo
    for pass in "$@"; do
        echo "### \`$pass\`"
        echo
        if [ -f "$reports/$pass.md" ]; then
            cat "$reports/$pass.md"
        else
            echo "Not run."
        fi
        if [ -s "$reports/$pass.err" ]; then
            echo
            echo "The pass's last lines on stderr:"
            echo
            echo '```text'
            tail -5 "$reports/$pass.err"
            echo '```'
        fi
        echo
    done
} > "$note"
echo "note in $note"
