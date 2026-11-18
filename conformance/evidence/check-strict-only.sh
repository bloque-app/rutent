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

# Run from the repository's root whatever the caller's directory, and refuse a ref that names no commit: a check that
# can't read the diff must fail, never pass as "unchanged".
root="$(git rev-parse --show-toplevel)"
cd "$root"
git rev-parse --verify --quiet "$base^{commit}" >/dev/null || { echo "check-strict-only: no commit $base" >&2; exit 2; }
if [ "$head" != "-" ]; then
  git rev-parse --verify --quiet "$head^{commit}" >/dev/null || { echo "check-strict-only: no commit $head" >&2; exit 2; }
fi
[ -f "$scope" ] || { echo "check-strict-only: no $scope in $root" >&2; exit 2; }

if [ "$head" = "-" ]; then range=("$base"); else range=("$base" "$head"); fi
diff="$(git diff --unified=0 --no-color "${range[@]}" -- "$scope")"
changed="$(printf '%s\n' "$diff" | grep -E '^[+-]' | grep -vE '^(\+\+\+|---) ' || true)"
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
