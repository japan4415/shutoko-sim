#!/usr/bin/env python3
"""
scripts/build-inventory-and-bindings.py

Builds canonical ramp inventory (data/ramp-inventory.json) and
OSM ramp bindings (data/osm-ramp-bindings.json) from:
  1. data/official-population-snapshot.json (official Shutoko population)
  2. data/ramp-support-decisions.json (reviewed facility-level decisions)
  3. fixtures/osm/shutoko-all.json (full-network OSM fixture)
  4. fixtures/generated/graph.json (directed entry/exit validation)

Requirements:
  - Every active general entry/exit is either verified-bound or explicitly unsupported.
  - No nearest-edge/proximity fallback. New unclassified official records stop the build.
  - Zero consecutive/placeholder IDs (no osmNodeId = osmWayId + 1).
  - Every osmNodeId must be in osm_way_id.nodes.
  - Every motorwayNodeId must exist in the OSM graph.
  - Distinct route keys (1U vs 1H, 6M vs 6S, etc.).
  - Preserves closed ramps (Gofukubashi, Edobashi) with status="closed".
"""

import hashlib
import json
import re

FACILITY_SLUG_MAP = {
    "神田橋": "kandabashi", "宝町": "takaracho", "京橋": "kyobashi",
    "新富町": "shintomicho", "銀座": "ginza", "汐留": "shiodome",
    "芝公園": "shibakoen", "飯倉": "iikura", "霞が関": "kasumigaseki",
    "代官町": "daikancho", "北の丸": "kitanomaru",
    "芝浦": "shibaura", "勝島": "katsushima", "鈴ヶ森": "suzugamori", "鈴ケ森": "suzugamori",
    "平和島": "heiwajima", "空港西": "kukonishi", "羽田": "haneda",
    "一ノ橋": "ichinohashi", "目黒": "meguro", "戸越": "togoshi", "荏原": "ebara",
    "渋谷": "shibuya", "高樹町": "takagicho", "用賀": "yoga",
    "外苑": "gaien", "新宿": "shinjuku", "初台": "hatsudai", "幡ヶ谷": "hatagaya", "永福": "eifuku", "高井戸": "takaido",
    "飯田橋": "iidabashi", "早稲田": "waseda", "護国寺": "gokokuji", "東池袋": "higashi-ikebukuro",
    "北池袋": "kita-ikebukuro", "板橋本町": "itabashi-honcho", "中台": "nakadai",
    "志村": "shimura", "戸田南": "toda-minami", "戸田": "toda",
    "箱崎": "hakozaki", "浜町": "hamacho", "清洲橋": "kiyosubashi", "福住": "fukuzumi",
    "駒形": "komagata", "向島": "mukojima", "堤通": "tsutsumidori",
    "加平": "kahei", "八潮南": "yashio-minami", "八潮": "yashio", "三郷": "misato",
    "錦糸町": "kinshicho", "亀戸": "kameido", "小松川": "komatsugawa", "一之江": "ichinoe",
    "木場": "kiba", "塩浜": "shiohama", "枝川": "edagawa",
    "豊洲": "toyosu", "晴海": "harumi", "台場": "daiba",
    "五反田": "gotanda", "中環大井南": "chukan-oiminami", "富ヶ谷": "tomigaya",
    "初台南": "hatsudai-minami", "中野長者橋": "nakano-chojabashi", "西池袋": "nishi-ikebukuro",
    "高松": "takamatsu", "新板橋": "shin-itabashi", "滝野川": "takinogawa",
    "王子南": "oji-minami", "王子北": "oji-kita", "扇大橋": "ogi-ohashi",
    "千住新橋": "senju-shinbashi", "小菅": "kosuge", "四つ木": "yotsugi",
    "平井大橋": "hirai-ohashi", "中環小松川": "chukan-komatsugawa", "船堀橋": "funaboribashi", "清新町": "seishincho",
    "安行": "angyo", "新郷": "shingo", "足立入谷": "adachi-iriya", "鹿浜橋": "shikahamabashi", "東領家": "higashi-ryoke",
    "新都心": "shintoshin", "新都心西": "shintoshin-nishi", "さいたま見沼": "saitama-minuma",
    "浦和南": "urawa-minami", "浦和北": "urawa-kita", "与野": "yono",
    "大井": "oi", "大井南": "oiminami", "臨海副都心": "rinkai-fukutoshin", "有明": "ariake",
    "東雲": "shinonome", "新木場": "shinkiba", "葛西": "kasai", "舞浜": "maihama",
    "浦安": "urayasu", "千鳥町": "chidoricho", "空港中央": "kuko-chuo", "湾岸環八": "wangan-kampachi",
    "浮島": "ukishima", "湾岸川崎": "wangan-kawasaki", "東扇島": "higashi-ogishima", "大黒ふ頭": "daikoku-futo",
    "本牧ふ頭": "hommoku-futo", "三溪園": "sankeien", "三渓園": "sankeien", "磯子": "isogo", "杉田": "sugita", "幸浦": "sachiura",
    "大師": "daishi", "浅田": "asada", "浜川崎": "hamakawasaki", "汐入": "shioiri",
    "生麦": "namamugi", "守屋町": "moriyacho", "子安": "koyasu", "東神奈川": "higashi-kanagawa",
    "横浜駅東口": "yokohama-eki-higashiguchi", "みなとみらい": "minato-mirai", "横浜公園": "yokohama-koen",
    "山下町": "yamashitacho", "新山下": "shin-yamashita", "石川町": "ishikawacho",
    "横浜駅西口": "yokohama-eki-nishiguchi", "三ツ沢": "mitsuzawa",
    "永田": "nagata", "花之木": "hananoki", "井土ヶ谷": "idogaya", "狩場": "kariba",
    "殿町": "tonomachi",
    "岸谷生麦": "kishiya-namamugi", "馬場": "baba", "新横浜": "shin-yokohama",
    "横浜港北": "yokohama-kohoku", "横浜青葉": "yokohama-aoba",
    "本町": "honcho", "上野": "ueno", "入谷": "iriya",
    "八重洲": "yaesu", "丸の内": "marunouchi",
    "呉服橋": "gofukubashi", "江戸橋": "edobashi",
    "天現寺": "tengenji", "池尻": "ikejiri", "三軒茶屋": "sangenjaya",
    "西神田": "nishikanda", "高島平": "takashimadaira", "一ツ橋": "hitotsubashi",
    "新井宿": "araijuku", "南本牧ふ頭": "minamihommoku-futo", "阪東橋": "bandobashi",
    "東名高速接続用賀": "tomei-yoga", "中央道接続高井戸": "chuo-takaido", "外環接続美女木": "gaikan-bijogi",
    "常磐道接続三郷": "joban-misato", "京葉道路接続篠崎": "keiyo-shinozaki", "アクアライン接続浮島": "aqualine-ukishima",
    "横横道路接続並木": "yokoyoko-namiki", "第三京浜接続三ツ沢": "daisan-mitsuzawa", "横横保土ヶ谷接続狩場": "yokoyoko-kariba",
    "第三京浜接続横浜港北": "daisan-kohoku", "東名高速接続横浜青葉": "tomei-aoba", "東北道接続川口": "tohoku-kawaguchi",
}

def get_facility_slug(name: str) -> str:
    if name in FACILITY_SLUG_MAP:
        return FACILITY_SLUG_MAP[name]
    # Fallback to ascii cleanup
    clean = re.sub(r'[^a-zA-Z0-9]+', '-', name).strip('-').lower()
    return clean or "ramp"


def compact_json_sha256(value) -> str:
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def edge_way_id(edge_id: str) -> int:
    parts = edge_id.split(":")
    if len(parts) < 3 or parts[0] != "e" or not parts[1].startswith("w"):
        raise SystemExit(f"invalid graph edge ID in binding candidate: {edge_id}")
    return int(parts[1][1:])


def validate_binding_candidates(support_file, decisions, osm_ways, graph):
    candidates = support_file.get("bindingCandidates", [])
    if not isinstance(candidates, list):
        raise SystemExit("bindingCandidates in ramp-support-decisions.json must be an array")

    references = {}
    for decision in decisions:
        evidence = decision.get("bindingCandidateEvidence")
        if evidence is None:
            continue
        inverse = evidence.get("rampIdInverseMap", {})
        candidate_id = inverse.get("candidateId")
        ramp_id = inverse.get("rampId")
        if not candidate_id or ramp_id != decision["rampId"]:
            raise SystemExit(f"invalid bindingCandidateEvidence inverse map for {ramp_id}")
        if candidate_id in references:
            raise SystemExit(f"duplicate binding candidate reference: {candidate_id}")
        references[candidate_id] = (decision, evidence)

    edge_by_id = {edge["id"]: edge for edge in graph["edges"]}
    seen_candidate_ids = set()
    seen_ramp_ids = set()
    seen_segment_ids = set()
    for candidate in candidates:
        candidate_id = candidate.get("candidateId")
        ramp_id = candidate.get("rampId")
        status = candidate.get("status")
        if not candidate_id or candidate_id in seen_candidate_ids:
            raise SystemExit(f"invalid or duplicate binding candidate ID: {candidate_id}")
        if not ramp_id or ramp_id in seen_ramp_ids:
            raise SystemExit(f"invalid or duplicate binding candidate rampId: {ramp_id}")
        if status not in ("verified_bound", "unresolved", "unsupported"):
            raise SystemExit(f"invalid binding candidate status for {candidate_id}: {status}")
        if candidate_id not in references:
            raise SystemExit(f"binding candidate {candidate_id} has no support-decision reference")
        decision, evidence = references[candidate_id]
        if evidence.get("rampIdInverseMap", {}).get("rampId") != ramp_id:
            raise SystemExit(f"binding candidate {candidate_id} has a conflicting rampId")
        if candidate.get("publicProjection") != evidence.get("publicProjection"):
            raise SystemExit(f"binding candidate {candidate_id} has conflicting publicProjection")
        if status != evidence.get("status"):
            raise SystemExit(f"binding candidate {candidate_id} has conflicting status")
        expected_support_state = "verified_bound" if status == "verified_bound" else "unsupported"
        if decision.get("supportState") != expected_support_state:
            raise SystemExit(f"binding candidate {candidate_id} has conflicting supportState")
        if decision.get("binding") is not None:
            raise SystemExit(f"binding candidate {candidate_id} must not also have a schema binding")
        expected_projection = "included_verified" if status == "verified_bound" else "excluded_unresolved"
        if candidate.get("publicProjection") != expected_projection:
            raise SystemExit(f"binding candidate {candidate_id} has invalid publicProjection")
        if status == "verified_bound":
            if candidate.get("unresolvedReason") or candidate.get("unresolvedReasonCodes"):
                raise SystemExit(f"verified binding candidate {candidate_id} retains unresolved evidence")
        elif not candidate.get("unresolvedReason") or not candidate.get("unresolvedReasonCodes"):
            raise SystemExit(f"unresolved binding candidate {candidate_id} lacks reason codes")
        if not candidate.get("supportEvidence"):
            raise SystemExit(f"binding candidate {candidate_id} lacks supportEvidence")

        segments = candidate.get("directedSegments", [])
        if len(segments) != 1:
            raise SystemExit(f"binding candidate {candidate_id} must have exactly one directed segment")
        segment = segments[0]
        segment_id = segment.get("segmentId")
        if not segment_id or segment_id in seen_segment_ids:
            raise SystemExit(f"invalid or duplicate binding candidate segmentId: {segment_id}")
        edge_ids = segment.get("edgeIds", [])
        osm_node_ids = segment.get("osmNodeIds", [])
        osm_way_ids = segment.get("osmWayIds", [])
        if not edge_ids or len(osm_node_ids) != len(edge_ids) + 1 or len(osm_way_ids) < 2:
            raise SystemExit(f"binding candidate {candidate_id} has an invalid directed segment shape")
        graph_edges = []
        for edge_id in edge_ids:
            edge = edge_by_id.get(edge_id)
            if edge is None:
                raise SystemExit(f"binding candidate {candidate_id} references missing edge {edge_id}")
            graph_edges.append(edge)
        if graph_edges[0]["from"] != segment.get("fromNodeId") or graph_edges[-1]["to"] != segment.get("toNodeId"):
            raise SystemExit(f"binding candidate {candidate_id} has non-contiguous endpoint evidence")
        if any(left["to"] != right["from"] for left, right in zip(graph_edges, graph_edges[1:])):
            raise SystemExit(f"binding candidate {candidate_id} has a discontinuous edge path")
        graph_nodes = [graph_edges[0]["from"]] + [edge["to"] for edge in graph_edges]
        if graph_nodes != [f"n:{node_id}" for node_id in osm_node_ids]:
            raise SystemExit(f"binding candidate {candidate_id} OSM node order does not match graph edges")
        actual_way_ids = []
        for edge_id in edge_ids:
            way_id = edge_way_id(edge_id)
            if not actual_way_ids or actual_way_ids[-1] != way_id:
                actual_way_ids.append(way_id)
        if actual_way_ids != osm_way_ids:
            raise SystemExit(f"binding candidate {candidate_id} OSM way order does not match edge IDs")
        if any(way_id not in osm_ways for way_id in osm_way_ids):
            raise SystemExit(f"binding candidate {candidate_id} references an unknown OSM way")
        if segment.get("edgeIdsSha256") != compact_json_sha256(edge_ids):
            raise SystemExit(f"binding candidate {candidate_id} has invalid edgeIdsSha256")
        kind = evidence.get("kind")
        if kind == "general_entry":
            expected_mainline = segment.get("toNodeId")
            expected_ground = segment.get("fromNodeId")
        elif kind == "general_exit":
            expected_mainline = segment.get("fromNodeId")
            expected_ground = segment.get("toNodeId")
        else:
            raise SystemExit(f"binding candidate {candidate_id} has unsupported evidence kind {kind}")
        if evidence.get("mainlineNodeId") != expected_mainline or evidence.get("declaredGroundNodeId") != expected_ground:
            raise SystemExit(f"binding candidate {candidate_id} has conflicting endpoint evidence")

        route_evidence = candidate.get("routeEvidence", {})
        if not route_evidence.get("routeId") or route_evidence.get("direction") != candidate.get("direction"):
            raise SystemExit(f"binding candidate {candidate_id} has invalid route evidence")
        if not route_evidence.get("relationId") or not route_evidence.get("groundWayId"):
            raise SystemExit(f"binding candidate {candidate_id} has incomplete route evidence")
        if not route_evidence.get("firstExitAfterBranch"):
            raise SystemExit(f"binding candidate {candidate_id} lacks first-exit relation evidence")

        seen_candidate_ids.add(candidate_id)
        seen_ramp_ids.add(ramp_id)
        seen_segment_ids.add(segment_id)

    extra_references = set(references) - seen_candidate_ids
    if extra_references:
        raise SystemExit(f"support decisions reference missing binding candidates: {sorted(extra_references)}")
    return candidates


def main():
    with open("data/official-population-snapshot.json", "r", encoding="utf-8") as f:
        snap = json.load(f)

    with open("fixtures/osm/shutoko-all.json", "r", encoding="utf-8") as f:
        osm = json.load(f)

    with open("data/ramp-support-decisions.json", "r", encoding="utf-8") as f:
        support_file = json.load(f)

    osm_ways = {e["id"]: e for e in osm.get("elements", []) if e["type"] == "way"}
    osm_nodes = {e["id"]: e for e in osm.get("elements", []) if e["type"] == "node"}
    with open("fixtures/generated/graph.json", "r", encoding="utf-8") as f:
        g = json.load(f)

    directed_edges = {}
    for e in g["edges"]:
        parts = e["id"].split(":")
        if len(parts) >= 2 and parts[1].startswith("w"):
            directed_edges[(int(parts[1][1:]), e["from"], e["to"])] = e

    decisions = {d["rampId"]: d for d in support_file["decisions"]}
    if len(decisions) != len(support_file["decisions"]):
        raise SystemExit("duplicate rampId in ramp-support-decisions.json")

    binding_candidates = validate_binding_candidates(
        support_file,
        support_file["decisions"],
        osm_ways,
        g,
    )
    candidate_ramp_ids = {candidate["rampId"] for candidate in binding_candidates}

    all_official = snap["generalEntries"] + snap["generalExits"]
    print(f"Classifying {len(all_official)} official ramps from reviewed decisions...")
    seen_facility_route_dir_kind = set()
    canonical_ramps = []
    bindings = []

    for item in all_official:
        fac_name = item["facilityName"]
        route = item["route"]
        direction = item["direction"]
        kind = item["kind"]
        inout_no = item["inoutNumber"]
        fac_url = item["url"]

        slug = get_facility_slug(fac_name)
        ramp_id = f"ramp:{route.lower()}-{direction}:{slug}-{kind.replace('general_', '')}"
        facility_id = f"fac:{route.lower()}:{slug}"

        # Resolve duplicate (facility_id, route, direction, kind) by appending inout_no
        key = (facility_id, route, direction, kind)
        if key in seen_facility_route_dir_kind or ramp_id in [r["rampId"] for r in canonical_ramps]:
            facility_id = f"fac:{route.lower()}:{slug}-{inout_no.lower()}"
            ramp_id = f"ramp:{route.lower()}-{direction}:{slug}-{inout_no.lower()}-{kind.replace('general_', '')}"
        seen_facility_route_dir_kind.add((facility_id, route, direction, kind))

        decision = decisions.get(ramp_id)
        if decision is None:
            raise SystemExit(f"unclassified official ramp: {ramp_id}")
        support_state = decision["supportState"]
        if support_state not in ("verified_bound", "unsupported"):
            raise SystemExit(f"invalid supportState for {ramp_id}: {support_state}")

        canonical_ramps.append({
            "rampId": ramp_id,
            "facilityId": facility_id,
            "facilityName": fac_name,
            "route": route,
            "direction": direction,
            "kind": kind,
            "lat": decision["lat"],
            "lon": decision["lon"],
            "restrictions": ["etc_only"] if "etc" in fac_url.lower() else [],
            "restrictionStatus": "verified" if "etc" in fac_url.lower() else "unverified",
            "status": "active",
            "source": fac_url,
            "sourceDate": "2026-09-16",
            "coordinateSource": decision["coordinateSource"],
            "coordinateStatus": decision["coordinateStatus"],
            "supportState": support_state,
            "supportReason": decision["supportReason"],
            "supportEvidence": decision["supportEvidence"],
            "routingCapability": decision["routingCapability"],
            "routingCapabilityReason": decision["routingCapabilityReason"]
        })

        binding = decision.get("binding")
        if support_state == "unsupported":
            if binding is not None:
                raise SystemExit(f"unsupported ramp unexpectedly has binding: {ramp_id}")
            continue
        if binding is None and ramp_id in candidate_ramp_ids:
            continue
        if binding is None:
            raise SystemExit(f"verified ramp lacks binding: {ramp_id}")
        if ramp_id in candidate_ramp_ids:
            raise SystemExit(f"candidate-backed ramp unexpectedly has schema binding: {ramp_id}")
        if binding["direction"] != direction:
            raise SystemExit(f"binding direction mismatch for {ramp_id}")
        way = osm_ways.get(binding["osmWayId"])
        if way is None or binding["osmNodeId"] not in way.get("nodes", []):
            raise SystemExit(f"binding way/node does not exist for {ramp_id}")
        if binding["motorwayNodeId"] not in osm_nodes:
            raise SystemExit(f"binding motorway node does not exist for {ramp_id}")
        if kind == "general_entry":
            edge_key = (binding["osmWayId"], f'n:{binding["osmNodeId"]}', f'n:{binding["motorwayNodeId"]}')
            expected_kind = "entry"
        else:
            edge_key = (binding["osmWayId"], f'n:{binding["motorwayNodeId"]}', f'n:{binding["osmNodeId"]}')
            expected_kind = "exit"
        edge = directed_edges.get(edge_key)
        if edge is None or edge.get("kind") != expected_kind:
            raise SystemExit(f"binding is not an exact directed {expected_kind} segment: {ramp_id}")
        bindings.append(binding)

    extra_decisions = sorted(set(decisions) - {r["rampId"] for r in canonical_ramps})
    if extra_decisions:
        raise SystemExit(f"support decisions contain non-official ramp IDs: {extra_decisions}")

    # Add 24 Boundary JCTs
    # Distinct boundary connections:
    # 3 (Yoga), 4 (Takaido), 5 (Bijogi), 6S (Misato), 7 (Shinozaki),
    # B (Ukishima, Namiki), K2 (Mitsuzawa), K3 (Kariba), K7 (Kohoku, Aoba), S1 (Kawaguchi)
    BOUNDARY_DEFS = [
        ("boundary:3-inbound:tomei-jct-in", "東名高速接続用賀", "3", "inbound", "boundary_in", 35.6264, 139.6267, 1535564384, 13990199112, 13990199113),
        ("boundary:3-outbound:tomei-jct-out", "東名高速接続用賀", "3", "outbound", "boundary_out", 35.6264, 139.6267, 1535564384, 13990199113, 13990199112),
        ("boundary:4-inbound:chuo-jct-in", "中央道接続高井戸", "4", "inbound", "boundary_in", 35.6745, 139.6105, 1535567270, 13990210275, 13990210276),
        ("boundary:4-outbound:chuo-jct-out", "中央道接続高井戸", "4", "outbound", "boundary_out", 35.6745, 139.6105, 1535567270, 13990210276, 13990210275),
        ("boundary:5-inbound:gaikan-bijogi-in", "外環接続美女木", "5", "inbound", "boundary_in", 35.8235, 139.6465, 438372559, 4361189063, 4361189064),
        ("boundary:5-outbound:gaikan-bijogi-out", "外環接続美女木", "5", "outbound", "boundary_out", 35.8235, 139.6465, 438372559, 4361189064, 4361189063),
        ("boundary:6s-inbound:joban-jct-in", "常磐道接続三郷", "6S", "inbound", "boundary_in", 35.8346, 139.8578, 1199150818, 11116631897, 11116631898),
        ("boundary:6s-outbound:joban-jct-out", "常磐道接続三郷", "6S", "outbound", "boundary_out", 35.8346, 139.8578, 1199150818, 11116631898, 11116631897),
        ("boundary:7-inbound:keiyo-jct-in", "京葉道路接続篠崎", "7", "inbound", "boundary_in", 35.7001, 139.8972, 156796455, 5660587045, 5660587046),
        ("boundary:7-outbound:keiyo-jct-out", "京葉道路接続篠崎", "7", "outbound", "boundary_out", 35.7001, 139.8972, 156796455, 5660587046, 5660587045),
        ("boundary:b-east:aqualine-jct-in", "アクアライン接続浮島", "B", "east", "boundary_in", 35.5342, 139.7821, 82596770, 1112499632, 1112499633),
        ("boundary:b-west:aqualine-jct-out", "アクアライン接続浮島", "B", "west", "boundary_out", 35.5342, 139.7821, 82596770, 1112499633, 1112499632),
        ("boundary:b-east:yokoyoko-jct-in", "横横道路接続並木", "B", "east", "boundary_in", 35.3621, 139.6432, 347256154, 443726406, 443726407),
        ("boundary:b-west:yokoyoko-jct-out", "横横道路接続並木", "B", "west", "boundary_out", 35.3621, 139.6432, 347256154, 443726407, 443726406),
        ("boundary:k2-inbound:daisan-jct-in", "第三京浜接続三ツ沢", "K2", "inbound", "boundary_in", 35.4741, 139.6002, 44823948, 568327079, 568327080),
        ("boundary:k2-outbound:daisan-jct-out", "第三京浜接続三ツ沢", "K2", "outbound", "boundary_out", 35.4741, 139.6002, 44823948, 568327080, 568327079),
        ("boundary:k3-inbound:yokoyoko-kariba-jct-in", "横横保土ヶ谷接続狩場", "K3", "inbound", "boundary_in", 35.4412, 139.5781, 31709129, 447798983, 447798984),
        ("boundary:k3-outbound:yokoyoko-kariba-jct-out", "横横保土ヶ谷接続狩場", "K3", "outbound", "boundary_out", 35.4412, 139.5781, 31709129, 447798984, 447798983),
        ("boundary:k7-inbound:daisan-kohoku-jct-in", "第三京浜接続横浜港北", "K7", "inbound", "boundary_in", 35.5162, 139.5962, 481160986, 4727188953, 4727188954),
        ("boundary:k7-outbound:daisan-kohoku-jct-out", "第三京浜接続横浜港北", "K7", "outbound", "boundary_out", 35.5162, 139.5962, 481160986, 4727188954, 4727188953),
        ("boundary:k7-inbound:tomei-aoba-jct-in", "東名高速接続横浜青葉", "K7", "inbound", "boundary_in", 35.5532, 139.5312, 1395143774, 13697646231, 13697646232),
        ("boundary:k7-outbound:tomei-aoba-jct-out", "東名高速接続横浜青葉", "K7", "outbound", "boundary_out", 35.5532, 139.5312, 1395143774, 13697646232, 13697646231),
        ("boundary:s1-inbound:tohoku-jct-in", "東北道接続川口", "S1", "inbound", "boundary_in", 35.8527, 139.7340, 44351425, 13990246761, 5432829255),
        ("boundary:s1-outbound:tohoku-jct-out", "東北道接続川口", "S1", "outbound", "boundary_out", 35.8527, 139.7340, 438367202, 4361138358, 13990246762),
    ]

    for bdef in BOUNDARY_DEFS:
        rid, fname, rt, dir_, kind, lat, lon, _wid, _nid, _mid = bdef
        canonical_ramps.append({
            "rampId": rid,
            "facilityId": f"boundary:{rt.lower()}:{get_facility_slug(fname)}",
            "facilityName": fname,
            "route": rt,
            "direction": dir_,
            "kind": kind,
            "lat": lat,
            "lon": lon,
            "restrictions": [],
            "status": "active",
            "source": "https://search.shutoko.jp/",
            "sourceDate": "2026-09-16",
            "coordinateSource": "https://www.openstreetmap.org/",
            "coordinateStatus": "derived",
            "supportState": "not_routable",
            "supportReason": "高速道路境界JCTであり一般選択ランプではない。",
            "supportEvidence": ["data/official-population-snapshot.json"],
            "routingCapability": "not_routable",
            "routingCapabilityReason": "境界JCTは一般選択端点ではない。"
        })

    # Add closed ramps (historical Gofukubashi & Edobashi)
    CLOSED_RAMPS = [
        ("ramp:c1-inner:gofukubashi-entry", "呉服橋", "C1", "inner", "general_entry", 35.6845, 139.7712),
        ("ramp:c1-outer:gofukubashi-exit", "呉服橋", "C1", "outer", "general_exit", 35.6848, 139.7715),
        ("ramp:c1-inner:edobashi-entry", "江戸橋", "C1", "inner", "general_entry", 35.6852, 139.7760),
        ("ramp:c1-outer:edobashi-exit", "江戸橋", "C1", "outer", "general_exit", 35.6855, 139.7763),
    ]
    for rid, fname, rt, dir_, kind, lat, lon in CLOSED_RAMPS:
        canonical_ramps.append({
            "rampId": rid,
            "facilityId": f"fac:c1:{get_facility_slug(fname)}",
            "facilityName": fname,
            "route": rt,
            "direction": dir_,
            "kind": kind,
            "lat": lat,
            "lon": lon,
            "restrictions": ["closed"],
            "status": "closed",
            "source": "https://www.shutoko.jp/use/network/",
            "sourceDate": "2026-09-16",
            "coordinateSource": "https://www.openstreetmap.org/",
            "coordinateStatus": "derived",
            "supportState": "not_routable",
            "supportReason": "閉鎖済み施設であり一般選択ランプではない。",
            "supportEvidence": ["https://www.shutoko.jp/use/network/"],
            "routingCapability": "not_routable",
            "routingCapabilityReason": "閉鎖済み施設は探索端点として使用しない。"
        })
        # Closed historical ramps intentionally have no active graph binding.
        # Reusing an unrelated live C1 edge would make them selectable and would
        # fabricate an OSM association.

    # Output inventory file
    inv_file = {
        "version": 3,
        "source": "https://search.shutoko.jp/",
        "sourceDate": "2026-09-16",
        "coordinateSource": "https://www.openstreetmap.org/",
        "description": "首都高速道路全路線（東京・神奈川・埼玉）の公式母集団準拠正本インベントリ。公式検索システム一次情報に基づき全出入口の方向・路線・種別を完全整理。",
        "ramps": canonical_ramps
    }
    with open("data/ramp-inventory.json", "w", encoding="utf-8") as f:
        json.dump(inv_file, f, ensure_ascii=False, indent=2)

    # Output bindings file
    bindings_file = {
        "version": 4,
        "sourceDate": support_file["sourceDate"],
        "bindings": bindings,
        "bindingCandidates": binding_candidates,
        "sharedPhysicalOverrides": support_file["sharedPhysicalOverrides"]
    }
    with open("data/osm-ramp-bindings.json", "w", encoding="utf-8") as f:
        json.dump(bindings_file, f, ensure_ascii=False, indent=2)

    print(f"Generated data/ramp-inventory.json: {len(canonical_ramps)} ramps")
    active_general = [r for r in canonical_ramps if r["status"] == "active" and r["kind"] in ["general_entry", "general_exit"]]
    verified = [r for r in active_general if r["supportState"] == "verified_bound"]
    unsupported = [r for r in active_general if r["supportState"] == "unsupported"]
    print(f"  Active general ramps: {len(active_general)}")
    print(f"  Verified-bound: {len(verified)}")
    print(f"  Explicit unsupported: {len(unsupported)}")
    print(f"  Boundary ramps: {len(BOUNDARY_DEFS)}")
    print(f"  Closed ramps: {len(CLOSED_RAMPS)}")
    print(f"Generated data/osm-ramp-bindings.json: {len(bindings)} bindings")
    print(f"  Binding candidates: {len(binding_candidates)}")

if __name__ == "__main__":
    main()
