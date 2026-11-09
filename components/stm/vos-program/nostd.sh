#!/bin/sh
# The no_std copy of the host-contract suite's probe, which the program is built with under its `probe-machine`
# feature, to score the host's mechanics: the suite is typed on its probe and compares states with it, so a host that
# runs `Stm` is scored for its mechanics with the probe, and for `Stm` by its own tests (T-010-05). The copy is
# conformance's probe, patched with a committed patch whose hunks touch only `#![no_std]`, `extern crate alloc`, `use`
# lines and `cfg` attributes. Anything else it needs is a separate file, listed in SEPARATE. contracts/ needs no copy:
# the crate is no_std itself (DEC-26, T-010-01).
#
# Usage:
#   nostd.sh generate   rewrite probe-nostd/src/probe.rs from the original and the patch
#   nostd.sh check      fail unless the committed copy equals the original plus the patch, every changed line
#                       of the patches is plumbing only, and the files beside the copies are exactly SEPARATE
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
root=$(cd "$here/../../.." && pwd -P)
program="$here"

# Copy the originals into DIR, laid out as this directory lays them out, and apply the patches there.
copy_into() {
    dir=$1
    mkdir -p "$dir/probe-nostd/src"
    cp "$root/conformance/src/host/probe.rs" "$dir/probe-nostd/src/probe.rs"
    for p in probe-nostd; do
        (cd "$dir" && patch -s -p1 --no-backup-if-mismatch < "$program/$p.patch")
    done
}

# Every file a copy holds, relative to this directory: what copy_into writes.
copied_files() {
    echo probe-nostd/src/probe.rs
}

case "${1:-}" in
generate)
    rm -f "$program/probe-nostd/src/probe.rs"
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    copy_into "$work"
    cp "$work/probe-nostd/src/probe.rs" "$program/probe-nostd/src/probe.rs"
    echo "nostd.sh: copy generated"
    ;;
check)
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    failed=0
    # 1. The copy equals the original plus the patch.
    copy_into "$work/expected"
    for f in $(copied_files); do
        if ! cmp -s "$work/expected/$f" "$program/$f"; then
            echo "nostd.sh: $f differs from the original plus its patch" >&2
            failed=1
        fi
    done
    # 2. Every changed line is plumbing only: #![no_std], extern crate alloc, a one-line use, or a cfg attribute.
    plumbing='^[-+](#!\[no_std\]|extern crate alloc;|[[:space:]]*(pub(\([a-z]+\))? )?use [^;{}]*(\{[^;]*\})?[^;{}]*;|[[:space:]]*#!?\[cfg(_attr)?\(.*\)\])$'
    for p in probe-nostd; do
        bad=$(grep -E '^[-+]' "$program/$p.patch" | grep -vE '^(\+\+\+|---) ' | grep -vE "$plumbing" || true)
        if [ -n "$bad" ]; then
            echo "nostd.sh: $p.patch changes lines that are not plumbing:" >&2
            echo "$bad" >&2
            failed=1
        fi
    done
    # 3. The files beside the copy are exactly the listed separate files.
    copied_files | sort > "$work/copied"
    grep -v '^#' "$program/SEPARATE" | grep -v '^$' | sort > "$work/listed"
    (cd "$program" && find probe-nostd -type f -not -path '*/target/*') | sort > "$work/present"
    comm -23 "$work/present" "$work/copied" > "$work/beside"
    if ! cmp -s "$work/beside" "$work/listed"; then
        echo "nostd.sh: the files beside the copies are not exactly SEPARATE:" >&2
        diff "$work/listed" "$work/beside" >&2 || true
        failed=1
    fi
    if [ "$failed" -ne 0 ]; then
        exit 1
    fi
    echo "nostd.sh: the copy equals the original plus its patch; every hunk is plumbing only;" \
        "the separate files are exactly the $(wc -l < "$work/listed" | tr -d ' ') listed"
    ;;
*)
    echo "usage: nostd.sh generate|check" >&2
    exit 2
    ;;
esac
