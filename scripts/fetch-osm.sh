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
# 2. Motorway link expansion (Issue #39): entry/exit ramps consist of 3–5 successive
#    motorway_link ways, but large junctions can chain more. The expansion is therefore a
#    *contact hop* (ways sharing a node with the mainline) plus SHUTOKO_LINK_HOPS further
#    hops, and the cap is probed after every fetch (see below) so a truncation cannot pass
#    silently. Measured 2026-10-12: from C1 the cap leaves nothing behind, while from 湾岸線
#    B one hop beyond a six-way chain still reaches 12 link ways — partly the link network
#    of other expressways at shared junctions, which is why the probe prints names and refs
#    instead of just a count. Raise SHUTOKO_LINK_HOPS when the probe reports ways that
#    belong to a Shutoko ramp, then re-fetch.
# 3. Turn restrictions for the expressway ways and ramps are included so the
#    router can honour prohibited manoeuvres.
# 4. Surface-road context (final hop): after the motorway_link expansion,
#    one additional hop fetches every highway way that shares a node with the
#    motorway_link ways (NOT the main-line motorway ways). This lets the graph
#    builder distinguish real exit ramps (terminal node shared with a surface-road
#    way) from JCT connectors (terminal node shared only with motorway/motorway_link
#    or with nothing). Scope is limited to motorway_link nodes because ramp termini
#    are always nodes of motorway_link ways; expanding from main-line motorway nodes
#    would pull in large numbers of surface roads running under the elevated C1 loop
#    that contribute nothing to terminus classification. Context ways are output with
#    their node elements (way tags + nd refs + node coordinates) so that the builder can
#    evaluate the surface-connection signal per location (issue #37): a shared node proves
#    a street connection, but its absence only counts as evidence where the extract maps
#    streets in that endpoint's own neighbourhood. Context ways are NOT added to the
#    routing graph.
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

# Motorway_link expansion depth (Issue #39). The contact hop (ways sharing a node with
# the mainline) is always taken; SHUTOKO_LINK_HOPS further hops follow, so the cap is
# "chains of up to SHUTOKO_LINK_HOPS + 1 link ways". The default matches the documented
# ramp structure (3–5 successive motorway_link ways). Raising it is a deliberate act:
# check what the hop probe (below) reports beyond the cap first.
SHUTOKO_LINK_HOPS="${SHUTOKO_LINK_HOPS:-4}"

LINK_HOP_BLOCKS=""
hop_index=0
while [ "${hop_index}" -lt "${SHUTOKO_LINK_HOPS}" ]; do
  LINK_HOP_BLOCKS="${LINK_HOP_BLOCKS}$(cat <<'BLOCK'
( .links; node(w.links); ) -> .l_nodes;
( .links; way(bn.l_nodes)["highway"="motorway_link"]; ) -> .links;
BLOCK
)"$'\n'
  hop_index=$((hop_index + 1))
done

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

${LINK_HOP_BLOCKS}
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
// Context ways: way elements (tags + nd refs) plus the coordinates of their nodes.
// The way elements carry the nd-ref lists the builder needs for shared-node matching; the node
// elements are what let it decide whether the *absence* of a shared node is evidence at all
// (issue #37). Without them every surface node that has coordinates is also a ramp/mainline
// node, so "no shared node" degrades from "no street connection" to "the extract cannot tell",
// and the gap is reported as incomplete-surface-evidence in the manifest.
( .ctx_ways; node(w.ctx_ways); );
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

# --- Surface-geometry check (Issue #37) ----------------------------------------------
# The context-ways pass must return the coordinates of the surface-road nodes it fetches, not
# just the way elements: the graph builder can prove a street connection from a shared node,
# but it can only *disprove* one where it can see that streets are mapped around the ramp
# endpoint. An extract without that geometry makes every unshared endpoint undecidable and
# every release manifest report incomplete-surface-evidence, so check the fetched file locally
# (no network) and say so instead of letting the degradation pass silently.
python3 - "${OUTPUT_PATH}" <<'PY'
import json
import sys

# Mirrors is_vehicle_highway() in crates/graph-builder/src/topology.rs: only these highway
# values count as vehicle-accessible surface-road evidence for the builder. Keep the two in
# step — a surface way this check accepts but the builder rejects would hide the degradation.
VEHICLE_HIGHWAYS = {
    "trunk", "trunk_link", "primary", "primary_link", "secondary", "secondary_link",
    "tertiary", "tertiary_link", "unclassified", "residential", "living_street",
}
PUBLIC_SERVICE_SUBTAGS = {"alley"}

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

motorway_nodes: set = set()
surface_nodes: set = set()
node_ids: set = set()
for element in payload.get("elements", []):
    if element.get("type") == "node":
        node_ids.add(element["id"])
        continue
    if element.get("type") != "way":
        continue
    tags = element.get("tags") or {}
    highway = tags.get("highway")
    if highway in ("motorway", "motorway_link"):
        motorway_nodes.update(element.get("nodes") or [])
    elif highway in VEHICLE_HIGHWAYS or (
        highway == "service" and tags.get("service") in PUBLIC_SERVICE_SUBTAGS
    ):
        surface_nodes.update(element.get("nodes") or [])

# Street geometry = vehicle-accessible surface-road nodes that the motorway network does not
# already cover, i.e. nodes that only exist because the context ways were fetched together with
# their geometry.
street_only = [node for node in surface_nodes - motorway_nodes if node in node_ids]
print(f"  Surface-road nodes outside the motorway network with coordinates: {len(street_only)}")
if not street_only:
    print(
        "  WARNING: the extract carries no surface-road node geometry. Ramp endpoint classification\n"
        "  will report every unshared endpoint as missing per-location surface evidence\n"
        "  (incomplete-surface-evidence in the manifest) and fall back to OSM node tags. Check that\n"
        "  the context-ways pass still ends with 'node(w.ctx_ways);' before publishing a release.",
        file=sys.stderr,
    )
PY

# --- Hop-cap probe (Issue #39) -------------------------------------------------------
# The expansion walks at most ${SHUTOKO_LINK_HOPS} hops beyond the contact hop. If a
# motorway_link way exists one hop further, part of a ramp's geometry is missing from the
# extract and nothing else would notice (the builder only sees what was fetched). Probe
# one hop beyond the cap with the same expansion and report what lies outside, with names,
# so a Shutoko ramp is distinguishable from another expressway's link complex at a shared
# junction. Skipping it keeps the fetch to a single Overpass request:
#   SHUTOKO_SKIP_HOP_PROBE=1 ./scripts/fetch-osm.sh
if [ "${SHUTOKO_SKIP_HOP_PROBE:-0}" = "1" ]; then
  echo "Hop-cap probe skipped (SHUTOKO_SKIP_HOP_PROBE=1)."
else
  HOP_PROBE_QUERY="$(cat <<EOF
[out:json][timeout:300];
${EXPRESSWAYS_QUERY}
( .expressways; way(r.expressways); node(w); ) -> .ew_all;
node.ew_all -> .ew_nodes;
way(bn.ew_nodes)["highway"="motorway_link"] -> .links;
${LINK_HOP_BLOCKS}( .links; node(w.links); ) -> .l_nodes;
way(bn.l_nodes)["highway"="motorway_link"] -> .probe;
( .probe; - .links; ) -> .overflow;
.overflow out tags;
EOF
)"
  HOP_PROBE_OUTPUT="$(mktemp)"
  trap 'rm -f "${HOP_PROBE_OUTPUT}"' EXIT
  echo "Probing the ${SHUTOKO_LINK_HOPS}-hop motorway_link cap (one hop beyond)..."
  if curl -sS -f -X POST \
    -H "User-Agent: ShutokoSim-DataPipeline/1.0 (https://github.com/japan4415/shutoko-sim)" \
    --data-urlencode "data=${HOP_PROBE_QUERY}" \
    "${ENDPOINT}" \
    -o "${HOP_PROBE_OUTPUT}"; then
    python3 - "${HOP_PROBE_OUTPUT}" "${SHUTOKO_LINK_HOPS}" <<'PY'
import json
import sys

path, hops = sys.argv[1], sys.argv[2]
with open(path, encoding="utf-8") as handle:
    payload = json.load(handle)
ways = [element for element in payload.get("elements", []) if element.get("type") == "way"]
print(f"  ways one hop beyond the {hops}-hop cap: {len(ways)}")
if not ways:
    print("  no truncation detected: every motorway_link way adjacent to the expansion is already fetched")
    raise SystemExit(0)
for way in ways[:20]:
    tags = way.get("tags") or {}
    name = tags.get("name") or tags.get("name:ja") or "<unnamed>"
    ref = tags.get("ref")
    print(f"    way {way.get('id')}: {name}{'' if ref is None else f' (ref {ref})'}")
if len(ways) > 20:
    print(f"    ... and {len(ways) - 20} more")
print(
    "  WARNING: ramp geometry may be truncated. Check whether these ways belong to a\n"
    "  Shutoko ramp; if they do, raise SHUTOKO_LINK_HOPS, re-fetch, and re-run the probe."
)
PY
  else
    echo "  WARNING: the hop-cap probe could not run (Overpass request failed);" >&2
    echo "  the extract may be truncated without notice. Re-run the probe before publishing." >&2
  fi
fi
