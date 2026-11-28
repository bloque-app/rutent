#!/bin/sh
# Runs rutent-scenarios-vos's tests (T-024-02): rutent-scenarios's checks with the state machine on three VOS replicas.
# It builds the host server, rutent-stm-vos-serve (scenarios/serve/), with the client's toolchain, and runs the
# scenarios with theirs (scenarios/rust-toolchain.toml): the module platform the assembled Core runs builds only with
# a newer one than VOS's crates do, so the two are two processes, and the scenarios drive the server over its wire.
# The node, the agent and the program are build.sh's, as test.sh names them. Linux x86_64 only for a scored run;
# VOS_DEV=1 runs the interpreter, a development run whose result is not the task's.
# Usage: VOS_DIR=<checkout> scenarios.sh OUT [cargo test arguments]
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
. "$here/sources.sh"
vos=$(cd "${VOS_DIR:?VOS_DIR names the checkout from fetch-vos.sh}" && pwd -P)
out=$(cd "$1" && pwd -P)
shift
sources_git_config "$vos"
# From each package's directory, so rustup picks its toolchain.
(cd "$here/scenarios/serve" && CARGO_TARGET_DIR="$out/client-target" \
    cargo build --release --locked --features vos --bin rutent-stm-vos-serve)
cd "$here/scenarios"
STM_VOS_SERVE="$out/client-target/release/rutent-stm-vos-serve" \
    VOS_BIN="$out/bin/vos" VOS_AGENT_ELF="$out/elf/vos-agent.elf" STM_PROGRAM_ELF="$out/elf/stm.elf" \
    CARGO_TARGET_DIR="$out/scenarios-target" \
    cargo test --release --locked --features vos "$@" -- --test-threads=1
