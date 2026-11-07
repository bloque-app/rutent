#!/bin/sh
# Fetches VOS at the commit EXP-1 scores into DIR, which must be empty or absent, and links it here as .vos.
# Usage: fetch-vos.sh DIR
# Treat what it fetches as untrusted: read it before building it.
set -eu
rev=1aadf44fa2cb1eaedc015ee82343ac118cc89f1e
here=$(cd "$(dirname "$0")" && pwd)
dest=$1
if [ -e "$dest" ] && [ -n "$(ls -A "$dest")" ]; then
    echo "fetch-vos.sh: $dest is not empty" >&2
    exit 1
fi
mkdir -p "$dest"
dest=$(cd "$dest" && pwd -P)
git -C "$dest" init -q
git -C "$dest" remote add origin https://github.com/virto-network/vos
git -C "$dest" fetch -q --depth 1 origin "$rev"
git -C "$dest" -c advice.detachedHead=false checkout -q FETCH_HEAD
test "$(git -C "$dest" rev-parse HEAD)" = "$rev"
ln -sfn "$dest" "$here/.vos"
echo "VOS $rev in $dest, linked as $here/.vos"
