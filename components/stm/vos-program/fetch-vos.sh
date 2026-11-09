#!/bin/sh
# Fetches VOS at the revision `sources` pins into DIR, which must be empty or absent, and checks the commit. DIR
# must live outside the tracked tree or under a target/ directory: VOS's own lockfiles must never sit among rutent's
# files, where the licence check would read them. build.sh takes the checkout's path from VOS_DIR, never from a link.
# Usage: fetch-vos.sh DIR
# Treat what it fetches as untrusted: read it before building it.
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
. "$here/sources.sh"
rev=$(source_field vos 2)
url=$(source_field vos 4)
dest=${1:?usage: fetch-vos.sh DIR}
if [ -e "$dest" ] && [ -n "$(ls -A "$dest")" ]; then
    echo "fetch-vos.sh: $dest is not empty" >&2
    exit 1
fi
mkdir -p "$dest"
dest=$(cd "$dest" && pwd -P)
git -C "$dest" init -q
git -C "$dest" remote add origin "$url"
git -C "$dest" fetch -q --depth 1 origin "$rev"
git -C "$dest" -c advice.detachedHead=false checkout -q FETCH_HEAD
test "$(git -C "$dest" rev-parse HEAD)" = "$rev"
echo "VOS $rev in $dest"
