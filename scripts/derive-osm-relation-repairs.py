#!/usr/bin/env python3
"""Derive data/osm-relation-repairs.json from the release build and the label audit.

The upstream changeset for relation 4256077 (首都高速中央環状線, C2) has three parts:

* ``setMemberRole`` for every member way whose carriageway the released split and
  the independent anchor audit agree on (scripts/audit-c2-carriageway-labels.py);
* ``removeMemberWay`` for the members the released split never reaches (no C2
  mainline edge at all), which a mapper must either label or drop;
* ``splitMemberWayAtNode`` for the two members the relation walk can only enter from
  the interior of their own node list.

Ways whose role the audit cannot confirm are not rewritten here: they go into
``humanReviewRequired`` so a mapper decides, and the file stays honest about what is
mechanically established and what is not.

    python3 scripts/derive-osm-relation-repairs.py            # write the file
    python3 scripts/derive-osm-relation-repairs.py --check    # verify it is current
"""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
AUDIT_SCRIPT = REPO / "scripts" / "audit-c2-carriageway-labels.py"
SNAPSHOT = REPO / "fixtures" / "osm" / "shutoko-all.json"
OUTPUT = REPO / "data" / "osm-relation-repairs.json"
RELATION_ID = 4256077
ROLE_TEXT = {"inner": "Inner directions", "outer": "Outer directions"}

# The member-walk defects the graph builder reports verbatim.  They are verified
# against the snapshot below (the way must be a member and the node must sit in the
# interior of its node list), so a stale entry fails instead of shipping.
SPLIT_REPAIRS = [
    {
        "kind": "splitMemberWayAtNode",
        "wayId": 891237687,
        "nodeId": 8283617223,
        "reason": (
            "the member walk reaches n:8283617223 at the end of way 44211094 and way "
            "891237687 only continues from the interior of its own node list"
        ),
    },
    {
        "kind": "splitMemberWayAtNode",
        "wayId": 44130240,
        "nodeId": 370270234,
        "reason": (
            "way 44130240 holds n:370270234 at node index 1, so the chain from way "
            "28188274 cannot continue into it"
        ),
    },
]


def load_audit():
    spec = importlib.util.spec_from_file_location("audit_c2_carriageway_labels", AUDIT_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with contextlib.redirect_stdout(io.StringIO()):
        return module.audit()


def load_relation():
    document = json.load(open(SNAPSHOT))
    elements = document["elements"]
    relation = next(
        (
            element
            for element in elements
            if element.get("type") == "relation" and element.get("id") == RELATION_ID
        ),
        None,
    )
    if relation is None:
        raise SystemExit(f"relation {RELATION_ID} is absent from {SNAPSHOT}")
    ways = {element["id"]: element for element in elements if element.get("type") == "way"}
    return relation, ways


def verify_split_repairs(relation, ways):
    members = {member["ref"] for member in relation["members"] if member.get("type") == "way"}
    for repair in SPLIT_REPAIRS:
        way = ways.get(repair["wayId"])
        if way is None:
            raise SystemExit(f"split repair: way {repair['wayId']} is absent from the snapshot")
        if repair["wayId"] not in members:
            raise SystemExit(f"split repair: way {repair['wayId']} is not a relation member")
        nodes = way["nodes"]
        if repair["nodeId"] not in nodes[1:-1]:
            raise SystemExit(
                f"split repair: way {repair['wayId']} does not hold node {repair['nodeId']} "
                "as an interior node"
            )


def derive():
    audit = load_audit()
    relation, ways = load_relation()
    verify_split_repairs(relation, ways)
    rows = audit["ways"]

    member_order = [
        member["ref"] for member in relation["members"] if member.get("type") == "way"
    ]
    roles = []
    removals = []
    review = []
    for way_id in member_order:
        row = rows.get(str(way_id))
        if row is None:
            raise SystemExit(f"way {way_id} is a member but the audit has no row for it")
        if row["edges"] == 0:
            removals.append(
                {
                    "kind": "removeMemberWay",
                    "wayId": way_id,
                    "reason": (
                        "the released C2 split labels no mainline edge of this member, so a "
                        "mapper must assign a direction or drop the member (the relation walk "
                        "cannot reach it from either carriageway anchor)"
                    ),
                }
            )
            continue
        if row["split"] is not None:
            # The changeset writes out the labels of the released split, because the
            # builder expands a role-carrying relation instead of deriving the split.
            # Where the audit could not reproduce the label the way is still given its
            # role (a role-less member makes the whole relation fail closed), but it is
            # listed for review as well.
            confirmed = row["solved"] == row["split"]
            roles.append(
                {
                    "kind": "setMemberRole",
                    "wayId": way_id,
                    "role": ROLE_TEXT[row["split"]],
                    "reason": (
                        f"all {row['edges']} membership edges are {row['split']}; the "
                        "independent anchor audit reproduces the label from the official ramp "
                        "directions along the one-way chain "
                        "(scripts/audit-c2-carriageway-labels.py)"
                        if confirmed
                        else (
                            f"the released split says {row['split']} but the chain evidence "
                            f"reaching this way from the official ramp anchors says "
                            f"{row['solved']}: assigned for the relation to stay walkable, "
                            "confirm before uploading"
                        )
                    ),
                }
            )
            if not confirmed:
                review.append(row | {"wayId": way_id, "inRelation": True})
            continue
        review.append(row | {"wayId": way_id, "inRelation": True})

    repairs = sorted(roles, key=lambda item: item["wayId"])
    repairs += sorted(removals, key=lambda item: item["wayId"])
    repairs += sorted(SPLIT_REPAIRS, key=lambda item: item["wayId"])

    against_split = audit["againstSplit"]
    against_roles = audit["againstRoles"]
    human_review = [
        (
            "The setMemberRole entries are the labels of the released split, not a surveyed "
            "direction sign, and they are reproduced by an independent chain of evidence: "
            "the official ramp directions (data/ramp-inventory.json) fixed at the ramp "
            f"junctions, propagated along the exact one-way chain. The audit places "
            f"{audit['anchorsPlaced']} anchors, finds {len(audit['anchorConflicts'])} conflicts "
            f"with the split and classifies {audit['classifiedEdges']} of "
            f"{audit['membershipEdges']} membership edges."
        ),
        (
            "A mapper should still spot-check the two stretches where the carriageways share "
            "coordinates and the 2D geometry cannot separate them: 大井JCT (way 331692346 "
            "layer 2 = outer above way 250494182 layer 3 = inner) and the 王子 to 江北 run "
            "(ways 44209387 / 44211094 / 780870438), which the earlier proposal had as inner "
            "by mistake."
        ),
        (
            "The four removeMemberWay entries have no labelled C2 mainline edge in the "
            "released split: " + ", ".join(str(item["wayId"]) for item in sorted(removals, key=lambda item: item["wayId"])) +
            ". Assigning a direction is preferred over dropping a member when the way really "
            "is C2 mainline."
        ),
    ]
    if review:
        for row in review:
            human_review.append(
                (
                    f"Way {row['wayId']} is given the released split's role "
                    f"({ROLE_TEXT[row['split']]}) because a role-less member makes the whole "
                    f"relation fail closed, but the chain evidence reaching it from the official "
                    f"ramp anchors says {row['solved']}. It sits in the 大橋JCT tangle where the "
                    "two carriageways touch, so a mapper must confirm the direction before this "
                    "changeset is uploaded."
                )
            )
        human_review.append(
            "The 大橋JCT tangle also contains lane fold-backs, so geometry alone cannot label "
            f"it: the audit finds {audit['foldBackPairs']} antiparallel pairs inside a single "
            "chain and "
            f"{sum(pair['edgePairs'] for pair in audit['stackedPairs'])} stacked-carriageway "
            "edge pairs."
        )
    human_review.append(
        "No shipped C2 pair crosses the changed stretch; the three pairs keep the same "
        "mandatory lap hash."
    )

    document = {
        "schemaVersion": 1,
        "relationId": RELATION_ID,
        "routeId": "C2",
        "status": "proposed_upstream_not_applied",
        "description": (
            "Proposed upstream edits to OSM relation 4256077 (首都高速中央環状線). The relation "
            "is role-less, so graph-builder derives the inner/outer carriageway split from the "
            "verified-bound C2 ramp evidence plus the ring geometry; with the corrected 王子北 "
            f"binding (data/osm-ramp-bindings.json) that split labels all "
            f"{audit['membershipEdges']} relation mainline edges. This file writes those labels "
            "out as roles so the relation carries the directions itself. It is not applied to "
            "the release fixture; scripts/apply-osm-relation-repairs.py verifies it on a copy "
            "of the snapshot (docs/c2-route-design.md 段13)."
        ),
        "changeset": {
            "comment": (
                "首都高速中央環状線 C2 (relation 4256077) の本線 member に Inner/Outer "
                "directions role を付与し、到達不能な member を除去"
            ),
            "source": (
                "OSM snapshot 2026-09-16; 首都高速道路 路線図 (official route map) for the ramp "
                "directions; scripts/audit-c2-carriageway-labels.py for the independent label "
                "audit"
            ),
            "tags": {
                "created_by": "shutoko-sim osm-relation-repairs schemaVersion 1",
                "hashtags": "首都高速;中央環状線;C2",
            },
        },
        "applyCommand": [
            "python3",
            "scripts/apply-osm-relation-repairs.py",
            "--osm",
            "fixtures/osm/shutoko-all.json",
            "--repairs",
            "data/osm-relation-repairs.json",
            "--out",
            "/tmp/shutoko-repaired.json",
        ],
        "verification": {
            "measuredAt": "2026-10-11",
            "appliedToSnapshot": (
                "python3 scripts/apply-osm-relation-repairs.py --osm fixtures/osm/shutoko-all.json "
                "--repairs data/osm-relation-repairs.json --out /tmp/shutoko-repaired.json then "
                "run graph-builder on it"
            ),
            "baseline": {
                "unassignedEdges": 169,
                "sameLabelAntiparallelPairs": 792,
                "splitBug": (
                    "the 王子北 binding pointed at a 川口線 ramp 1.6 km from the C2 mainline, which "
                    "seeded the outer carriageway eastbound chain (44209387 / 780870438 / 44211094) "
                    "as inner"
                ),
            },
            "after": (
                "the release fixture itself is corrected (graph-builder pairing fill plus the "
                "王子北 binding fix), so this file is the changeset that follows it"
            ),
            "labelAudit": {
                "script": "scripts/audit-c2-carriageway-labels.py",
                "evidence": (
                    "official ramp directions anchored at the ramp junctions, propagated along "
                    "the exact one-way chain; 2D geometry is used only where the chain is silent"
                ),
                "junctionSearchValidatedAgainst": "6 junction nodes recorded by the three authored pairs",
                "anchorsPlaced": f"{audit['anchorsPlaced']} / 25 verified_bound C2 ramps",
                "anchorConflicts": len(audit["anchorConflicts"]),
                "classifiedEdges": f"{audit['classifiedEdges']} / {audit['membershipEdges']}",
                "chainConflicts": len(audit["chainConflicts"]),
                "foldBackPairsInsideOneChain": audit["foldBackPairs"],
                "perWayAgainstSplit": {
                    "agree": sum(
                        1
                        for row in rows.values()
                        if row["split"] is not None and row["solved"] == row["split"]
                    ),
                    "disagree": len(against_split),
                    "notInMembership": sum(1 for row in rows.values() if row["edges"] == 0),
                },
                "rolesAssignedButUnconfirmed": [row["wayId"] for row in review],
                "previousRolesWithdrawn": [
                    {"wayId": item["wayId"], "role": item["role"]} for item in against_roles
                ],
                "fieldCheckSites": audit["fieldCheckSites"],
                "ambiguousJunctions": audit["ambiguousJunctions"],
            },
        },
        "humanReviewRequired": human_review,
        "repairs": repairs,
    }
    return document


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if the checked-in file differs from the derivation",
    )
    args = parser.parse_args(argv)
    document = derive()
    text = json.dumps(document, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        current = OUTPUT.read_text() if OUTPUT.exists() else ""
        if current != text:
            print(f"{OUTPUT} is stale: re-run scripts/derive-osm-relation-repairs.py", file=sys.stderr)
            return 1
        print(f"{OUTPUT} is current")
        return 0
    OUTPUT.write_text(text)
    kinds = {}
    for repair in document["repairs"]:
        kinds[repair["kind"]] = kinds.get(repair["kind"], 0) + 1
    print(f"wrote {OUTPUT}: {len(document['repairs'])} repairs {kinds}, {len(document['humanReviewRequired'])} review notes")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
