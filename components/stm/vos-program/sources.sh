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

# The files that pin each source: VOS in every manifest and lockfile, the forks where the client's workspace names
# them (s4 is only in VOS's own lockfile, which build.sh checks). A pin of a source in any of its files that differs
# from the list's, or a missing one in a file that must hold it, is a failure.
sources_pinned_in() {
    case "$1" in
    vos) echo "Cargo.toml client/Cargo.toml Cargo.lock client/Cargo.lock" ;;
    lsm-tree) echo "client/Cargo.toml client/Cargo.lock" ;;
    stwo) echo "client/Cargo.lock" ;;
    s4) echo "" ;;
    esac
}

sources_check() {
    ok=0
    for name in vos lsm-tree stwo s4; do
        want=$(source_field "$name" 2)
        upstream=$(source_field "$name" 3)
        for rel in $(sources_pinned_in "$name"); do
            file="$sources_sha_root/$rel"
            found=$(grep -F "$upstream" "$file" | grep -oE "rev(=| = \")[0-9a-f]{40}" | grep -oE '[0-9a-f]{40}' | sort -u || true)
            if [ -z "$found" ]; then
                echo "sources.sh: $rel does not pin $name, which the list says it must" >&2
                ok=1
            fi
            for rev in $found; do
                if [ "$rev" != "$want" ]; then
                    echo "sources.sh: $rel pins $name at $rev, the list at $want" >&2
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
