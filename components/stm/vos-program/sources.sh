#!/bin/sh
# Reads the list of sources (./sources, or the file SOURCES names) for the other scripts. Sourced, it defines:
#   source_field NAME COLUMN   column 2 (revision), 3 (upstream URL) or 4 (fetch URL) of NAME's line
#   sources_git_config DIR     exports GIT_CONFIG_* so that cargo and git fetch every source from its fetch URL, and
#                              VOS from the checkout DIR (never from the network)
#   sources_check              fails unless the revisions pinned in the manifests and lockfiles are the list's
# Run as `sources.sh check`, it runs sources_check.
set -eu
sources_here=$(cd "$(dirname "$0")" && pwd -P)
SOURCES=${SOURCES:-$sources_here/sources}

source_field() {
    awk -v name="$1" -v col="$2" '$1 !~ /^#/ && $1 == name { print $col; found = 1 } END { exit !found }' "$SOURCES" ||
        { echo "sources.sh: $SOURCES has no line for $1" >&2; return 1; }
}

sources_git_config() {
    vos_dir=$1
    n=0
    for name in vos lsm-tree stwo s4; do
        upstream=$(source_field "$name" 3)
        fetch=$(source_field "$name" 4)
        if [ "$name" = vos ]; then
            fetch="file://$vos_dir"
        fi
        if [ "$upstream" != "$fetch" ]; then
            export "GIT_CONFIG_KEY_$n=url.$fetch.insteadOf"
            export "GIT_CONFIG_VALUE_$n=$upstream"
            n=$((n + 1))
        fi
    done
    export GIT_CONFIG_COUNT=$n
    export CARGO_NET_GIT_FETCH_WITH_CLI=true
}

# Every `rev = "..."` the manifests pin, and every `?rev=...` the lockfiles lock, for a source's upstream URL.
sources_check() {
    ok=0
    for name in vos lsm-tree stwo s4; do
        want=$(source_field "$name" 2)
        upstream=$(source_field "$name" 3)
        for file in "$sources_sha_root/Cargo.toml" "$sources_sha_root/client/Cargo.toml" \
            "$sources_sha_root/Cargo.lock" "$sources_sha_root/client/Cargo.lock"; do
            # A pinned revision of this source in the file that differs from the list's.
            found=$(grep -F "$upstream" "$file" | grep -oE "rev(=| = \")[0-9a-f]{40}" | grep -oE '[0-9a-f]{40}' | sort -u || true)
            for rev in $found; do
                if [ "$rev" != "$want" ]; then
                    echo "sources.sh: $file pins $name at $rev, the list at $want" >&2
                    ok=1
                fi
            done
        done
    done
    return $ok
}

sources_sha_root=$sources_here
if [ "${1:-}" = check ]; then
    sources_check
    echo "sources.sh: the pins agree with the list"
fi
