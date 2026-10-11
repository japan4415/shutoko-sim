#!/usr/bin/env python3
"""Build the upstream submission package for relation 4256077 (C2 中央環状線).

data/osm-relation-repairs.json is the proposed edit; this script turns it into the
three artifacts a mapper needs to file it against OpenStreetMap:

* ``plan.json``            the edits with every resolved id (split node lists, the
                           role each member gets), machine readable and stable;
* ``relation-4256077.osm`` the proposed target state of the relation as OSM XML,
                           for review and diffing (not for upload: the snapshot has
                           no object versions);
* ``SPOT-CHECK.md``        the human checklist: what to verify in the editor, the
                           OSM URLs, the changeset comment and tags, and how to
                           verify the upload afterwards.

The package never touches the network and never uploads.  It also re-derives the
target relation with scripts/apply-osm-relation-repairs.py and fails if the two
disagree, so the review sheet cannot drift from the verified repair file.

    python3 scripts/build-osm-changeset.py --out-dir /tmp/osm-changeset
"""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import json
import sys
import xml.sax.saxutils as saxutils
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
APPLY_SCRIPT = REPO / "scripts" / "apply-osm-relation-repairs.py"
SNAPSHOT = REPO / "fixtures" / "osm" / "shutoko-all.json"
REPAIRS = REPO / "data" / "osm-relation-repairs.json"
RELATION_ID = 4256077
OSM_WAY_URL = "https://www.openstreetmap.org/way/{way_id}"
OSM_NODE_URL = "https://www.openstreetmap.org/node/{node_id}"
OSM_API_RELATION = "https://api.openstreetmap.org/api/0.6/relation/4256077/full.json"


def per_way_audit_rows():
    """Per-way label comparison from the independent audit (split vs chain evidence)."""
    audit_module = load_module(
        REPO / "scripts" / "audit-c2-carriageway-labels.py", "audit_c2_carriageway_labels"
    )
    with contextlib.redirect_stdout(io.StringIO()):
        audit = audit_module.audit()
    return audit["ways"]


def load_module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_json(path):
    with open(path) as handle:
        return json.load(handle)


def build_plan(snapshot, repairs_document):
    """Apply the repairs in memory and describe every edit with resolved ids."""
    elements = snapshot["elements"]
    relation = next(
        element
        for element in elements
        if element.get("type") == "relation" and element.get("id") == RELATION_ID
    )
    ways = {element["id"]: element for element in elements if element.get("type") == "way"}
    before_members = list(relation["members"])
    roles = {}
    removals = []
    splits = []
    for repair in repairs_document["repairs"]:
        kind = repair["kind"]
        if kind == "setMemberRole":
            roles[repair["wayId"]] = repair["role"]
        elif kind == "removeMemberWay":
            removals.append(repair["wayId"])
        elif kind == "splitMemberWayAtNode":
            way = ways[repair["wayId"]]
            nodes = way["nodes"]
            cut = nodes.index(repair["nodeId"])
            splits.append(
                {
                    "wayId": repair["wayId"],
                    "nodeId": repair["nodeId"],
                    "headRef": f"split:{repair['wayId']}:head",
                    "tailRef": f"split:{repair['wayId']}:tail",
                    "headNodes": nodes[: cut + 1],
                    "tailNodes": nodes[cut:],
                }
            )
    split_by_way = {split["wayId"]: split for split in splits}
    # Resolve the member list exactly like scripts/apply-osm-relation-repairs.py:
    # removals disappear and a split way is replaced in place by [head, tail].
    members = []
    for member in before_members:
        if member.get("type") != "way":
            members.append(dict(member))
            continue
        way_id = member["ref"]
        if way_id in removals:
            continue
        split = split_by_way.get(way_id)
        role = roles.get(way_id, member.get("role", ""))
        if split is not None:
            members.append({"type": "way", "ref": split["headRef"], "role": role})
            members.append({"type": "way", "ref": split["tailRef"], "role": role})
            continue
        members.append({"type": "way", "ref": way_id, "role": role})
    return {
        "relationId": RELATION_ID,
        "roles": {str(way_id): role for way_id, role in sorted(roles.items())},
        "removals": sorted(removals),
        "splits": splits,
        "membersBefore": len(before_members),
        "membersAfter": len(members),
        "members": members,
    }


def render_osm(snapshot, plan):
    """Render the proposed target relation as OSM XML (reference, not an upload)."""
    elements = snapshot["elements"]
    relation = next(
        element
        for element in elements
        if element.get("type") == "relation" and element.get("id") == RELATION_ID
    )
    ways = {element["id"]: element for element in elements if element.get("type") == "way"}
    split_by_way = {split["wayId"]: split for split in plan["splits"]}
    lines = [
        "<?xml version='1.0' encoding='UTF-8'?>",
        "<osm version='0.6' generator='shutoko-sim build-osm-changeset.py'>",
        "  <!-- Proposed target state of relation 4256077 (reference for review; the",
        "       snapshot carries no object versions, so this file is not an upload). -->",
        f"  <relation id='{RELATION_ID}' version='0' action='modify'>",
    ]
    for member in relation["members"]:
        if member.get("type") != "way":
            lines.append(
                f"    <member type={saxutils.quoteattr(member['type'])} "
                f"ref='{member['ref']}' role={saxutils.quoteattr(member.get('role', ''))}/>"
            )
            continue
        way_id = member["ref"]
        split = split_by_way.get(way_id)
        if split is not None:
            for suffix, new_id in (("head", -100 - plan["splits"].index(split)), ("tail", -1 - plan["splits"].index(split))):
                lines.append(
                    f"    <member type='way' ref='{new_id}' "
                    f"role={saxutils.quoteattr(plan['roles'].get(str(way_id), ''))}/>  <!-- split {suffix} of way {way_id} -->"
                )
            continue
        if way_id in plan["removals"]:
            lines.append(f"    <!-- dropped member way {way_id} (see plan.json) -->")
            continue
        lines.append(
            f"    <member type='way' ref='{way_id}' "
            f"role={saxutils.quoteattr(plan['roles'].get(str(way_id), member.get('role', '')))}/>"
        )
    for key, value in sorted(relation.get("tags", {}).items()):
        lines.append(f"    <tag k={saxutils.quoteattr(key)} v={saxutils.quoteattr(value)}/>")
    lines.append("  </relation>")
    for split in plan["splits"]:
        way = ways[split["wayId"]]
        for suffix, new_id, nodes in (
            ("head", -100 - plan["splits"].index(split), split["headNodes"]),
            ("tail", -1 - plan["splits"].index(split), split["tailNodes"]),
        ):
            lines.append(
                f"  <!-- split {suffix} of way {split['wayId']} at {split['nodeId']} "
                f"(existing way keeps the {('head' if suffix == 'head' else 'tail')} half upstream) -->"
            )
            lines.append(f"  <way id='{new_id}' version='0' action='modify'>")
            for node_id in nodes:
                lines.append(f"    <nd ref='{node_id}'/>")
            for key, value in sorted(way.get("tags", {}).items()):
                lines.append(f"    <tag k={saxutils.quoteattr(key)} v={saxutils.quoteattr(value)}/>")
            lines.append("  </way>")
    lines.append("</osm>")
    return "\n".join(lines) + "\n"


def render_spot_check(snapshot, plan, repairs_document, audit_rows):
    changeset = repairs_document["changeset"]
    ways = {element["id"]: element for element in snapshot["elements"] if element.get("type") == "way"}
    lines = [
        "# relation 4256077 (首都高速中央環状線, C2) 上流 changeset のスポットチェック",
        "",
        "このファイルは `scripts/build-osm-changeset.py` が `data/osm-relation-repairs.json` から",
        "生成した人手確認用のチェックリストです。ネットワークへは接続せず、アップロードも行いません。",
        "",
        "## changeset",
        "",
        f"- comment: `{changeset['comment']}`",
        f"- source: `{changeset['source']}`",
        "- tags: " + ", ".join(f"`{key}={value}`" for key, value in sorted(changeset["tags"].items())),
        "",
        "## 編集内容",
        "",
        f"- `setMemberRole`: {len(plan['roles'])} member（Inner directions "
        f"{sum(1 for role in plan['roles'].values() if role.startswith('Inner'))} / Outer directions "
        f"{sum(1 for role in plan['roles'].values() if role.startswith('Outer'))}）",
        f"- `removeMemberWay`: {len(plan['removals'])} member "
        + ", ".join(f"[{way_id}]({OSM_WAY_URL.format(way_id=way_id)})" for way_id in plan["removals"]),
        f"- `splitMemberWayAtNode`: {len(plan['splits'])} way "
        + ", ".join(
            f"[{split['wayId']}]({OSM_WAY_URL.format(way_id=split['wayId'])}) at "
            f"[{split['nodeId']}]({OSM_NODE_URL.format(node_id=str(split['nodeId']).removeprefix('n:'))})"
            for split in plan["splits"]
        ),
        f"- relation members: {plan['membersBefore']} -> {plan['membersAfter']}",
        "",
        "## 目視で確認する区間",
        "",
        "| # | 区間 | 見るもの | なぜ |",
        "| --- | --- | --- | --- |",
        "| 1 | 大井JCT（lat 35.608, lon 139.755） | way 331692346 が layer 2、way 250494182 が layer 3 の上下 2 段で、内回りが上（=way 250494182 が inner）かどうか。トンネル区間で 2 次元幾何では分離できない | anchor 監査は上を inner として 331692346 を outer と確定する。ここが逆なら両方の role が入れ替わる |",
        "| 2 | 王子〜江北（lon 139.732–139.745） | way 44209387 / 44211094 / 780870438 が外回りかどうか。飛鳥山トンネルで上下 2 段ではない例外区間 | 旧案はこの 3 way を inner としていた（バグ）。公式ランプ方向と OSM の案内表示は outer を支持する |",
        "| 3 | 大橋JCT（lat 35.6518, lon 139.6878） | way 768600220（role は今回 outer を付与）。ここは一周する link way が折り返す区間で、鎖の証拠は inner と言う | 監査が split を再現できなかった唯一の way。role 無しだと relation 全体が fail-closed になるため付与した |",
        "",
        "## 手順",
        "",
        "1. エディタ（JOSM 推奨）で relation 4256077 と全 member way を取得する。",
        "   `https://api.openstreetmap.org/api/0.6/relation/4256077/full.json` を `out meta;` 相当で",
        "   取得したスナップショットを `scripts/apply-osm-relation-repairs.py` に通すと、version 付きの",
        "   編集後オブジェクトが手に入る。",
        "2. 上記 3 区間を目視で確認する。",
        "3. `plan.json` の `roles` を member に適用し、`removals` を削除し、`splits` の node で 2 way に分割する。",
        "   分割では `headNodes` / `tailNodes` の node 列をそのまま使い、タグは元 way を複製する。",
        "4. changeset の comment / source / tags を上の値に設定してアップロードする。",
        "5. アップロード後に relation 4256077 の履歴と role を読み返し、`plan.json` と一致することを確認する。",
        "   一致しない場合はリバートせず、差分を `humanReviewRequired` に追記して再提案する。",
        "",
        "## アップロード前チェック",
        "",
        "- [ ] relation が role 無しだったことを履歴で確認した（role 付きなら本提案は古い）。",
        "- [ ] `plan.json` の role 数が relation の member 数と整合する（"
        f"{len(plan['roles'])} + 削除 {len(plan['removals'])} + 分割 {len(plan['splits'])}）。",
        "- [ ] 3 区間の目視確認を終えた。",
        "- [ ] 分割で作る way のタグが元 way と一致している。",
        "- [ ] `data/osm-relation-repairs.json` の `humanReviewRequired` を読み、未解決点を changeset の",
        "      discussion に書く用意がある。",
        "",
        "## 参考: member way ごとの role と監査結果",
        "",
        "| way | role | edges | split | audit | layer |",
        "| --- | --- | ---: | --- | --- | --- |",
    ]
    for way_id, role in sorted(plan["roles"].items(), key=lambda item: int(item[0])):
        row = audit_rows.get(way_id, {})
        layer = ways.get(int(way_id), {}).get("tags", {}).get("layer", "")
        lines.append(
            f"| [{way_id}]({OSM_WAY_URL.format(way_id=way_id)}) | {role} | "
            f"{row.get('edges', '')} | {row.get('split', '')} | {row.get('solved', '')} | {layer} |"
        )
    review = repairs_document["humanReviewRequired"]
    lines += ["", "## 提案側が残した未解決点（humanReviewRequired）", ""]
    lines += [f"- {note}" for note in review]
    lines.append("")
    return "\n".join(lines)


def target_members_from_apply(module, snapshot_path, repairs_path, out_path):
    """Run the checked-in apply script and return its relation members."""
    module.main(["--osm", str(snapshot_path), "--repairs", str(repairs_path), "--out", str(out_path)])
    repaired = read_json(out_path)
    relation = next(
        element
        for element in repaired["elements"]
        if element.get("type") == "relation" and element.get("id") == RELATION_ID
    )
    return relation["members"]


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--osm", default=str(SNAPSHOT), help="input Overpass snapshot")
    parser.add_argument("--repairs", default=str(REPAIRS), help="osm-relation-repairs.json")
    parser.add_argument("--out-dir", required=True, help="where the package is written")
    args = parser.parse_args(argv)

    snapshot = read_json(args.osm)
    repairs_document = read_json(args.repairs)
    if repairs_document.get("relationId") != RELATION_ID:
        raise SystemExit(f"{args.repairs}: unexpected relationId")
    audit_rows = per_way_audit_rows()

    plan = build_plan(snapshot, repairs_document)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "plan.json").write_text(json.dumps(plan, ensure_ascii=False, indent=2) + "\n")
    (out_dir / "relation-4256077.osm").write_text(render_osm(snapshot, plan))
    (out_dir / "SPOT-CHECK.md").write_text(render_spot_check(snapshot, plan, repairs_document, audit_rows))
    (out_dir / "changeset-tags.json").write_text(
        json.dumps(repairs_document["changeset"], ensure_ascii=False, indent=2) + "\n"
    )

    # Cross-check the planned member list against the verified apply script.
    apply_module = load_module(APPLY_SCRIPT, "apply_osm_relation_repairs")
    applied_path = out_dir / "applied-snapshot.json"
    applied = target_members_from_apply(apply_module, Path(args.osm), Path(args.repairs), applied_path)
    applied_path.unlink()
    synthetic = {}
    for split in plan["splits"]:
        synthetic[apply_module.split_way_id(split["wayId"], 1)] = split["headRef"]
        synthetic[apply_module.split_way_id(split["wayId"], 2)] = split["tailRef"]
    applied_members = [
        {"ref": synthetic.get(member["ref"], member["ref"]), "role": member.get("role", "")}
        for member in applied
        if member.get("type") == "way"
    ]
    planned_members = [{"ref": member["ref"], "role": member["role"]} for member in plan["members"]]
    if applied_members != planned_members:
        for planned, actual in zip(planned_members, applied_members):
            if planned != actual:
                raise SystemExit(
                    "the planned relation disagrees with scripts/apply-osm-relation-repairs.py "
                    f"at member {planned} vs {actual}"
                )
        raise SystemExit(
            "the planned relation disagrees with scripts/apply-osm-relation-repairs.py: "
            f"{len(planned_members)} planned vs {len(applied_members)} applied members"
        )

    print(
        f"wrote {out_dir}/plan.json, relation-4256077.osm, SPOT-CHECK.md, changeset-tags.json; "
        f"cross-checked {len(planned_members)} member ways and their roles against the apply script"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
