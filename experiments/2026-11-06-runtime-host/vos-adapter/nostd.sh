#!/bin/sh
# The no_std copies the VOS program is built with (re-score brief, section 3.1): contracts/src, and conformance's
# probe, each copied and patched with a committed patch whose hunks touch only `#![no_std]`, `extern crate alloc`,
# `use` lines and `cfg` attributes. Anything else the program needs is a separate file, listed in program/SEPARATE.
#
# Usage:
#   nostd.sh generate   rewrite program/contracts-nostd/src and program/probe-nostd/src/probe.rs from the
#                       originals and the patches
#   nostd.sh check      fail unless the committed copies equal the originals plus the patches, every changed line
#                       of the patches is plumbing only, and the files beside the copies are exactly program/SEPARATE
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
root=$(cd "$here/../../.." && pwd -P)
program="$here/program"

# Copy the originals into DIR, laid out as program/ lays them out, and apply the patches there.
copy_into() {
    dir=$1
    mkdir -p "$dir/contracts-nostd" "$dir/probe-nostd/src"
    cp -R "$root/contracts/src" "$dir/contracts-nostd/src"
    cp "$root/conformance/src/host/probe.rs" "$dir/probe-nostd/src/probe.rs"
    for p in contracts-nostd probe-nostd; do
        (cd "$dir" && patch -s -p1 --no-backup-if-mismatch < "$program/$p.patch")
    done
}

# Every file a copy holds, relative to program/: what copy_into writes.
copied_files() {
    (cd "$root/contracts" && find src -type f | sed 's|^|contracts-nostd/|')
    echo probe-nostd/src/probe.rs
}

case "${1:-}" in
generate)
    rm -rf "$program/contracts-nostd/src" "$program/probe-nostd/src/probe.rs"
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    copy_into "$work"
    cp -R "$work/contracts-nostd/src" "$program/contracts-nostd/src"
    cp "$work/probe-nostd/src/probe.rs" "$program/probe-nostd/src/probe.rs"
    echo "nostd.sh: copies generated"
    ;;
check)
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    failed=0
    # 1. The copies equal the originals plus the patches.
    copy_into "$work/expected"
    for f in $(copied_files); do
        if ! cmp -s "$work/expected/$f" "$program/$f"; then
            echo "nostd.sh: program/$f differs from the original plus its patch" >&2
            failed=1
        fi
    done
    # 2. Every changed line is plumbing only: #![no_std], extern crate alloc, a one-line use, or a cfg attribute.
    plumbing='^[-+](#!\[no_std\]|extern crate alloc;|[[:space:]]*(pub(\([a-z]+\))? )?use [^;{}]*(\{[^;]*\})?[^;{}]*;|[[:space:]]*#!?\[cfg(_attr)?\(.*\)\])$'
    for p in contracts-nostd probe-nostd; do
        bad=$(grep -E '^[-+]' "$program/$p.patch" | grep -vE '^(\+\+\+|---) ' | grep -vE "$plumbing" || true)
        if [ -n "$bad" ]; then
            echo "nostd.sh: $p.patch changes lines that are not plumbing:" >&2
            echo "$bad" >&2
            failed=1
        fi
    done
    # 3. The files beside the copies are exactly the listed separate files.
    copied_files | sort > "$work/copied"
    grep -v '^#' "$program/SEPARATE" | grep -v '^$' | sort > "$work/listed"
    (cd "$program" && find contracts-nostd probe-nostd -type f -not -path '*/target/*') | sort > "$work/present"
    comm -23 "$work/present" "$work/copied" > "$work/beside"
    if ! cmp -s "$work/beside" "$work/listed"; then
        echo "nostd.sh: the files beside the copies are not exactly program/SEPARATE:" >&2
        diff "$work/listed" "$work/beside" >&2 || true
        failed=1
    fi
    if [ "$failed" -ne 0 ]; then
        exit 1
    fi
    echo "nostd.sh: the copies equal the originals plus their patches; every hunk is plumbing only;" \
        "the separate files are exactly the $(wc -l < "$work/listed" | tr -d ' ') listed"
    ;;
*)
    echo "usage: nostd.sh generate|check" >&2
    exit 2
    ;;
esac
