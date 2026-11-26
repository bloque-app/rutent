#!/bin/sh
# Runs the rutent demo (conformance/demo/README.md): a narrated run on simulated providers, holding no real value.
# Usage: conformance/demo/run.sh [--step] [--out DIR] [--seed N]
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd -P)
cd "$root"
exec cargo run --quiet -p rutent-scenarios --bin rutent-demo -- "$@"
