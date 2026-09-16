#!/usr/bin/env python3
"""
Fetch official Shutoko ramp population from search.shutoko.jp.
Primary source:
  - https://search.shutoko.jp/js/jquery.shutoko.roaddata.js
  - https://search.shutoko.jp/js/jquery.shutoko.icdata.js
  - https://search.shutoko.jp/js/jquery.shutoko.icinoutnodata.js
Output:
  - data/official-population-snapshot.json
"""

import json
import re
import sys
import urllib.request

ROAD_CODE_MAP = {
    "S00": {"route": "C1", "name": "高速都心環状線", "code": "C1"},
    "S31": {"route": "C2", "name": "高速中央環状線", "code": "C2"},
    "S90": {"route": "Y", "name": "高速八重洲線", "code": "Y"},
    "S51": {"route": "B", "name": "高速湾岸線", "code": "B"},
    "S18": {"route": "1U", "name": "高速1号上野線", "code": "1U"},
    "S01": {"route": "1H", "name": "高速1号羽田線", "code": "1H"},
    "S02": {"route": "2", "name": "高速2号目黒線", "code": "2"},
    "S03": {"route": "3", "name": "高速3号渋谷線", "code": "3"},
    "S04": {"route": "4", "name": "高速4号新宿線", "code": "4"},
    "S05": {"route": "5", "name": "高速5号池袋線", "code": "5"},
    "S06": {"route": "6M", "name": "高速6号向島線", "code": "6M"},
    "S16": {"route": "6S", "name": "高速6号三郷線", "code": "6S"},
    "S07": {"route": "7", "name": "高速7号小松川線", "code": "7"},
    "S09": {"route": "9", "name": "高速9号深川線", "code": "9"},
    "S10": {"route": "10", "name": "高速10号晴海線", "code": "10"},
    "S11": {"route": "11", "name": "高速11号台場線", "code": "11"},
    "S21": {"route": "S1", "name": "高速川口線", "code": "S1"},
    "S26": {"route": "S2", "name": "高速埼玉新都心線", "code": "S2"},
    "S25": {"route": "S5", "name": "高速埼玉大宮線", "code": "S5"},
    "S81": {"route": "K1", "name": "高速神奈川1号横羽線", "code": "K1"},
    "S91": {"route": "K2", "name": "高速神奈川2号三ツ沢線", "code": "K2"},
    "S92": {"route": "K3", "name": "高速神奈川3号狩場線", "code": "K3"},
    "S83": {"route": "K5", "name": "高速神奈川5号大黒線", "code": "K5"},
    "S86": {"route": "K6", "name": "高速神奈川6号川崎線", "code": "K6"},
    "S93": {"route": "K7", "name": "高速神奈川7号横浜北線", "code": "K7"},
    "S94": {"route": "K7", "name": "高速神奈川7号横浜北西線", "code": "K7"},
}

DIR_MAP = {
    "内回り": "inner",
    "外回り": "outer",
    "上り": "inbound",
    "下り": "outbound",
    "東行き": "east",
    "西行き": "west",
    "北行き": "north",
    "南行き": "south",
}

def fetch_js(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (ShutokoSim)"})
    with urllib.request.urlopen(req) as res:
        return res.read().decode("utf-8")

def main():
    print("Fetching official Shutoko data...")
    road_js = fetch_js("https://search.shutoko.jp/js/jquery.shutoko.roaddata.js")
    ic_js = fetch_js("https://search.shutoko.jp/js/jquery.shutoko.icdata.js")
    inout_js = fetch_js("https://search.shutoko.jp/js/jquery.shutoko.icinoutnodata.js")

    # Parse icData
    ic_dict = {}
    for m in re.finditer(r'"(\d+)":\s*"([^"]+)"', ic_js):
        key, val = m.groups()
        parts = [p.strip() for p in val.split(",")]
        if len(parts) >= 11:
            road_code = parts[7]
            if road_code in ROAD_CODE_MAP:
                rinfo = ROAD_CODE_MAP[road_code]
                ic_dict[key] = {
                    "key": key,
                    "name": parts[0],
                    "kana": parts[1],
                    "roadCode": road_code,
                    "route": rinfo["route"],
                    "roadName": rinfo["name"],
                    "order": int(parts[10]) if parts[10].isdigit() else 999,
                    "urlPath": parts[12] if len(parts) > 12 else "",
                    "isConnection": "接続" in parts[0],
                }

    # Parse icInOutNoData
    inout_dict = {}
    for m in re.finditer(r'"(\d+)":\s*\{([^}]+)\}', inout_js):
        key, block = m.groups()
        if key in ic_dict:
            pairs = {}
            for pm in re.finditer(r'"([^"]*)":\s*"([^"]*)"', block):
                k, v = pm.groups()
                if k:
                    pairs[k] = v
            inout_dict[key] = pairs

    facilities = []
    general_entries = []
    general_exits = []
    connections = []

    for key, ic in sorted(ic_dict.items(), key=lambda x: (x[1]["roadCode"], x[1]["order"])):
        fac_obj = {
            "facilityKey": key,
            "name": ic["name"],
            "kana": ic["kana"],
            "route": ic["route"],
            "roadCode": ic["roadCode"],
            "roadName": ic["roadName"],
            "url": f"https://www.shutoko.jp/use/network/map/{ic['urlPath']}" if ic["urlPath"] else "https://search.shutoko.jp/",
            "isConnection": ic["isConnection"],
        }
        facilities.append(fac_obj)

        if ic["isConnection"]:
            connections.append(fac_obj)
            continue

        io = inout_dict.get(key, {})
        for inout_no, setting in io.items():
            parts = setting.split(",")
            iotype = parts[0]
            raw_in_dir = parts[1] if len(parts) > 1 else ""
            raw_out_dir = parts[2] if len(parts) > 2 else ""

            if iotype in ["1", "3"] and raw_in_dir and raw_in_dir in DIR_MAP:
                direction = DIR_MAP[raw_in_dir]
                general_entries.append({
                    "facilityKey": key,
                    "facilityName": ic["name"],
                    "route": ic["route"],
                    "direction": direction,
                    "directionJa": raw_in_dir,
                    "inoutNumber": inout_no,
                    "kind": "general_entry",
                    "url": fac_obj["url"],
                })

            if iotype in ["2", "3"] and raw_out_dir and raw_out_dir in DIR_MAP:
                direction = DIR_MAP[raw_out_dir]
                general_exits.append({
                    "facilityKey": key,
                    "facilityName": ic["name"],
                    "route": ic["route"],
                    "direction": direction,
                    "directionJa": raw_out_dir,
                    "inoutNumber": inout_no,
                    "kind": "general_exit",
                    "url": fac_obj["url"],
                })

    snapshot = {
        "version": 1,
        "source": "https://search.shutoko.jp/",
        "sourceDate": "2026-09-16",
        "description": "Official population of Tokyo Metropolitan Expressway ramps captured from search.shutoko.jp official database.",
        "summary": {
            "totalFacilities": len(facilities),
            "generalFacilities": len(facilities) - len(connections),
            "connectionFacilities": len(connections),
            "totalGeneralEntries": len(general_entries),
            "totalGeneralExits": len(general_exits),
            "totalGeneralRamps": len(general_entries) + len(general_exits),
        },
        "facilities": facilities,
        "generalEntries": general_entries,
        "generalExits": general_exits,
        "connectionPoints": connections,
    }

    out_path = "data/official-population-snapshot.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(snapshot, f, ensure_ascii=False, indent=2)

    print(f"Generated {out_path}:")
    print(f"  Facilities: {len(facilities)} ({len(connections)} connections)")
    print(f"  General Entries: {len(general_entries)}")
    print(f"  General Exits:   {len(general_exits)}")
    print(f"  Total General Ramps: {len(general_entries) + len(general_exits)}")

if __name__ == "__main__":
    main()
