#!/usr/bin/env bash
set -euo pipefail

# scripts/fetch-osm.sh
# Fetch Shutoko (Tokyo Metropolitan Expressway) routes and their entry/exit ramps
# from OpenStreetMap via Overpass API [out:json].
#
# Local surface streets are intentionally excluded: the routing model assumes
# "board at the nearest entrance" so there is no need to solve surface-road paths.
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

# Overpass query rationale:
# 1. Target expressway relations are listed in the relation(id:...) filter.
#    To add a new route (e.g. C2, Wangan), append its relation ID to that list.
#    Current targets:
#      4256008 — C1 Inner Circular Route (都心環状線)
# 2. Motorway link expansion: entry/exit ramps consist of 3–5 successive
#    motorway_link ways. Four hops of expansion capture all ramp geometry.
# 3. Turn restrictions for the expressway ways and ramps are included so the
#    router can honour prohibited manoeuvres.
# 4. No bbox or local-road queries: general surface streets are not needed.
OVERPASS_QUERY='[out:json][timeout:90];
relation(id:4256008) -> .expressways;
(
  .expressways;
  way(r.expressways);
  node(w);
) -> .ew_all;
node.ew_all -> .ew_nodes;

way(bn.ew_nodes)["highway"="motorway_link"] -> .links;

( .links; node(w.links); ) -> .l_nodes;
( .links; way(bn.l_nodes)["highway"="motorway_link"]; ) -> .links;

( .links; node(w.links); ) -> .l_nodes;
( .links; way(bn.l_nodes)["highway"="motorway_link"]; ) -> .links;

( .links; node(w.links); ) -> .l_nodes;
( .links; way(bn.l_nodes)["highway"="motorway_link"]; ) -> .links;

( .links; node(w.links); ) -> .l_nodes;
( .links; way(bn.l_nodes)["highway"="motorway_link"]; ) -> .links;

(
  .ew_all;
  .links;
  node(w.links);
  relation(bw.ew_all)["type"="restriction"];
  relation(bw.links)["type"="restriction"];
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
