#!/usr/bin/env bash
set -euo pipefail

# scripts/fetch-osm.sh
# Fetch Tokyo Inner Circular Route (C1), connecting ramps, and surrounding major surface streets
# from OpenStreetMap via Overpass API [out:json].
#
# Usage:
#   ./scripts/fetch-osm.sh [OUTPUT_PATH] [ENDPOINT]
#
# Defaults:
#   OUTPUT_PATH: fixtures/osm/shutoko-c1.json
#   ENDPOINT:    https://overpass-api.de/api/interpreter

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

OUTPUT_PATH="${1:-"${REPO_ROOT}/fixtures/osm/shutoko-c1.json"}"
ENDPOINT="${2:-"https://overpass-api.de/api/interpreter"}"

mkdir -p "$(dirname "${OUTPUT_PATH}")"

OVERPASS_QUERY='[out:json][timeout:90];
relation(4256008) -> .c1;
(
  .c1;
  way(r.c1);
  node(w);
) -> .c1_all;
node.c1_all -> .c1_nodes;
way(bn.c1_nodes)["highway"="motorway_link"] -> .direct_links;
(
  .direct_links;
  node(w.direct_links);
) -> .links_step1;
node.links_step1 -> .links_nodes1;
way(bn.links_nodes1)["highway"="motorway_link"] -> .all_links;
node(w.all_links) -> .all_link_nodes;
(
  way["highway"~"^(trunk|primary|secondary)$"](35.65,139.735,35.695,139.78);
  way(bn.all_link_nodes)["highway"~"^(tertiary|residential|unclassified)$"];
) -> .local_ways;
(
  .c1_all;
  .all_links;
  node(w.all_links);
  .local_ways;
  node(w.local_ways);
  relation(bw.c1_all)["type"="restriction"];
  relation(bw.all_links)["type"="restriction"];
  relation(bw.local_ways)["type"="restriction"];
);
out body;'

echo "Fetching OSM data from ${ENDPOINT}..."
curl -sS -f -X POST \
  --data-urlencode "data=${OVERPASS_QUERY}" \
  "${ENDPOINT}" \
  -o "${OUTPUT_PATH}"

QUERY_SHA256=$(printf "%s" "${OVERPASS_QUERY}" | shasum -a 256 | awk '{print $1}')
FILE_SIZE=$(wc -c < "${OUTPUT_PATH}" | tr -d ' ')
FETCH_TIME=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

echo "Successfully fetched OSM data to ${OUTPUT_PATH}"
echo "  Timestamp (UTC): ${FETCH_TIME}"
echo "  Endpoint:        ${ENDPOINT}"
echo "  Query SHA-256:   ${QUERY_SHA256}"
echo "  File size:       ${FILE_SIZE} bytes"
