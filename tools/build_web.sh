#!/usr/bin/env bash
# Builds the WebAssembly module and assembles `web/` into a directory that can be
# served as-is — which is exactly what the GitHub Pages workflow uploads.
#
#   tools/build_web.sh          build into web/
#   tools/build_web.sh --serve  build, then serve it on http://localhost:8000
#
# `wasm-bindgen` must be the same version as the `wasm-bindgen` crate in Cargo.lock;
# it refuses to run otherwise, and says so. Install the matching one with:
#
#   cargo install wasm-bindgen-cli --version "$(tools/build_web.sh --print-version)"

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

wanted_version() {
    # The version Cargo actually resolved, rather than the range in the manifest.
    cargo metadata --format-version 1 --locked 2>/dev/null |
        sed -n 's/.*"name":"wasm-bindgen","version":"\([^"]*\)".*/\1/p' |
        head -1
}

if [[ "${1:-}" == "--print-version" ]]; then
    wanted_version
    exit 0
fi

echo "==> building the wasm module"
cargo build --release --package ll2-wasm --target wasm32-unknown-unknown

echo "==> generating the JavaScript bindings"
wasm-bindgen \
    --target web \
    --out-dir web/pkg \
    --out-name ll2_wasm \
    target/wasm32-unknown-unknown/release/ll2_wasm.wasm

# Optional, and worth roughly a fifth of the module -- but only from a recent
# binaryen. Ubuntu ships 108 (2022), which does not understand the externref table
# `wasm-bindgen` emits: it drops the table, reports success, and the module then
# dies at instantiation with "WebAssembly.Table.grow(): failed to grow table by 4".
# A silently broken artifact is worse than a bigger one, so an old `wasm-opt` is
# skipped rather than trusted -- and said out loud, because a build that quietly
# stops optimising is its own kind of surprise.
readonly MINIMUM_BINARYEN=116
if command -v wasm-opt >/dev/null 2>&1; then
    binaryen=$(wasm-opt --version 2>/dev/null | grep -oE '[0-9]+' | head -1 || true)
    if [[ -n "$binaryen" && "$binaryen" -ge "$MINIMUM_BINARYEN" ]]; then
        echo "==> wasm-opt -Os (binaryen $binaryen)"
        wasm-opt -Os -o web/pkg/ll2_wasm_bg.wasm web/pkg/ll2_wasm_bg.wasm
    else
        echo "==> skipping wasm-opt: binaryen ${binaryen:-unknown} predates $MINIMUM_BINARYEN"
    fi
fi

echo "==> copying the example map"
mkdir -p web/sample
cp tests/data/mapping_example.osm web/sample/mapping_example.osm

echo "==> web/ is ready ($(du -sh web/pkg | cut -f1) of wasm and glue)"

if [[ "${1:-}" == "--serve" ]]; then
    echo "==> http://localhost:8000  (Ctrl-C to stop)"
    # ES modules and `fetch` both need a real origin; file:// will not do.
    python3 -m http.server 8000 --directory web
fi
