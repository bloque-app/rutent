#!/usr/bin/env bash
# Builds the module runtime's stand-ins (`T-013-01`): the sixteen module-boundary stand-ins, in Rust and in C, and the
# four role stand-ins (`B-01`), each a pair of components the module tests admit.
#
#   CARGO_TARGET_DIR=<a build directory for the guests only> ./build.sh <output directory>
#
# It needs: cargo and rustc with the wasm32-unknown-unknown target (rustup), whose sysroot ships rust-lld; and a
# clang that targets wasm32, from $CLANG, else Homebrew's LLVM on macOS, else `clang`. It downloads nothing beyond
# the crates `Cargo.lock` pins. It runs on macOS (arm64) and Linux (x86_64). It writes `{language}-{category}-{part}.wasm`
# `role-{category}-sandboxed.wasm` and the admission tests' `hostile-*.wasm` to the output directory, and `built` there when it is done.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR to a build directory for the guests only}"
out="${1:?usage: build.sh <output directory>}"
target="$CARGO_TARGET_DIR"
wit="$here/../src/module/wit"
categories=(reserve exchange ramp verifier)
parts=(sandboxed outside)

say() { printf '==> %s\n' "$*" >&2; }

mkdir -p "$out"
rm -f "$out/built"

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
    echo "the wasm32-unknown-unknown target is missing: rustup target add wasm32-unknown-unknown" >&2
    exit 1
fi

say "building the guests' tool"
cargo build --quiet --locked --manifest-path "$here/Cargo.toml" -p rutent-wasm-guests
tool="$target/debug/rutent-wasm-guests"

say "writing the hostile components"
"$tool" hostile "$out"

say "building the Rust stand-ins"
for category in "${categories[@]}"; do
    for part in "${parts[@]}"; do
        cargo build --quiet --release --locked --manifest-path "$here/Cargo.toml" -p rutent-wasm-standin \
            --target wasm32-unknown-unknown --features "$part,$category"
        "$tool" componentize "$target/wasm32-unknown-unknown/release/rutent_wasm_standin.wasm" \
            "$out/rs-$category-$part.wasm"
    done
done

say "building the role stand-ins"
for category in "${categories[@]}"; do
    cargo build --quiet --release --locked --manifest-path "$here/Cargo.toml" -p rutent-wasm-role-standin \
        --target wasm32-unknown-unknown --features "$category"
    "$tool" componentize "$target/wasm32-unknown-unknown/release/rutent_wasm_role_standin.wasm" \
        "$out/role-$category-sandboxed.wasm"
done

say "building the C stand-ins"
if [[ -n "${CLANG:-}" ]]; then
    cc="$CLANG"
elif [[ "$(uname -s)" == Darwin && -x /opt/homebrew/opt/llvm/bin/clang ]]; then
    cc=/opt/homebrew/opt/llvm/bin/clang
else
    cc=clang
fi
if ! "$cc" -print-targets 2>/dev/null | grep -qw wasm32; then
    echo "$cc does not target wasm32: set CLANG to a clang that does" >&2
    exit 1
fi
sysroot="$(rustc --print sysroot)"
host="$(rustc -vV | sed -n 's/^host: //p')"
lld="$sysroot/lib/rustlib/$host/bin/rust-lld"
bindings="$target/c-bindings"
objects="$target/c-build"
rm -rf "$bindings" "$objects"
mkdir -p "$bindings" "$objects"
"$tool" slot-header "$bindings/slot.h"
for part in "${parts[@]}"; do
    "$tool" c-bindings "$wit" "$part" "$bindings/$part"
done
cflags=(--target=wasm32 -O2 -ffreestanding -nostdlib -fno-builtin -Wall -Wextra -Werror
    -Wno-unused-parameter -isystem "$here/c/shim" -I "$here/c" -I "$bindings")
for category in "${categories[@]}"; do
    for part in "${parts[@]}"; do
        define="PART_$(printf '%s' "$part" | tr '[:lower:]' '[:upper:]')"
        objs=()
        for src in "$here/c/standin.c" "$here/c/json.c" "$here/c/rt.c" "$bindings/$part/$part.c"; do
            obj="$objects/$category-$part-$(basename "$src" .c).o"
            # The generated bindings are wit-bindgen's, built as they come.
            relax=-Werror
            [[ "$src" == "$bindings"/* ]] && relax=-Wno-error
            "$cc" "${cflags[@]}" "$relax" -I "$bindings/$part" -D"$define" \
                -DCATEGORY="\"$category\"" -c "$src" -o "$obj"
            objs+=("$obj")
        done
        DYLD_LIBRARY_PATH="$sysroot/lib" LD_LIBRARY_PATH="$sysroot/lib" "$lld" -flavor wasm --no-entry \
            --gc-sections --strip-debug -o "$objects/c-$category-$part.core.wasm" "${objs[@]}" \
            "$bindings/$part/${part}_component_type.o"
        "$tool" componentize "$objects/c-$category-$part.core.wasm" "$out/c-$category-$part.wasm"
    done
done

say "stand-ins, by SHA-256"
"$tool" digest "$out"/*.wasm | sed "s|$out/||" >&2
touch "$out/built"
