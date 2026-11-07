#!/usr/bin/env bash
# Builds EXP-2's PolkaVM binding and its stand-ins (T-008-03): the binding's programs, then one sandboxed and
# one outside PolkaVM program per provider category in Rust and in C, and the hostile sandboxed parts admission
# must refuse. Works on x86_64 Linux and on macOS.
#
# Needs CARGO_TARGET_DIR set explicitly; everything it builds or fetches goes under it:
#   $CARGO_TARGET_DIR/release/rutent-pvm-*      the binding's programs
#   $CARGO_TARGET_DIR/pvm-guests/{rs,c}/*.polkavm  the stand-ins
#   $CARGO_TARGET_DIR/pvm-fetched/               PolkaVM's C guest header, fetched and checked
#
# Toolchains:
#   - Rust stand-ins: the pinned nightly below, as a non-default toolchain with rust-src, building core with
#     -Z build-std for PolkaVM's riscv64emac target JSON. The default toolchain is never changed.
#   - C stand-ins: clang with a riscv64 target ($CLANG, or Homebrew's LLVM on macOS, or clang on PATH), linked
#     with the nightly's own rust-lld. On macOS rust-lld needs DYLD_LIBRARY_PATH set to the toolchain's lib
#     directory to find its libLLVM. The C link needs -z notext, which the Rust link doesn't: the header's
#     import and export metadata are absolute relocations in read-only sections.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
: "${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR explicitly}"
export CARGO_TARGET_DIR

NIGHTLY=nightly-2026-09-20
POLKAVM_COMMIT=471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d
HEADER_SHA256=1b1f198f05cc51b5e82228282f7ec019608fcda21591fc57f8ab9f79e2cb9e9b
CATEGORIES="reserve exchange ramp verifier"
OUT=$CARGO_TARGET_DIR/pvm-guests
FETCHED=$CARGO_TARGET_DIR/pvm-fetched

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

echo "== the binding's programs (default toolchain: $(rustc --version))"
cargo build --release --locked --manifest-path "$HERE/Cargo.toml" --bins
TOOL=$CARGO_TARGET_DIR/release/rutent-pvm-tool

echo "== the Rust stand-ins ($NIGHTLY)"
if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
    rustup toolchain install "$NIGHTLY" --profile minimal --component rust-src
fi
rustup component add rust-src --toolchain "$NIGHTLY" >/dev/null
JSON=$("$TOOL" target-json "$CARGO_TARGET_DIR/pvm-target")
rustup run "$NIGHTLY" cargo build --release --locked --examples \
    --manifest-path "$HERE/guests/rust/Cargo.toml" \
    -Z build-std=core -Zjson-target-spec --target "$JSON" 2>&1 | grep -v "is not a recognized feature for this target" || true
ELFS=$CARGO_TARGET_DIR/riscv64emac-unknown-none-polkavm/release/examples
mkdir -p "$OUT/rs" "$OUT/c"
for part in sandboxed outside; do
    for category in $CATEGORIES; do
        "$TOOL" link "$ELFS/${part}_$category.elf" "$OUT/rs/$part-$category.polkavm"
    done
done
for hostile in hostile-secret hostile-clock; do
    "$TOOL" link "$ELFS/${hostile//-/_}.elf" "$OUT/rs/$hostile.polkavm"
done

echo "== the C stand-ins"
mkdir -p "$FETCHED"
HEADER=$FETCHED/polkavm_guest.h
if [ ! -f "$HEADER" ] || [ "$(sha256 "$HEADER")" != "$HEADER_SHA256" ]; then
    curl -fsSL -o "$HEADER" "https://raw.githubusercontent.com/paritytech/polkavm/$POLKAVM_COMMIT/capi/polkavm_guest.h"
fi
if [ "$(sha256 "$HEADER")" != "$HEADER_SHA256" ]; then
    echo "polkavm_guest.h doesn't match its pinned SHA-256" >&2
    exit 1
fi
if [ -z "${CLANG:-}" ]; then
    if [ -x /opt/homebrew/opt/llvm/bin/clang ]; then CLANG=/opt/homebrew/opt/llvm/bin/clang; else CLANG=clang; fi
fi
if ! "$CLANG" -print-targets | grep -q riscv64; then
    echo "$CLANG has no riscv64 target; set CLANG" >&2
    exit 1
fi
echo "clang: $("$CLANG" --version | head -1)"
SYSROOT=$(rustup run "$NIGHTLY" rustc --print sysroot)
HOST=$(rustup run "$NIGHTLY" rustc -vV | sed -n 's/^host: //p')
LLD=$SYSROOT/lib/rustlib/$HOST/bin/rust-lld
# Only for rust-lld: set for clang too, it would load the toolchain's libLLVM instead of its own.
lld() { DYLD_LIBRARY_PATH=$SYSROOT/lib "$LLD" "$@"; }
echo "rust-lld: $(lld -flavor gnu --version)"
CFLAGS="--target=riscv64-unknown-none-elf -march=rv64emac_zbb -mabi=lp64e -fPIC -fvisibility=hidden \
    -ffreestanding -fno-builtin -nostdlib -Os -std=gnu11 -Wall -Wextra -Werror"
LDFLAGS="-flavor gnu -shared -z notext --emit-relocs --unique --apply-dynamic-relocs --no-allow-shlib-undefined \
    -Bsymbolic --gc-sections -z relro -z now --strip-debug"
OBJ=$CARGO_TARGET_DIR/pvm-c-objects
mkdir -p "$OBJ"
c_guest() { # name, defines...
    local name=$1; shift
    # shellcheck disable=SC2086
    "$CLANG" $CFLAGS -I"$FETCHED" "$@" -c "$HERE/guests/c/guest.c" -o "$OBJ/$name.o"
    # shellcheck disable=SC2086
    lld $LDFLAGS -o "$OBJ/$name.elf" "$OBJ/$name.o"
    "$TOOL" link "$OBJ/$name.elf" "$OUT/c/$name.polkavm"
}
for category in $CATEGORIES; do
    c_guest "sandboxed-$category" -DRUTENT_SANDBOXED "-DRUTENT_CATEGORY=\"$category\""
    c_guest "outside-$category" -DRUTENT_OUTSIDE "-DRUTENT_CATEGORY=\"$category\""
done
c_guest hostile-secret -DRUTENT_SANDBOXED '-DRUTENT_CATEGORY="reserve"' -DRUTENT_HOSTILE=1
c_guest hostile-clock -DRUTENT_SANDBOXED '-DRUTENT_CATEGORY="reserve"' -DRUTENT_HOSTILE=2

echo "== built"
ls -l "$OUT/rs" "$OUT/c"
