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
# デバッグ用の関数名（`name` カスタムセクション）を落とす（Issue #43）。
# 実行には不要で、wasm の 39%（636 KB）を占める。gzip 後でも 49 KB（opt-level="z" の
# ビルドでは 63 KB）減り、Slow 4G の初回ロードで約 0.5 秒に相当する。落とすのは
# `name` セクションだけで、構造が想定外ならスクリプトが何も書かずに失敗する。
python3 scripts/strip-wasm-names.py dist/wasm/shutoko_routing_bg.wasm
printf '%s\n' '{"type":"module","private":true}' > dist/wasm/package.json
cp crates/routing-wasm/types/index.d.ts dist/wasm/index.d.ts
cp crates/routing-wasm/wasm-contract.json dist/wasm/wasm-contract.json
