#!/usr/bin/env bash
set -euo pipefail

# scripts/generate-fixtures.sh
# Deterministically rebuild the generated release fixtures (graph, snap-index,
# ramps, manifest, od-tariffs, pair-candidates) from fixtures/osm/shutoko-all.json
# and the data/*.json inputs. The default release is all-real-v4, whose manifest
# binds five artifacts and records billingPairsVersion=v3 plus tariffModelVersion=1.
# Older releases stay reproducible by passing the release id as the 7th argument
# (rollback targets all-real-v3 / all-real-v2).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

OUT_DIR="${1:-"${REPO_ROOT}/fixtures/generated"}"
OSM_PATH="${2:-"${REPO_ROOT}/fixtures/osm/shutoko-all.json"}"
SEED_PATH="${3:-"${REPO_ROOT}/data/billing-pairs-seed.json"}"
INVENTORY_PATH="${4:-"${REPO_ROOT}/data/ramp-inventory.json"}"
BINDINGS_PATH="${5:-"${REPO_ROOT}/data/osm-ramp-bindings.json"}"
TARIFFS_PATH="${6:-"${REPO_ROOT}/data/od-tariffs.json"}"
ADJACENCY_PATH="${SHUTOKO_ADJACENCY_PATH:-"${REPO_ROOT}/data/billing-pair-adjacency.json"}"
SUPPORT_DECISIONS_PATH="${SHUTOKO_SUPPORT_DECISIONS_PATH:-"${REPO_ROOT}/data/ramp-support-decisions.json"}"
RELEASE_ID="${7:-"${SHUTOKO_RELEASE_ID:-all-real-v4}"}"

cargo run --manifest-path "${REPO_ROOT}/Cargo.toml" --bin shutoko-graph-builder --locked -- \
  --osm "${OSM_PATH}" \
  --seed "${SEED_PATH}" \
  --inventory "${INVENTORY_PATH}" \
  --bindings "${BINDINGS_PATH}" \
  --tariffs "${TARIFFS_PATH}" \
  --adjacency "${ADJACENCY_PATH}" \
  --support-decisions "${SUPPORT_DECISIONS_PATH}" \
  --out-dir "${OUT_DIR}" \
  --release-id "${RELEASE_ID}" \
  --built-at "2026-09-24T00:00:00Z" \
  --source-date "2026-09-16" \
  --vehicle-profile "passenger-car-etc" \
  --coverage-area "Metropolitan Expressway network (Tokyo, Kanagawa, Saitama)" \
  --graph-version "1.0.0"
