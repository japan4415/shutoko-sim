#!/usr/bin/env python3
"""Apply proposed upstream OSM relation repairs to an Overpass snapshot.

Issue #44 records two structural defects of relation 4256077 (C2 中央環状線) that
stop the graph builder from walking the carriageways as continuous runs:

* at 大橋JCT the relation puts a ``motorway_link`` member next to the mainline
  member on the same node, so ``build_relation_member_paths`` stops at the fork;
* member way 891237687 is entered from the interior of its own node list
  (``n:8283617223``), which the member walk cannot express.

``data/osm-relation-repairs.json`` carries the edits that would fix them upstream.
This script applies those edits to a copy of the fetched snapshot so the builder's
own diagnostics verify the fix, without editing the release fixture:

    python3 scripts/apply-osm-relation-repairs.py \\
        --osm fixtures/osm/shutoko-all.json \\
        --repairs data/osm-relation-repairs.json \\
        --out /tmp/shutoko-all-repaired.json

The output is a byte-stable Overpass JSON document: the elements keep the input
order, the split halves of a way keep its tags, and the new way ids are derived
from the source way so repeated runs produce identical ids.
"""

from __future__ import annotations

import argparse
import json
import sys

# Split halves get ids outside the OSM id space of the snapshot so they can never
# collide with a real element id.
SPLIT_WAY_ID_BASE = 9_000_000_000_000


def split_way_id(way_id: int, half: int) -> int:
    return SPLIT_WAY_ID_BASE + way_id * 10 + half


def parse_args(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--osm", required=True, help="input Overpass [out:json] snapshot")
    parser.add_argument("--repairs", required=True, help="osm-relation-repairs.json")
    parser.add_argument("--out", required=True, help="path of the repaired snapshot")
    parser.add_argument(
        "--report",
        action="store_true",
        help="print each applied edit as JSON lines",
    )
    return parser.parse_args(argv)


def load_repairs(path):
    document = json.load(open(path))
    if document.get("schemaVersion") != 1:
        raise SystemExit(f"{path}: unsupported schemaVersion")
    relation_id = document.get("relationId")
    if not isinstance(relation_id, int):
        raise SystemExit(f"{path}: relationId is required")
    repairs = document.get("repairs")
    if not isinstance(repairs, list) or not repairs:
        raise SystemExit(f"{path}: repairs must be a non-empty list")
    return relation_id, repairs


def find_relation(elements, relation_id):
    for element in elements:
        if element.get("type") == "relation" and element.get("id") == relation_id:
            return element
    raise SystemExit(f"relation {relation_id} is absent from the snapshot")


def way_map(elements):
    return {
        element["id"]: element for element in elements if element.get("type") == "way"
    }


def remove_member_way(relation, ways, repair, report):
    way_id = repair["wayId"]
    if way_id not in ways:
        raise SystemExit(f"removeMemberWay: way {way_id} is absent from the snapshot")
    members = relation["members"]
    kept = [member for member in members if member.get("ref") != way_id]
    if len(kept) == len(members):
        raise SystemExit(f"removeMemberWay: relation has no member way {way_id}")
    relation["members"] = kept
    report.append(
        {
            "kind": "removeMemberWay",
            "relationId": relation["id"],
            "wayId": way_id,
            "removedMembers": len(members) - len(kept),
            "reason": repair.get("reason"),
        }
    )


def split_member_way_at_node(elements, relation, ways, repair, report):
    way_id = repair["wayId"]
    node_id = repair["nodeId"]
    way = ways.get(way_id)
    if way is None:
        raise SystemExit(f"splitMemberWayAtNode: way {way_id} is absent from the snapshot")
    nodes = way.get("nodes")
    if not nodes or len(nodes) < 3:
        raise SystemExit(f"splitMemberWayAtNode: way {way_id} has no interior node")
    if node_id not in nodes[1:-1]:
        raise SystemExit(
            f"splitMemberWayAtNode: way {way_id} does not hold node {node_id} "
            "as an interior node"
        )
    cut = nodes.index(node_id)
    head = dict(way)
    head["id"] = split_way_id(way_id, 1)
    head["nodes"] = nodes[: cut + 1]
    tail = dict(way)
    tail["id"] = split_way_id(way_id, 2)
    tail["nodes"] = nodes[cut:]
    index = next(
        (i for i, member in enumerate(relation["members"]) if member.get("ref") == way_id),
        None,
    )
    if index is None:
        raise SystemExit(f"splitMemberWayAtNode: relation has no member way {way_id}")
    # Replace the member in place: the two halves keep the member order, so the
    # relation walk enters the way at the split node.
    relation["members"][index : index + 1] = [
        {"type": "way", "ref": head["id"], "role": relation["members"][index].get("role", "")},
        {"type": "way", "ref": tail["id"], "role": relation["members"][index].get("role", "")},
    ]
    position = elements.index(way)
    elements[position : position + 1] = [head, tail]
    ways[head["id"]] = head
    ways[tail["id"]] = tail
    del ways[way_id]
    report.append(
        {
            "kind": "splitMemberWayAtNode",
            "relationId": relation["id"],
            "wayId": way_id,
            "nodeId": node_id,
            "splitInto": [head["id"], tail["id"]],
            "reason": repair.get("reason"),
        }
    )


def set_member_role(relation, ways, repair, report):
    way_id = repair["wayId"]
    role = repair.get("role")
    if way_id not in ways:
        raise SystemExit(f"setMemberRole: way {way_id} is absent from the snapshot")
    if not isinstance(role, str) or not role:
        raise SystemExit(f"setMemberRole: way {way_id} needs a non-empty role")
    members = [
        member for member in relation["members"] if member.get("ref") == way_id
    ]
    if not members:
        raise SystemExit(f"setMemberRole: relation has no member way {way_id}")
    for member in members:
        member["role"] = role
    report.append(
        {
            "kind": "setMemberRole",
            "relationId": relation["id"],
            "wayId": way_id,
            "role": role,
            "reason": repair.get("reason"),
        }
    )


def main(argv):
    args = parse_args(argv)
    relation_id, repairs = load_repairs(args.repairs)
    document = json.load(open(args.osm))
    elements = document["elements"]
    relation = find_relation(elements, relation_id)
    ways = way_map(elements)
    report = []
    # Splitting mutates `elements` in place; removals and role assignments only
    # touch the relation, so the repair file may list them in any order.
    for repair in repairs:
        kind = repair.get("kind")
        if kind == "removeMemberWay":
            remove_member_way(relation, ways, repair, report)
        elif kind == "splitMemberWayAtNode":
            split_member_way_at_node(elements, relation, ways, repair, report)
        elif kind == "setMemberRole":
            set_member_role(relation, ways, repair, report)
        else:
            raise SystemExit(f"unsupported repair kind {kind!r}")
    document["elements"] = elements
    with open(args.out, "w") as handle:
        json.dump(document, handle, ensure_ascii=False, separators=(",", ":"))
        handle.write("\n")
    print(
        f"applied {len(report)} repair(s) to relation {relation_id}: "
        f"{args.osm} -> {args.out}"
    )
    print(f"  relation members: {len(relation['members'])}")
    if args.report:
        for item in report:
            print(json.dumps(item, ensure_ascii=False))


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
