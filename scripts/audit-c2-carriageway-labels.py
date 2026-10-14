#!/usr/bin/env python3
"""Audit the C2 carriageway labels against the official ramp directions.

graph-builder derives the inner/outer carriageway split from ramp reachability,
which fails closed near JCTs.  This audit re-derives the labels from a different
chain of evidence:

1. Anchors.  For every verified_bound C2 ramp the junction with the mainline is
   found topologically: an entry ramp walks forward from its merge node, an exit
   ramp walks backward from its diverge point, both over Shutoko edges that are
   not in the membership, until a node the membership touches.  The membership
   edge there that runs with the ramp is the ramp's carriageway.  Ramp directions
   come from the official route map via data/ramp-inventory.json.
2. Topology.  Two membership edges that share a node and continue each other in
   the one-way direction are the same carriageway.  This is exact (no distance
   threshold) and is what resolves the sections where the two carriageways share
   coordinates, e.g. inside the 大井JCT tunnel where way 331692346 (layer 2) and
   way 250494182 (layer 3) overlap: 2D geometry cannot tell them apart, but the
   directed chain into the 品川線 outer bore (way 805363106, anchored by the
   official 五反田 *outer* entry) pins 331692346 to outer.
3. Geometry.  Adjacent members only: within 60 m a collinear pair is the same
   carriageway and an antiparallel pair is the other one.  Pairs whose ways carry
   different `layer` tags are reported as stacked pairs, because there the two
   carriageways are separated vertically rather than horizontally.

The three constraints plus the anchors are solved as one parity problem
(union-find with parity), so the answer does not depend on propagation order and
a contradiction shows up as a cycle instead of a silently wrong label.

    python3 scripts/audit-c2-carriageway-labels.py [--json PATH]
"""

import argparse
import heapq
import json
import math
from collections import Counter, defaultdict
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from graph_wire import load_graph  # noqa: E402  (repo-local helper)

REPO = Path(__file__).resolve().parent.parent
GEOM_M = 60.0
COLLINEAR_COS = 0.99
PAIR_COS = 0.9
TURN_COS = 0.5
GAP_M = 150.0
WALK_BUDGET_M = 3000.0
CELL_DEG = 0.003

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

# Ways the resolution depends on and that a mapper should look at, with the
# coordinates of the stretch.  Reported so the audit output carries the field
# check list instead of it living only in prose.
FIELD_CHECK_SITES = [
    (331692346, 35.608, 139.755, "大井JCT tunnel: stacked with way 250494182 (layer 3)"),
    (250494182, 35.605, 139.754, "大井JCT tunnel: stacked with way 331692346 (layer 2)"),
]


def load():
    # schema 5（nodeIdDeltas + 位置インデックス）は reader と同じ規則で schema 4 形状へ
    # 正規化してから読む（scripts/graph_wire.py）。
    graph = load_graph()
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


def cell_of(point):
    return (int(point[0] // (CELL_DEG * 111320)), int(point[1] // (CELL_DEG * 111320)))


class Parity:
    """Union-find with parity: 0 = same carriageway, 1 = opposite."""

    def __init__(self, names):
        self.parent = {name: name for name in names}
        self.rank = {name: 0 for name in names}
        self.rel = {name: 0 for name in names}

    def find(self, name):
        path = []
        root = name
        while self.parent[root] != root:
            path.append(root)
            root = self.parent[root]
        parity = 0
        for node in reversed(path):
            parity ^= self.rel[node]
            self.parent[node] = root
            self.rel[node] = parity
        return root, self.rel[name]

    def union(self, left, right, parity):
        """Returns False when the constraint contradicts what is already known."""
        left_root, left_parity = self.find(left)
        right_root, right_parity = self.find(right)
        if left_root == right_root:
            return (left_parity ^ right_parity) == parity
        if self.rank[left_root] < self.rank[right_root]:
            left_root, right_root = right_root, left_root
            left_parity, right_parity = right_parity, left_parity
        self.parent[right_root] = left_root
        self.rel[right_root] = left_parity ^ right_parity ^ parity
        if self.rank[left_root] == self.rank[right_root]:
            self.rank[left_root] += 1
        return True


def solve(anchors, groups):
    """Solve the parity problem by priority.

    `groups` is an ordered list of (name, constraints): a constraint that cannot be
    satisfied together with the already-applied, higher-priority evidence is
    skipped and reported instead of silently relabelling a way.  Anchors come
    first (they are the official ramp directions), then the exact chain topology,
    then the geometry.

    A component's orientation is carried as `bias[root]`: the direction (0 inner,
    1 outer) of the edges at even parity from that root.  Merging two components
    re-derives the bias from whichever side was already pinned, so the labels do not
    depend on the order in which the constraints are applied.
    """
    names = set(anchors)
    for _, constraints in groups:
        for left, right, _ in constraints:
            names.add(left)
            names.add(right)
    union = Parity(sorted(names))
    bias = {}
    skipped = []
    for edge_id, direction in sorted(anchors.items()):
        value = 0 if direction == "inner" else 1
        root, parity = union.find(edge_id)
        if root in bias:
            if bias[root] ^ parity != value:
                skipped.append(("anchor", edge_id, f"##{direction}", 0, None, None))
        else:
            bias[root] = value ^ parity

    def direction_of(edge_id):
        root, parity = union.find(edge_id)
        value = bias.get(root)
        return None if value is None else value ^ parity

    for group, constraints in groups:
        for left, right, parity in constraints:
            left_root, left_parity = union.find(left)
            right_root, right_parity = union.find(right)
            left_bias, right_bias = bias.get(left_root), bias.get(right_root)
            if left_root == right_root:
                if (left_parity ^ right_parity) != parity:
                    skipped.append((group, left, right, parity, None, None))
                    continue
            elif (
                left_bias is not None
                and right_bias is not None
                and (left_bias ^ right_bias) != (parity ^ left_parity ^ right_parity)
            ):
                skipped.append(
                    (
                        group,
                        left,
                        right,
                        parity,
                        "inner" if left_bias ^ left_parity == 0 else "outer",
                        "inner" if right_bias ^ right_parity == 0 else "outer",
                    )
                )
                continue
            union.union(left, right, parity)
            new_root, left_parity_after = union.find(left)
            if left_bias is not None:
                bias[new_root] = left_bias ^ left_parity ^ left_parity_after
            elif right_bias is not None:
                _, right_parity_after = union.find(right)
                bias[new_root] = right_bias ^ right_parity ^ right_parity_after

    labels = {}
    for name in sorted(names):
        value = direction_of(name)
        if value is not None:
            labels[name] = "inner" if value == 0 else "outer"
    return labels, skipped


def junction_search(edges, member_nodes, out_edges, in_edges, membership, start, forward):
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


def place_anchors(edges, nodes, membership, inventory, bindings, member_nodes, out_edges, in_edges, way_edges):
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
        node, walked = junction_search(
            edges, member_nodes, out_edges, in_edges, membership,
            ramp_edge["from"] if is_exit else ramp_edge["to"], not is_exit,
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
    return anchors, conflicts, missing


def chain_constraints(membership, edges, geo):
    """Directed-chain constraints: the exact evidence from the one-way topology.

    At a node the membership is read as a stream.  A node with one outgoing edge
    and one incoming edge continues the same carriageway, and at a merge (several
    incoming, one outgoing) every stream that arrives leaves as the same
    carriageway.  A fork (several outgoing) is left unconstrained: nothing there
    says which branch continues the stream, and a JCT link must not inherit the
    mainline label just because it starts on the mainline node.

    A join whose heading turns by more than 60 degrees is not a continuation of the
    same carriageway (a U-turn link or a crossing), so it is left unconstrained and
    reported separately.  The merges are returned as well so the caller can check
    afterwards whether a merge ended up joining two differently labelled streams --
    that is the signature of a junction where the two carriageways touch.
    """
    succ = defaultdict(list)
    pred = defaultdict(list)
    for eid in membership:
        succ[edges[eid]["from"]].append(eid)
        pred[edges[eid]["to"]].append(eid)

    def cos_between(left, right):
        left_heading, right_heading = geo[left][1], geo[right][1]
        return left_heading[0] * right_heading[0] + left_heading[1] * right_heading[1]

    same = []
    forks = []
    sharp = []
    merges = []
    for node in sorted(set(succ) | set(pred)):
        incoming = sorted(pred.get(node, ()))
        outgoing = sorted(succ.get(node, ()))
        if not incoming or not outgoing:
            continue

        def keep(left, right):
            if cos_between(left, right) >= TURN_COS:
                same.append((left, right))
            else:
                sharp.append((node, left, right, cos_between(left, right)))

        if len(outgoing) == 1:
            # Continuation or merge: every stream that arrives here leaves as the
            # same carriageway.
            for eid in incoming:
                keep(eid, outgoing[0])
            if len(incoming) > 1:
                merges.append((node, incoming, outgoing[0]))
            continue
        if len(incoming) == 1:
            # Fork: the node branches, so the audit cannot tell which outgoing edge
            # continues the stream.  Leave them all free (the geometry pairs them
            # later if it can) instead of letting the smoothest heading decide.
            forks.append((node, None, incoming[0], outgoing))
            continue
        # Both sides branch: nothing here says which stream continues into which, so
        # the junction is left unconstrained and reported.
        forks.append((node, None, None, incoming + outgoing))
    return same, forks, sharp, merges


def geometry_constraints(membership, geo):
    """Adjacent-member constraints for the two carriageways of one station.

    Only pairs from different ways are used: continuity inside a way is already
    exact through the chain, and a way that doubles back on itself would otherwise
    contradict it.
    """
    grid = defaultdict(list)
    for eid in membership:
        grid[cell_of(geo[eid][0])].append(eid)
    order = sorted(membership)
    same = []
    opposite = []
    for eid in order:
        mid, heading = geo[eid]
        cx, cy = cell_of(mid)
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                for other in grid.get((cx + dx, cy + dy), ()):
                    if other <= eid or way_of(other) == way_of(eid):
                        continue
                    omid, oheading = geo[other]
                    distance = math.hypot(omid[0] - mid[0], omid[1] - mid[1])
                    if distance > GEOM_M:
                        continue
                    cos = heading[0] * oheading[0] + heading[1] * oheading[1]
                    if cos >= COLLINEAR_COS:
                        same.append((eid, other))
                    elif cos <= -PAIR_COS:
                        opposite.append((eid, other))
    return same, opposite


def audit():
    """Run the audit and return the machine-readable result."""
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

    anchors, conflicts, missing = place_anchors(
        edges, nodes, membership, inventory, bindings, member_nodes, out_edges, in_edges, way_edges
    )
    print(f"anchors placed: {len(anchors)}")
    print(f"validation against the three authored pairs: {len(KNOWN_JUNCTIONS)} junctions checked")
    print(f"anchor conflicts with the derived split: {len(conflicts)}")
    for ramp_id, message in conflicts:
        print(f"  ! {ramp_id}: {message}")
    for ramp_id, why in missing:
        print(f"  - {ramp_id}: {why}")

    geo = {eid: geometry(edges[eid], nodes) for eid in membership}
    osm = json.load(open(f"{REPO}/fixtures/osm/shutoko-all.json"))
    ways = {el["id"]: el for el in osm["elements"] if el["type"] == "way"}
    layer_of = {wid: ways.get(wid, {}).get("tags", {}).get("layer") for wid in ways}
    chain_same, forks, sharp, merges = chain_constraints(membership, edges, geo)
    geom_same, geom_opposite = geometry_constraints(membership, geo)

    # Grow the official ramp directions along the exact chain topology.  The wave
    # front is deterministic (BFS level, then edge id), and where two differently
    # anchored chains claim the same way the junction is a conflict: the audit
    # reports it instead of letting the arrival order decide the label.
    adjacency = defaultdict(list)
    for left, right in chain_same:
        adjacency[left].append(right)
        adjacency[right].append(left)
    for neighbours in adjacency.values():
        neighbours.sort()
    labels = {}
    heap = []
    for edge_id, direction in sorted(anchors.items()):
        labels[edge_id] = direction
        heapq.heappush(heap, (0, edge_id))
    conflicts = []
    seen_conflicts = set()
    while heap:
        level, edge_id = heapq.heappop(heap)
        for other in adjacency.get(edge_id, ()):
            if other in labels:
                if labels[other] != labels[edge_id]:
                    key = tuple(sorted((edge_id, other)))
                    if key not in seen_conflicts:
                        seen_conflicts.add(key)
                        conflicts.append((key[0], key[1], labels[key[0]], labels[key[1]]))
                continue
            labels[other] = labels[edge_id]
            heapq.heappush(heap, (level + 1, other))

    used_opposite = []
    used_same = []
    fold_backs = 0
    for left, right in geom_opposite:
        if labels.get(left) is not None and labels.get(right) is not None:
            if labels[left] != labels[right]:
                fold_backs += 1
            continue
        used_opposite.append((left, right, 1))
    for left, right in geom_same:
        if labels.get(left) is not None and labels.get(right) is not None:
            continue
        used_same.append((left, right, 0))
    labels, skipped = solve(labels, [("geometry-opposite", used_opposite), ("geometry-same", used_same)])

    print(
        f"constraints: anchors={len(anchors)}, chain same={len(chain_same)}, "
        f"geometry opposite={len(geom_opposite)}, geometry same={len(geom_same)}"
    )
    print(f"chain conflicts (two anchored chains claim one way through the topology): {len(conflicts)}")
    for left, right, left_label, right_label in conflicts[:12]:
        node = edges[left]["to"] if edges[left]["to"] in (edges[right]["from"], edges[right]["to"]) else edges[left]["from"]
        print(
            f"  ! {left} ({left_label}, way {way_of(left)}) meets {right} ({right_label}, way {way_of(right)}) "
            f"at {node} {nodes.get(node)}"
        )
    print(
        f"geometry constraints used where the chain is silent: opposite={len(used_opposite)}, same={len(used_same)}; "
        f"skipped against the chain={len(skipped)}"
    )
    print(f"antiparallel pairs inside one chain (lane fold-backs): {fold_backs}")
    touching = [
        (node, incoming, outgoing)
        for node, incoming, outgoing in merges
        if len({labels[eid] for eid in incoming if eid in labels}) > 1
    ]
    print(f"merges that join two differently labelled streams (the carriageways touch there): {len(touching)}")
    for node, incoming, outgoing in touching[:8]:
        detail = ", ".join(
            f"{eid} (way {way_of(eid)}, {labels.get(eid)}, layer {layer_of.get(way_of(eid))})"
            for eid in incoming
        )
        print(f"  ! {node} {nodes.get(node)}: {detail} -> {outgoing} (way {way_of(outgoing)}, {labels.get(outgoing)})")
    for group, left, right, parity, left_label, right_label in skipped[:8]:
        print(f"    skipped {group}: {left} {'==' if parity == 0 else '!='} {right}")
    for node, left, right, cos in sharp:
        print(f"  - sharp join at {node}: {left} -> {right} turns {math.degrees(math.acos(max(-1.0, min(1.0, cos)))):.0f} deg, left unconstrained")
    for node, eid, chosen, dropped in forks:
        if eid is None:
            print(f"  - ambiguous junction at {node}: {len(dropped)} edges left unconstrained {dropped}")
        else:
            print(f"  - fork at {node}: outgoing {eid} continues from {chosen}, branches {dropped} left free")

    sources = Counter()
    for eid in membership:
        if eid in anchors:
            sources["anchored"] += 1
        elif eid in labels:
            sources["constrained"] += 1
        else:
            sources["unclassified"] += 1
    print(f"classified membership edges: {len(labels)}/{len(membership)} ({dict(sources)})")
    conflicted_edges = {eid for left, right, *_ in conflicts for eid in (left, right)}
    print(f"membership edges involved in a chain conflict: {len(conflicted_edges)}")

    # Stacked pairs: antiparallel neighbours whose ways sit on different layers.
    stacked = defaultdict(lambda: {"pairs": 0, "labels": Counter()})
    for eid, other in geom_opposite:
        left, right = way_of(eid), way_of(other)
        if left == right:
            continue
        left_layer, right_layer = layer_of.get(left), layer_of.get(right)
        if left_layer is None or right_layer is None or left_layer == right_layer:
            continue
        key = (min(left, right), max(left, right))
        stacked[key]["pairs"] += 1
        stacked[key]["labels"][(labels.get(eid), labels.get(other))] += 1
    print(f"stacked carriageway pairs (antiparallel within {GEOM_M:.0f} m, different layer):")
    for (left, right), info in sorted(stacked.items(), key=lambda item: -item[1]["pairs"]):
        print(
            f"  way {left} (layer {layer_of[left]}, {_way_label(labels, membership, left)}) <-> "
            f"way {right} (layer {layer_of[right]}, {_way_label(labels, membership, right)}): "
            f"{info['pairs']} edge pairs, labels {dict(info['labels'])}"
        )

    relation = next(
        el for el in osm["elements"] if el["type"] == "relation" and el["id"] == 4256077
    )
    role_of = {
        r["wayId"]: r["role"]
        for r in json.load(open(f"{REPO}/data/osm-relation-repairs.json"))["repairs"]
        if r["kind"] == "setMemberRole"
    }
    relation_ways = [m["ref"] for m in relation["members"] if m["type"] == "way"]

    by_way = defaultdict(list)
    for eid in membership:
        by_way[way_of(eid)].append(eid)
    counts = Counter()
    against_split = []
    against_file = []
    gaps = []
    way_rows = {}
    for way_id in sorted(set(by_way) | set(role_of) | set(relation_ways)):
        eids = by_way.get(way_id, [])
        votes = Counter(labels[e] for e in eids if e in labels)
        split_votes = Counter(membership[e] for e in eids)
        split = split_votes.most_common(1)[0][0] if len(split_votes) == 1 else None
        solved = votes.most_common(1)[0][0] if votes else None
        role = role_of.get(way_id)
        role_direction = None
        if role:
            role_direction = "inner" if role.startswith("Inner") else "outer"
        way_rows[way_id] = {
            "inRelation": way_id in relation_ways,
            "edges": len(eids),
            "split": split,
            "solved": solved,
            "solvedVotes": dict(votes),
            "role": role_direction,
        }
        if not eids:
            counts["not_in_membership"] += 1
        elif not votes:
            counts["unclassified"] += 1
        elif votes.most_common(1)[0][1] != len(eids):
            counts["solved_partial"] += 1
        else:
            counts["solved_full"] += 1
        if solved is not None and split is not None and solved != split:
            against_split.append((way_id, split, solved))
        if solved is not None and role_direction is not None and solved != role_direction:
            against_file.append((way_id, role_direction, solved, split))
        if solved is None:
            continue
        nearest = None
        for e in eids:
            mid = geo[e][0]
            for other, direction in membership.items():
                if direction == solved:
                    continue
                omid = geo[other][0]
                distance = math.hypot(omid[0] - mid[0], omid[1] - mid[1])
                if nearest is None or distance < nearest[0]:
                    nearest = (distance, way_of(other))
        if nearest and nearest[0] > GAP_M:
            gaps.append((way_id, solved, round(nearest[0]), nearest[1]))

    print(f"per member way: {dict(counts)}")
    print("disagreements against the derived split (graph membership labels):")
    for way_id, split, solved in against_split:
        tags = ways.get(way_id, {}).get("tags", {})
        print(
            f"  {way_id:<12} split={split:<6} solved={solved:<6} "
            f"layer={tags.get('layer')} destination={tags.get('destination') or '-'}"
        )
    if not against_split:
        print("  (none)")
    print("disagreements against data/osm-relation-repairs.json roles:")
    for way_id, role, solved, split in against_file:
        print(f"  {way_id:<12} role={role:<6} solved={solved:<6} split={split}")
    if not against_file:
        print("  (none)")
    print(f"members whose opposite carriageway is farther than {GAP_M:.0f} m away:")
    for way_id, solved, distance, other in gaps:
        print(f"  {way_id:<12} labelled {solved:<6} nearest {other} edge at {distance} m")
    if not gaps:
        print("  (none)")

    # Internal consistency: at one station the two carriageways of a ring run in
    # opposite directions, so an antiparallel pair of membership edges that share
    # a label is a contradiction -- one of the two is mislabelled. No external
    # source is needed to see it.
    pairs = defaultdict(int)
    for eid, other in geom_opposite:
        if way_of(eid) == way_of(other):
            continue
        if labels.get(eid) is None or labels.get(eid) != labels.get(other):
            continue
        pairs[tuple(sorted((way_of(eid), way_of(other))))] += 1
    print("antiparallel same-label way pairs (one of the two must be mislabelled):")
    for (left, right), count in sorted(pairs.items(), key=lambda item: -item[1])[:8]:
        print(f"  way {left} <-> way {right}: {count} edge pairs, both labelled {_way_label(labels, membership, left)}")
    print(f"  ... {len(pairs)} way pairs in total, {sum(pairs.values())} edge pairs")

    print("field check list (stacks where 2D geometry is degenerate):")
    for way_id, lat, lon, note in FIELD_CHECK_SITES:
        row = way_rows.get(way_id, {})
        print(f"  way {way_id} ({lat}, {lon}) split={row.get('split')} solved={row.get('solved')}: {note}")

    return {
            "anchorsPlaced": len(anchors),
            "anchorsMissing": [{"rampId": ramp_id, "reason": why} for ramp_id, why in missing],
            "anchorConflicts": [{"rampId": ramp_id, "message": message} for ramp_id, message in conflicts],
            "classifiedEdges": len(labels),
            "membershipEdges": len(membership),
            "skippedConstraints": len(skipped),
            "chainConflicts": [
                {"left": a, "right": b, "leftLabel": la, "rightLabel": lb}
                for a, b, la, lb in conflicts
            ],
            "foldBackPairs": fold_backs,
            "ways": {str(k): v for k, v in sorted(way_rows.items())},
            "againstSplit": [
                {"wayId": w, "split": s, "solved": o} for w, s, o in against_split
            ],
            "againstRoles": [
                {"wayId": w, "role": r, "solved": o, "split": s} for w, r, o, s in against_file
            ],
            "stackedPairs": [
                {
                    "ways": [left, right],
                    "layers": [layer_of[left], layer_of[right]],
                    "edgePairs": info["pairs"],
                    "labels": {f"{a}/{b}": c for (a, b), c in info["labels"].items()},
                }
                for (left, right), info in sorted(stacked.items())
        ],
        "ambiguousJunctions": [
            {"node": node, "edges": dropped, "incoming": chosen}
            for node, chosen, _, dropped in forks
        ],
        "touchingMerges": [
            {
                "node": node,
                "incoming": [
                    {"edge": eid, "wayId": way_of(eid), "label": labels.get(eid), "layer": layer_of.get(way_of(eid))}
                    for eid in incoming
                ],
                "outgoing": {"edge": outgoing, "wayId": way_of(outgoing), "label": labels.get(outgoing)},
            }
            for node, incoming, outgoing in touching
        ],
        "fieldCheckSites": [
            {
                "wayId": way_id,
                "lat": lat,
                "lon": lon,
                "split": way_rows.get(way_id, {}).get("split"),
                "solved": way_rows.get(way_id, {}).get("solved"),
                "note": note,
            }
            for way_id, lat, lon, note in FIELD_CHECK_SITES
        ],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", help="write the machine-readable result here")
    args = parser.parse_args()
    payload = audit()
    if args.json:
        Path(args.json).write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
        print(f"wrote {args.json}")


def _way_label(labels, membership, way_id):
    votes = Counter(labels[e] for e in membership if way_of(e) == way_id and e in labels)
    if not votes:
        return "unclassified"
    winner, count = votes.most_common(1)[0]
    return winner if count == sum(votes.values()) else f"{winner} (partial {count}/{sum(votes.values())})"


if __name__ == "__main__":
    main()
