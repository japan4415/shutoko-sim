#!/usr/bin/env bash
set -euo pipefail

# scripts/fetch-osm.sh
# Fetch Shutoko (Tokyo Metropolitan Expressway) routes and their entry/exit ramps
# from OpenStreetMap via Overpass API [out:json].
#
# Surface streets are fetched as classification context but are NOT used in the
# routing graph. The routing model assumes "board at the nearest entrance" so
# there is no need to solve surface-road paths. The graph builder identifies
# context-only ways by their highway type (anything other than motorway /
# motorway_link) and uses them solely for entry/exit ramp classification.
#
# Usage:
#   ./scripts/fetch-osm.sh [OUTPUT_PATH] [ENDPOINT]
#
# Defaults:
#   OUTPUT_PATH: fixtures/osm/shutoko-all.json
#   ENDPOINT:    https://overpass-api.de/api/interpreter

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

OUTPUT_PATH="${1:-"${REPO_ROOT}/fixtures/osm/shutoko-all.json"}"
ENDPOINT="${2:-"https://overpass-api.de/api/interpreter"}"

mkdir -p "$(dirname "${OUTPUT_PATH}")"

# Overpass query rationale:
# 1. Target all relations tagged network=首都高速道路, with explicit relation
#    IDs retained for known tagging gaps. ROUTES_FILTER=c1 remains available for
#    focused diagnostics only; it is not the release default.
# 2. Motorway link expansion: entry/exit ramps consist of 3–5 successive
#    motorway_link ways. Four hops of expansion capture all ramp geometry.
# 3. Turn restrictions for the expressway ways and ramps are included so the
#    router can honour prohibited manoeuvres.
# 4. Surface-road context (final hop): after the 4-hop motorway_link expansion,
#    one additional hop fetches every highway way that shares a node with the
#    motorway_link ways (NOT the main-line motorway ways). This lets the graph
#    builder distinguish real exit ramps (terminal node shared with a surface-road
#    way) from JCT connectors (terminal node shared only with motorway/motorway_link
#    or with nothing). Scope is limited to motorway_link nodes because ramp termini
#    are always nodes of motorway_link ways; expanding from main-line motorway nodes
#    would pull in large numbers of surface roads running under the elevated C1 loop
#    that contribute nothing to terminus classification. Context ways are output in
#    a separate "out body" pass without their node elements — the builder needs only
#    the nd-ref lists (contained in way elements) for node-ID matching, not
#    coordinates. Context ways are NOT added to the routing graph.
# Target expressway selection:
# By default, fetch all routes of the Tokyo Metropolitan Expressway network via
# relation["network"="首都高速道路"], plus the few route relations whose tags lack that
# network value (the two 神奈川7号 relations below).
#
# The network master relation 4256559 (type=network, name=首都高速道路) lists 26 route
# relations carrying 23 distinct route codes:
#   C1, C2, 1 (上野線 / 羽田線), 2, 3, 4, 5, 6 (向島線 / 三郷線), 7, 9, 10, 11, B, Y,
#   K1, K2, K3, K5, K6, K7 (横浜北線 / 横浜北西線), S1, S2, S5.
# K4 / S3 / S4 do not exist in the network, so no relation or ref carries them.
#
# Adding a route here is half of the job: the new route code must also be added to
# SHUTOKO_REFS in crates/graph-builder/src/topology.rs, otherwise its mainline ways are
# dropped from the graph silently. crates/graph-builder/tests/shutoko_ref_coverage.rs fails
# until both sides agree.
# For backward compatibility or focused extraction, specific relation IDs
# (e.g. 4256008 for C1) can be targeted via ROUTES_FILTER.
ROUTES_FILTER="${ROUTES_FILTER:-"all"}"

if [ "${ROUTES_FILTER}" = "c1" ]; then
  EXPRESSWAYS_QUERY='relation(id:4256008) -> .expressways;'
else
  # Comprehensive query for all Shutoko routes:
  # Captures all routes tagged network=首都高速道路 plus the explicitly listed relations that
  # the network tag misses (the two 神奈川7号 lines). C1 / C2 are listed explicitly as
  # belt-and-braces; both already carry the network tag.
  EXPRESSWAYS_QUERY='(
    relation["network"="首都高速道路"];
    relation(id:4256008); // C1
    relation(id:4256077); // C2
    relation(id:10355798,10732984); // K7 Yokohama North / Northwest lines
  ) -> .expressways;'
fi

OVERPASS_QUERY="$(cat <<EOF
[out:json][timeout:300];
${EXPRESSWAYS_QUERY}
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

// Context-only final hop: surface highway ways sharing a node with any of
// the motorway_link ways captured above (NOT the main-line motorway ways).
// Scope rationale: ramp termini are always motorway_link nodes. Expanding
// from main-line motorway nodes would pull in surface roads under the
// elevated C1 loop that never serve as ramp termini, inflating query cost
// for zero classification benefit.
// motorway and motorway_link are excluded because they are already present
// in .ew_all / .links; re-fetching them would be redundant.
// All other highway values (trunk, primary, secondary, tertiary,
// unclassified, residential, service, living_street, etc.) are left
// unrestricted: any surface type can be the landing road of an exit ramp,
// so type-based filtering risks misclassifying a real exit as a JCT.
node(w.links) -> .link_nodes;
way(bn.link_nodes)["highway"]["highway"!="motorway"]["highway"!="motorway_link"]
  -> .ctx_ways;

// Routing elements: full geometry and tags needed by the graph builder.
(
  .ew_all;
  .links;
  node(w.links);
  relation(bw.ew_all)["type"="restriction"];
  relation(bw.links)["type"="restriction"];
);
out body;
// Context ways: output way elements only (tags + nd refs).
// "out body" for ways includes the nodes[] array (nd refs) which is all
// the builder needs for terminus-node detection. Node coordinates are NOT
// output — the builder performs node-ID containment checks, not geometry
// operations, so omitting node(w.ctx_ways) saves the bulk of the extra data.
( .ctx_ways; );
out body;
EOF
)"

echo "Fetching OSM data from ${ENDPOINT}..."
curl -sS -f -X POST \
  -H "User-Agent: ShutokoSim-DataPipeline/1.0 (https://github.com/japan4415/shutoko-sim)" \
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
