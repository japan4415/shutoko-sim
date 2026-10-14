#!/usr/bin/env python3
"""graph.json のワイヤ形式を tools 向けに正規化する（Issue #43）。

graph schema 5 はノード ID を昇順差分（`nodeIdDeltas`）で持ち、座標は `[lat, lon]`、
edges の `from` / `to` は位置インデックス。reader（routing-core / WASM）と同じ規則で
schema 2〜4 形状（`nodes[].id`、edges の `"n:<osmId>"` 文字列）へ戻す。schema 5 以外の
文書はそのまま返す。

schema 4 以前の形を前提に書かれた offline tools
（`audit-c2-carriageway-labels.py`、`build-inventory-and-bindings.py` など）は、
`load_graph()` を通して読むことで schema 5 でも動く。
"""

from __future__ import annotations

import json
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def normalize_graph_v5_to_v4(graph: dict) -> dict:
    """schema 5 の文書を schema 4 形状へ正規化して返す（schema 5 以外はそのまま）。"""
    if graph.get("schemaVersion") != 5:
        return graph
    deltas = graph["nodeIdDeltas"]
    coordinates = graph["nodes"]
    if not deltas or len(deltas) != len(coordinates):
        raise ValueError("schema 5 requires nodeIdDeltas and nodes to have the same length")
    node_ids: list[str] = []
    previous = 0
    for delta in deltas:
        previous += int(delta)
        if previous <= 0:
            raise ValueError(f"schema 5 node id must stay positive (got {previous})")
        node_ids.append(f"n:{previous}")
    rebuilt = dict(graph)
    rebuilt["schemaVersion"] = 4
    rebuilt["nodes"] = [
        {"id": node_id, "lat": coordinate[0], "lon": coordinate[1]}
        for node_id, coordinate in zip(node_ids, coordinates)
    ]
    rebuilt["edges"] = [
        {
            **{key: value for key, value in edge.items() if key not in ("from", "to")},
            "from": node_ids[edge["from"]],
            "to": node_ids[edge["to"]],
        }
        for edge in graph["edges"]
    ]
    del rebuilt["nodeIdDeltas"]
    return rebuilt


def load_graph(path: str | Path | None = None) -> dict:
    """fixtures/generated/graph.json（または指定パス）を読み、正規化して返す。"""
    target = Path(path) if path is not None else REPO / "fixtures/generated/graph.json"
    with open(target, "r", encoding="utf-8") as handle:
        return normalize_graph_v5_to_v4(json.load(handle))
