#!/usr/bin/env bash
# T-024-10: checks, from the diff, that a commit changes no line of the scope file but its `strict:` line.
#
#   conformance/evidence/check-strict-only.sh [<base> [<head>]]
#
# `<base>` defaults to `HEAD~1` and `<head>` to `HEAD`; `<head>` may be `-` for the working tree. The scope file's diff between them must be exactly one line
# removed and one added, the first reading `strict: off` and the second `strict: on` (or no diff at all). Any other
# changed line, an added or removed row included, fails. A diff of other files is not this script's concern.
set -euo pipefail

base="${1:-HEAD~1}"
head="${2:-HEAD}"
scope="conformance/evidence/v0-scope"

if [ "$head" = "-" ]; then range=("$base"); else range=("$base" "$head"); fi
changed="$(git diff --unified=0 --no-color "${range[@]}" -- "$scope" | grep -E '^[+-]' | grep -vE '^(\+\+\+|---) ' || true)"
expected="$(printf -- '-strict: off\n+strict: on')"

if [ -z "$changed" ]; then
  echo "check-strict-only: $scope unchanged between $base and $head"
  exit 0
fi
if [ "$changed" = "$expected" ]; then
  echo "check-strict-only: only the strict line changed ($base..$head)"
  exit 0
fi
echo "check-strict-only: $scope changed beyond its strict line between $base and $head:" >&2
printf '%s\n' "$changed" >&2
exit 1
