#!/usr/bin/env bash
set -euo pipefail

# scripts/generate-fixtures.sh
# Deterministically rebuild generated graph, snap-index, and manifest fixtures
# from fixtures/osm/shutoko-c1.json and data/billing-pairs-seed.json.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

OUT_DIR="${1:-"${REPO_ROOT}/fixtures/generated"}"
OSM_PATH="${2:-"${REPO_ROOT}/fixtures/osm/shutoko-c1.json"}"
SEED_PATH="${3:-"${REPO_ROOT}/data/billing-pairs-seed.json"}"

cargo run --manifest-path "${REPO_ROOT}/Cargo.toml" --bin shutoko-graph-builder --locked -- \
  --osm "${OSM_PATH}" \
  --seed "${SEED_PATH}" \
  --out-dir "${OUT_DIR}" \
  --release-id "c1-real-v1" \
  --built-at "2026-09-10T00:00:00Z" \
  --source-date "2026-09-10" \
  --vehicle-profile "passenger-car-etc" \
  --coverage-area "Tokyo Inner Circular Route (C1) and connecting ramps" \
  --graph-version "1.0.0"
