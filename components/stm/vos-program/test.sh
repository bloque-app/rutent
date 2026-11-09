#!/bin/sh
# Runs the client's tests against the binaries build.sh made: the host-contract suite on one node and on three, and
# the kill of the leader mid-submit. Linux x86_64 only for a scored run: the nodes run the JIT, which needs a kernel
# of 6.7 or later with userfaultfd write-protect, and the tests fail closed off that platform. VOS_DEV=1 runs the
# interpreter instead, on any machine, a development run whose result is not the task's.
# Usage: VOS_DIR=<checkout> test.sh OUT [cargo test arguments]
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
. "$here/sources.sh"
vos=$(cd "${VOS_DIR:?VOS_DIR names the checkout from fetch-vos.sh}" && pwd -P)
out=$(cd "$1" && pwd -P)
shift
sources_git_config "$vos"
# From the client's directory, so rustup picks the client's toolchain.
cd "$here/client"
VOS_BIN="$out/bin/vos" VOS_AGENT_ELF="$out/elf/vos-agent.elf" STM_PROGRAM_ELF="$out/elf/stm.elf" PROBE_PROGRAM_ELF="$out/elf/probe.elf" \
    CARGO_TARGET_DIR="$out/client-target" \
    cargo test --release --locked --features vos "$@" -- --test-threads=1
