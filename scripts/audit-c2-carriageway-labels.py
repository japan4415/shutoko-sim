#!/usr/bin/env python3
"""Audit the C2 carriageway labels against the official ramp directions.

graph-builder derives the inner/outer carriageway split from ramp reachability,
which fails closed near JCTs and leaves 169 of 2,479 relation edges unlabelled.
This audit re-derives the labels from a different chain of evidence:

1. Anchors. For every verified_bound C2 ramp the junction with the mainline is
   found topologically: an entry ramp walks forward from its merge node, an exit
   ramp walks backward from its diverge point, both over Shutoko edges that are
   not in the membership, until a node the membership touches. The membership
   edge there that runs with the ramp is the ramp's carriageway. Ramp directions
   come from the official route map via data/ramp-inventory.json.
2. Propagation. Geometry only: collinear continuation within 300 m stays on the
   same carriageway, an antiparallel pair within 60 m is the other one.
3. Gaps. Members whose nearest opposite-carriageway edge is far away are
   reported, because a ring cannot have one carriageway present without the
   other next to it.

    python3 scripts/audit-c2-carriageway-labels.py
"""

import json
import math
from collections import Counter, defaultdict
import heapq

REPO = "/Users/discord4415/Documents/hobby/shutoko-sim"
COLLINEAR_M = 300.0
COLLINEAR_COS = 0.99
PAIR_M = 60.0
PAIR_COS = 0.9
GAP_M = 150.0
WALK_BUDGET_M = 3000.0

# Junction nodes recorded by the three authored pairs, used to validate the
# junction search below (data/billing-pair-adjacency.json anchors).
KNOWN_JUNCTIONS = {
    "ramp:c2-outer:gotanda-entry": "n:3387909571",
    "ramp:c2-outer:hatsudai-minami-exit": "n:919617354",
    "ramp:c2-inner:takinogawa-entry": "n:4597822558",
    "ramp:c2-inner:nakano-chojabashi-exit": "n:3851530202",
    "ramp:c2-outer:kosuge-entry": "n:370270147",
    "ramp:c2-outer:hirai-ohashi-exit": "n:698279318",
}


def load():
    graph = json.load(open(f"{REPO}/fixtures/generated/graph.json"))
    nodes = {
        n["id"]: (n["lat"] * 111320.0, n["lon"] * 111320.0 * math.cos(math.radians(n["lat"])))
        for n in graph["nodes"]
    }
    edges = {e["id"]: e for e in graph["edges"]}
    membership = {}
    for m in graph["routeMemberships"]:
        if not m["membershipId"].startswith("route:C2"):
            continue
        for s in m["segments"]:
            if s["sourceKind"] != "relationMainline":
                continue
            for eid in s["orderedEdgeIds"]:
                membership[eid] = m["direction"]
    inventory = json.load(open(f"{REPO}/data/ramp-inventory.json"))
    bindings = {
        b["rampId"]: b
        for b in json.load(open(f"{REPO}/data/osm-ramp-bindings.json"))["bindings"]
    }
    return graph, nodes, edges, membership, inventory, bindings


def way_of(edge_id):
    return int(edge_id.split(":")[1][1:])


def segment_index(edge_id):
    return int(edge_id.split(":")[2])


def geometry(edge, nodes):
    a, b = nodes[edge["from"]], nodes[edge["to"]]
    mid = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
    dx, dy = b[0] - a[0], b[1] - a[1]
    norm = math.hypot(dx, dy) or 1.0
    return mid, (dx / norm, dy / norm)


def main():
    graph, nodes, edges, membership, inventory, bindings = load()
    member_nodes = defaultdict(list)
    for eid in membership:
        member_nodes[edges[eid]["from"]].append(eid)
        member_nodes[edges[eid]["to"]].append(eid)
    out_edges = defaultdict(list)
    in_edges = defaultdict(list)
    for e in graph["edges"]:
        if e["kind"] == "shutoko":
            out_edges[e["from"]].append(e["id"])
            in_edges[e["to"]].append(e["id"])
    way_edges = defaultdict(list)
    for e in graph["edges"]:
        if e["kind"] in ("entry", "exit", "shutoko"):
            way_edges[way_of(e["id"])].append(e)
    for eids in way_edges.values():
        eids.sort(key=lambda e: segment_index(e["id"]))

    def junction(start, forward):
        """First membership node reachable over non-membership Shutoko edges."""
        if start in member_nodes:
            return start, 0.0
        best = {start: 0.0}
        heap = [(0.0, start)]
        while heap:
            distance, node = heapq.heappop(heap)
            if distance > best.get(node, 1e18):
                continue
            step = out_edges if forward else in_edges
            for eid in step.get(node, ()):
                if eid in membership:
                    continue
                edge = edges[eid]
                nxt = edge["to"] if forward else edge["from"]
                walked = distance + edge["distanceMeters"]
                if walked > WALK_BUDGET_M:
                    continue
                if nxt in member_nodes:
                    return nxt, walked
                if walked < best.get(nxt, 1e18):
                    best[nxt] = walked
                    heapq.heappush(heap, (walked, nxt))
        return None, None

    anchors = {}
    conflicts = []
    missing = []
    for ramp in sorted(inventory["ramps"], key=lambda r: r["rampId"]):
        ramp_id = ramp["rampId"]
        if not ramp_id.startswith("ramp:c2-") or ramp.get("supportState") != "verified_bound":
            continue
        binding = bindings.get(ramp_id)
        eids = way_edges.get(binding["osmWayId"], []) if binding else []
        if not eids:
            missing.append((ramp_id, "no graph edge for the ramp way"))
            continue
        is_exit = ramp["kind"] in ("general_exit", "boundary_out")
        ramp_edge = eids[0] if is_exit else eids[-1]
        node, walked = junction(
            ramp_edge["from"] if is_exit else ramp_edge["to"], not is_exit
        )
        expected = KNOWN_JUNCTIONS.get(ramp_id)
        if expected is not None and node != expected:
            conflicts.append((ramp_id, f"junction search found {node}, expected {expected}"))
        if node is None:
            missing.append((ramp_id, f"no membership node within {WALK_BUDGET_M:.0f} m"))
            continue
        ramp_heading = geometry(ramp_edge, nodes)[1]
        candidates = []
        for eid in member_nodes[node]:
            edge = edges[eid]
            if is_exit and edge["to"] != node:
                continue
            if not is_exit and edge["from"] != node:
                continue
            heading = geometry(edge, nodes)[1]
            candidates.append((heading[0] * ramp_heading[0] + heading[1] * ramp_heading[1], eid))
        candidates.sort(reverse=True)
        if candidates and candidates[0][0] > 0.5:
            eid = candidates[0][1]
            anchors[eid] = ramp["direction"]
            if membership[eid] != ramp["direction"]:
                conflicts.append(
                    (
                        ramp_id,
                        f"anchor edge {eid} is labelled {membership[eid]} but the ramp is {ramp['direction']}",
                    )
                )
        else:
            missing.append((ramp_id, f"no aligned membership edge at junction {node}"))

    print(f"anchors placed: {len(anchors)}")
    print(f"validation against the three authored pairs: {len(KNOWN_JUNCTIONS)} junctions checked")
    print(f"anchor conflicts with the derived split: {len(conflicts)}")
    for ramp_id, message in conflicts:
        print(f"  ! {ramp_id}: {message}")
    for ramp_id, why in missing:
        print(f"  - {ramp_id}: {why}")

    # geometry-only propagation from the anchors
    geo = {eid: geometry(edges[eid], nodes) for eid in membership}
    grid = defaultdict(list)
    cell = 0.003

    def cell_of(point):
        return (int(point[0] // (cell * 111320)), int(point[1] // (cell * 111320)))

    for eid in membership:
        grid[cell_of(geo[eid][0])].append(eid)
    classified = dict(anchors)

    def flip(direction):
        return "outer" if direction == "inner" else "inner"

    for _ in range(300):
        added = 0
        for eid, direction in list(classified.items()):
            mid, heading = geo[eid]
            cx, cy = cell_of(mid)
            for dx in (-2, -1, 0, 1, 2):
                for dy in (-2, -1, 0, 1, 2):
                    for other in grid.get((cx + dx, cy + dy), ()):
                        if other in classified:
                            continue
                        omid, oheading = geo[other]
                        distance = math.hypot(omid[0] - mid[0], omid[1] - mid[1])
                        cos = heading[0] * oheading[0] + heading[1] * oheading[1]
                        if cos >= COLLINEAR_COS and distance <= COLLINEAR_M:
                            classified[other] = direction
                            added += 1
                        elif cos <= -PAIR_COS and distance <= PAIR_M:
                            classified[other] = flip(direction)
                            added += 1
        if not added:
            break
    print(f"classified membership edges: {len(classified)}/{len(membership)}")

    osm = json.load(open(f"{REPO}/fixtures/osm/shutoko-all.json"))
    ways = {el["id"]: el for el in osm["elements"] if el["type"] == "way"}
    relation = next(
        el for el in osm["elements"] if el["type"] == "relation" and el["id"] == 4256077
    )
    role_of = {
        r["wayId"]: r["role"]
        for r in json.load(open(f"{REPO}/data/osm-relation-repairs.json"))["repairs"]
        if r["kind"] == "setMemberRole"
    }

    counts = Counter()
    disagreements = []
    gaps = []
    by_way = defaultdict(list)
    for eid in membership:
        by_way[way_of(eid)].append(eid)
    for way_id, eids in sorted(by_way.items()):
        votes = Counter(classified[e] for e in eids if e in classified)
        if not votes:
            counts["unclassified"] += 1
            continue
        winner, count = votes.most_common(1)[0]
        role = role_of.get(way_id)
        role_direction = None
        if role:
            role_direction = "inner" if role.startswith("Inner") else "outer"
        if role_direction == winner and count == len(eids):
            counts["confirmed"] += 1
        elif role_direction == winner:
            counts["confirmed_partial"] += 1
        elif role is None:
            counts["no_role_in_file"] += 1
        else:
            counts["disagrees"] += 1
            disagreements.append((way_id, role_direction, winner, dict(votes), len(eids)))
        # gap check: how far is the nearest edge of the opposite carriageway?
        if winner not in ("inner", "outer"):
            continue
        nearest = None
        for e in eids:
            mid = geo[e][0]
            for other, direction in membership.items():
                if direction == winner:
                    continue
                omid = geo[other][0]
                distance = math.hypot(omid[0] - mid[0], omid[1] - mid[1])
                if nearest is None or distance < nearest[0]:
                    nearest = (distance, way_of(other))
        if nearest and nearest[0] > GAP_M:
            gaps.append((way_id, winner, round(nearest[0]), nearest[1]))

    print(f"per member way: {dict(counts)}")
    print("disagreements (file vs geometry):")
    for way_id, role_direction, winner, votes, total in disagreements:
        tags = ways.get(way_id, {}).get("tags", {})
        print(
            f"  {way_id:<12} file={role_direction:<6} geometry={winner:<6} "
            f"votes={votes}/{total} layer={tags.get('layer')} destination={tags.get('destination') or '-'}"
        )
    print(f"members whose opposite carriageway is farther than {GAP_M:.0f} m away:")
    for way_id, winner, distance, other in gaps:
        print(f"  {way_id:<12} labelled {winner:<6} nearest {other} edge at {distance} m")


if __name__ == "__main__":
    main()
