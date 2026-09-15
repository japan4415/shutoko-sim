#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
expected_version=0.2.128
if [[ "$(wasm-bindgen --version)" != "wasm-bindgen ${expected_version}" ]]; then
  echo "Install matching CLI: cargo install wasm-bindgen-cli --version ${expected_version} --locked" >&2
  exit 1
fi
cargo build --locked --release --target wasm32-unknown-unknown -p shutoko-routing-wasm
wasm-bindgen target/wasm32-unknown-unknown/release/shutoko_routing_wasm.wasm \
  --target web --out-dir dist/wasm --out-name shutoko_routing
printf '%s\n' '{"type":"module","private":true}' > dist/wasm/package.json
cp crates/routing-wasm/types/index.d.ts dist/wasm/index.d.ts
