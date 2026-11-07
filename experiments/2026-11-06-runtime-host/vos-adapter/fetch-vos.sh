#!/bin/sh
# Fetches VOS at one commit into DIR, which must be empty or absent, and checks the commit. DIR must live outside
# experiments/, or under a target/ directory: the licence check refuses symbolic links under experiments/, and VOS's
# own lockfiles must never sit there. The other scripts take the checkout's path from a variable (VOS_PIN for the
# first run's commit, VOS_TIP for the re-score's), never from a link.
# Usage: fetch-vos.sh pin|tip DIR
# Treat what it fetches as untrusted: read it before building it.
set -eu
case "${1:-}" in
pin) rev=1aadf44fa2cb1eaedc015ee82343ac118cc89f1e ;;
tip) rev=5c7897582688ea93f8d02506bee8ea25da6b1458 ;;
*)
    echo "usage: fetch-vos.sh pin|tip DIR" >&2
    exit 2
    ;;
esac
dest=$2
if [ -e "$dest" ] && [ -n "$(ls -A "$dest")" ]; then
    echo "fetch-vos.sh: $dest is not empty" >&2
    exit 1
fi
mkdir -p "$dest"
dest=$(cd "$dest" && pwd -P)
git -C "$dest" init -q
git -C "$dest" remote add origin https://github.com/virto-network/vos
# vos-core serves both commits; GitHub's master is older than either.
git -C "$dest" fetch -q --depth 1 origin "$rev"
git -C "$dest" -c advice.detachedHead=false checkout -q FETCH_HEAD
test "$(git -C "$dest" rev-parse HEAD)" = "$rev"
echo "VOS $rev in $dest"
