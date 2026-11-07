#!/usr/bin/env bash
# Builds the Wasm binding of the module-boundary suite (EXP-2, T-008-02) and its sixteen stand-ins, runs the suite
# once per provider category's stand-ins in Rust and C, and prints the report.
#
#   CARGO_TARGET_DIR=<a build directory of this binding's own> ./run.sh [run id] [report.json]
#
# Steps, each skippable for a rebuild: RUTENT_SKIP_HOST=1, RUTENT_SKIP_GUESTS=1, RUTENT_SKIP_SUITE=1. RUTENT_TESTS=1
# also runs the binding's own tests. It needs: cargo and rustc with the wasm32-unknown-unknown target (rustup), whose
# sysroot ships rust-lld; and a clang that targets wasm32, from $CLANG, else Homebrew's LLVM on macOS, else `clang`.
# It downloads nothing beyond the crates its two Cargo.lock files pin. It runs on macOS (arm64) and Linux (x86_64).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR to a build directory for this binding only}"
target="$CARGO_TARGET_DIR"
export CARGO_TARGET_DIR
run_id="${1:-EXP-2-wasm-dev-$(date -u +%Y%m%d)-1}"
report="${2:-$target/rutent-wasm-report.json}"
standins="$target/rutent-wasm-standins"
categories=(reserve exchange ramp verifier)
parts=(sandboxed outside)

say() { printf '==> %s\n' "$*" >&2; }

if [[ -z "${RUTENT_SKIP_HOST:-}" ]]; then
    say "building the binding (host workspace, release)"
    cargo build --release --locked --manifest-path "$here/Cargo.toml" --workspace
fi
tool="$target/release/rutent-wasm-guests"

if [[ -z "${RUTENT_SKIP_GUESTS:-}" ]]; then
    rm -rf "$standins"
    mkdir -p "$standins"

    if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
        echo "the wasm32-unknown-unknown target is missing: rustup target add wasm32-unknown-unknown" >&2
        exit 1
    fi
    say "building the Rust stand-ins"
    for category in "${categories[@]}"; do
        for part in "${parts[@]}"; do
            CARGO_TARGET_DIR="$target/guests-rust" cargo build --quiet --release --locked \
                --manifest-path "$here/guests/rust/Cargo.toml" --target wasm32-unknown-unknown \
                --features "$part,$category"
            "$tool" componentize "$target/guests-rust/wasm32-unknown-unknown/release/rutent_wasm_standin.wasm" \
                "$standins/rs-$category-$part.wasm"
        done
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
        "$tool" c-bindings "$here/wit" "$part" "$bindings/$part"
    done
    cflags=(--target=wasm32 -O2 -ffreestanding -nostdlib -fno-builtin -Wall -Wextra -Werror
        -Wno-unused-parameter -isystem "$here/guests/c/shim" -I "$here/guests/c" -I "$bindings")
    for category in "${categories[@]}"; do
        for part in "${parts[@]}"; do
            define="PART_$(printf '%s' "$part" | tr '[:lower:]' '[:upper:]')"
            objs=()
            for src in "$here/guests/c/standin.c" "$here/guests/c/json.c" "$here/guests/c/rt.c" \
                "$bindings/$part/$part.c"; do
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
            "$tool" componentize "$objects/c-$category-$part.core.wasm" "$standins/c-$category-$part.wasm"
        done
    done
    say "stand-ins, by SHA-256"
    "$tool" digest "$standins"/*.wasm | sed "s|$standins/||" >&2
fi

export RUTENT_WASM_STANDINS="$standins"

if [[ -n "${RUTENT_TESTS:-}" ]]; then
    say "running the binding's own tests"
    cargo test --release --locked --manifest-path "$here/Cargo.toml" --workspace -- --test-threads=1
fi

if [[ -z "${RUTENT_SKIP_SUITE:-}" ]]; then
    say "running the module-boundary suite as $run_id"
    "$target/release/rutent-wasm-run" "$run_id" "$report"
fi
