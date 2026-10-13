//! Issue #44: bound ランプの OSM way は、公式の種別どおりに Entry / Exit へ分類される。
//!
//! #44 の完了条件のうち「C2 の入口・出口が正しく分類されることを検証する」と、同 issue が
//! 残した懸念「山手トンネルなど地下区間の本線が `is_shutoko_motorway()` で Shutoko、出口
//! ランプ末端が地上の一般道に接続して Exit になることを**実データで確認していない**」を、
//! 抽出済みの実フィクスチャに対する回帰テストとして閉じる。
//!
//! `multi_route_jct_classification::tunnel_ramp_segments_split_at_the_street_end` は C2 の
//! トンネル区間ランプ 4 本を個別に固定している。この suite はその一般化にあたる:
//!
//! 1. 配布グラフ（`fixtures/generated/graph.json`）で `data/ramp-inventory.json` の
//!    `verified_bound` ランプ **238 件すべて**について、束縛済み OSM way（単一 binding・
//!    multi-way candidate・共有物理セグメント例外のいずれか）が期待種別のエッジを持ち、
//!    **逆種別のエッジを持たない**こと。束縛済みなのに binding が無いランプは fail-closed で
//!    落とす
//! 2. 中央環状線 C2 のトンネル区間 way（`tunnel=yes`）の全エッジが Shutoko であること。
//!    地下区間の本線は地表接続シグナルの対象外で、`is_shutoko_motorway()` だけで本線と
//!    判定される（そのため #37 の地点ごと評価でも種別は動かない）
//!
//! 期待種別は公式台帳の施設種別（`general_entry` → Entry、`general_exit` → Exit）から取る。
//! 束縛 way が Entry と Exit の両方を生む状態は、公式種別とグラフの向きが食い違っている
//! ことを意味するため、片方だけを許す。
//!
//! 素の topology build と配布グラフで 1 件だけ差がある。radial の天現寺出口は素の build では
//! Shutoko のまま残り、release CLI の exit-reachability filter（`builder_tests::
//! test_cli_with_full_fixtures` が固定する 16 エッジの再分類）で Exit になる。この差は
//! [`only_the_radial_tengenji_exit_awaits_the_release_post_pass`] が明示的に固定する。

use shutoko_graph_builder::{
    build_topology_with_report, CanonicalRampInventoryItem, EdgeKind, Graph, OsmRampBindingsFile,
    OverpassResponse, RampInventoryFile, RampKind, RestrictionReport, TopologyConfig,
};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const OSM_BYTES: &[u8] = include_bytes!("../../../fixtures/osm/shutoko-all.json");
const INVENTORY_BYTES: &[u8] = include_bytes!("../../../data/ramp-inventory.json");
const BINDINGS_BYTES: &[u8] = include_bytes!("../../../data/osm-ramp-bindings.json");

/// 公式台帳で `verified_bound` になっているランプの件数（`data/ramp-inventory.json` の実測）。
const VERIFIED_BOUND_RAMPS: usize = 238;
/// そのうち中央環状線 C2 のランプ。
const VERIFIED_BOUND_C2_RAMPS: usize = 25;
/// 中央環状線 C2 のトンネル区間 way と、それらが生むエッジ数（実フィクスチャの実測）。
const C2_TUNNEL_WAYS: usize = 55;
const C2_TUNNEL_EDGES: usize = 1147;

fn real_topology() -> (Graph, RestrictionReport) {
    static CACHE: OnceLock<(Graph, RestrictionReport)> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let osm: OverpassResponse = serde_json::from_slice(OSM_BYTES).unwrap();
            let (graph, _snap, report) =
                build_topology_with_report(&osm, &TopologyConfig::default()).unwrap();
            (graph, report)
        })
        .clone()
}

/// エッジ ID（`e:w<way>:<index>:<dir>`）から way ID を取り出す。
fn way_of_edge(edge_id: &str) -> Option<i64> {
    edge_id
        .strip_prefix("e:w")
        .and_then(|rest| rest.split(':').next())
        .and_then(|way| way.parse::<i64>().ok())
}

/// EdgeKind は Hash を実装しないため、way ごとの種別は重複を除いた Vec で持つ。
fn kinds_by_way(graph: &Graph) -> HashMap<i64, Vec<EdgeKind>> {
    let mut kinds: HashMap<i64, Vec<EdgeKind>> = HashMap::new();
    for edge in &graph.edges {
        if let Some(way) = way_of_edge(&edge.id) {
            let entry = kinds.entry(way).or_default();
            if !entry.contains(&edge.kind) {
                entry.push(edge.kind);
            }
        }
    }
    kinds
}

/// 公式台帳のランプ → 束縛 OSM way。単一 binding に加え、multi-way candidate（天現寺）と
/// 共有物理セグメント例外（加平）が指す way も含める。
fn ways_by_ramp(bindings: &OsmRampBindingsFile) -> HashMap<String, HashSet<i64>> {
    let mut ways: HashMap<String, HashSet<i64>> = HashMap::new();
    for binding in &bindings.bindings {
        ways.entry(binding.ramp_id.clone())
            .or_default()
            .insert(binding.osm_way_id);
    }
    for candidate in &bindings.binding_candidates {
        for segment in &candidate.directed_segments {
            ways.entry(candidate.ramp_id.clone())
                .or_default()
                .extend(segment.osm_way_ids.iter().copied());
        }
    }
    for override_ in &bindings.shared_physical_overrides {
        for ramp_id in &override_.ramp_ids {
            ways.entry(ramp_id.clone())
                .or_default()
                .insert(override_.osm_way_id);
        }
    }
    ways
}

/// C2 のトンネル区間 way（`tunnel=yes` かつ ref=C2 または名称に中央環状線を含む）。
fn c2_tunnel_way_ids(osm: &OverpassResponse) -> HashSet<i64> {
    osm.elements
        .iter()
        .filter(|element| element.is_way())
        .filter(|element| {
            matches!(
                element.get_tag("tunnel"),
                Some("yes" | "building_passage" | "culvert")
            )
        })
        .filter(|element| {
            element.get_tag("ref") == Some("C2")
                || element
                    .get_tag("name")
                    .is_some_and(|name| name.contains("中央環状線"))
        })
        .map(|element| element.id)
        .collect()
}

/// 配布グラフ（release CLI の post-pass 後）を way ごとの種別へ畳む。
fn release_graph_kinds() -> HashMap<i64, Vec<EdgeKind>> {
    static CACHE: OnceLock<HashMap<i64, Vec<EdgeKind>>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let graph: serde_json::Value =
                serde_json::from_str(include_str!("../../../fixtures/generated/graph.json"))
                    .unwrap();
            let mut kinds: HashMap<i64, Vec<EdgeKind>> = HashMap::new();
            for edge in graph["edges"].as_array().expect("edges array") {
                let id = edge["id"].as_str().expect("edge id");
                let kind = match edge["kind"].as_str().expect("edge kind") {
                    "shutoko" => EdgeKind::Shutoko,
                    "entry" => EdgeKind::Entry,
                    "exit" => EdgeKind::Exit,
                    "local" => EdgeKind::Local,
                    other => panic!("unknown edge kind {other:?} in the release graph"),
                };
                if let Some(way) = way_of_edge(id) {
                    let entry = kinds.entry(way).or_default();
                    if !entry.contains(&kind) {
                        entry.push(kind);
                    }
                }
            }
            kinds
        })
        .clone()
}

/// 公式台帳で bound なランプと、その期待種別。台帳と binding の突き合わせはここへ集約する。
fn bound_ramps() -> Vec<(CanonicalRampInventoryItem, EdgeKind, Vec<i64>)> {
    let inventory: RampInventoryFile = serde_json::from_slice(INVENTORY_BYTES).unwrap();
    let bindings: OsmRampBindingsFile = serde_json::from_slice(BINDINGS_BYTES).unwrap();
    let ways = ways_by_ramp(&bindings);
    inventory
        .ramps
        .into_iter()
        .filter(|ramp| ramp.support_state.as_deref() == Some("verified_bound"))
        .map(|ramp| {
            let expected = match ramp.kind {
                RampKind::GeneralEntry => EdgeKind::Entry,
                RampKind::GeneralExit => EdgeKind::Exit,
                other => panic!(
                    "verified_bound ramp {} has kind {other:?}; only general entries and exits \
                     may be bound to a graph edge",
                    ramp.ramp_id
                ),
            };
            let ramp_ways = ways
                .get(&ramp.ramp_id)
                .unwrap_or_else(|| {
                    panic!(
                        "verified_bound ramp {} ({}) has no OSM binding evidence; an unbound \
                         bound ramp would leave the product without a graph endpoint",
                        ramp.ramp_id, ramp.facility_name
                    )
                })
                .iter()
                .copied()
                .collect::<Vec<_>>();
            (ramp, expected, ramp_ways)
        })
        .collect()
}

/// 期待種別と逆種別を、与えられた way 集合のエッジ種別から判定する。
fn classify(
    seen_by_way: &HashMap<i64, Vec<EdgeKind>>,
    ramp_ways: &[i64],
    expected: EdgeKind,
) -> (Vec<EdgeKind>, bool, bool) {
    let opposite = match expected {
        EdgeKind::Entry => EdgeKind::Exit,
        _ => EdgeKind::Entry,
    };
    let mut seen: Vec<EdgeKind> = Vec::new();
    for way in ramp_ways {
        if let Some(kinds) = seen_by_way.get(way) {
            for kind in kinds {
                if !seen.contains(kind) {
                    seen.push(*kind);
                }
            }
        }
    }
    let has_expected = seen.contains(&expected);
    let has_opposite = seen.contains(&opposite);
    (seen, has_expected, has_opposite)
}

#[test]
fn every_bound_ramp_has_its_entry_or_exit_edge_in_the_release_graph() {
    let kinds = release_graph_kinds();
    let ramps = bound_ramps();

    let mut checked_c2 = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (ramp, expected, ramp_ways) in &ramps {
        let (seen, has_expected, has_opposite) = classify(&kinds, ramp_ways, *expected);
        if !has_expected {
            failures.push(format!(
                "{} ({} {}): expected {expected:?} on way(s) {ramp_ways:?}, found {seen:?}",
                ramp.ramp_id, ramp.route, ramp.facility_name
            ));
        } else if has_opposite {
            failures.push(format!(
                "{} ({} {}): way(s) {ramp_ways:?} carry both {expected:?} and the opposite kind; \
                 the graph direction contradicts the official facility kind",
                ramp.ramp_id, ramp.route, ramp.facility_name
            ));
        }
        if ramp.route == "C2" {
            checked_c2 += 1;
        }
    }

    assert!(
        failures.is_empty(),
        "bound ramp classification contract violated:\n{}",
        failures.join("\n")
    );
    assert_eq!(
        ramps.len(),
        VERIFIED_BOUND_RAMPS,
        "the number of verified_bound ramps changed; update this contract together with the \
         inventory"
    );
    assert_eq!(
        checked_c2, VERIFIED_BOUND_C2_RAMPS,
        "the number of bound C2 ramps changed; the C2 completion criteria depend on it"
    );
}

/// 素の topology build では、radial の天現寺出口 1 件だけが Exit になっておらず、
/// release CLI の exit-reachability filter を待つ。ここを「失敗」ではなく「known な差分」
/// として固定しておかないと、次に読む人がテストを緩めてしまう。
#[test]
fn only_the_radial_tengenji_exit_awaits_the_release_post_pass() {
    let (graph, _report) = real_topology();
    let kinds = kinds_by_way(&graph);
    let ramps = bound_ramps();

    let mut awaiting: Vec<String> = Vec::new();
    for (ramp, expected, ramp_ways) in &ramps {
        let (seen, has_expected, has_opposite) = classify(&kinds, ramp_ways, *expected);
        assert!(
            !has_opposite,
            "{} ({}): the plain build must never contradict the official kind (found {seen:?})",
            ramp.ramp_id, ramp.facility_name
        );
        if !has_expected {
            awaiting.push(format!("{} ({})", ramp.ramp_id, ramp.facility_name));
        }
    }

    assert_eq!(
        awaiting,
        vec!["ramp:2-outbound:tengenji-exit (天現寺)".to_string()],
        "the set of bound ramps that need the release post-pass changed"
    );
}

/// 天現寺出口の候補 way（multi-way candidate の 4 way）は、素の build では Shutoko しか生まない。
/// release CLI の exit-reachability filter が街路側の 1 エッジを Exit へ再分類する
/// （`builder_tests::test_cli_with_full_fixtures` がネットワーク全体で 16 エッジの再分類として
/// 固定している）。束縛 way の種別を両層で押さえておくことで、post-pass を消す変更が
/// 「素の build は変わらないが配布グラフから出口が消える」形で落ちる。
#[test]
fn tengenji_exit_candidate_ways_are_shutoko_in_the_plain_build() {
    let (graph, _report) = real_topology();
    let kinds = kinds_by_way(&graph);
    // `data/osm-ramp-bindings.json` の bindingCandidates が持つ multi-way チェーン。
    for way in [172358460_i64, 422023171, 172358461, 931759044] {
        let seen = kinds
            .get(&way)
            .unwrap_or_else(|| panic!("tengenji candidate way {way} produced no edges"));
        assert_eq!(
            seen,
            &vec![EdgeKind::Shutoko],
            "plain build: candidate way {way} must stay Shutoko until the release post-pass"
        );
    }
}
#[test]
fn c2_tunnel_mainline_ways_are_shutoko() {
    let (graph, _report) = real_topology();
    let osm: OverpassResponse = serde_json::from_slice(OSM_BYTES).unwrap();
    let tunnel_ways = c2_tunnel_way_ids(&osm);
    assert_eq!(
        tunnel_ways.len(),
        C2_TUNNEL_WAYS,
        "the C2 tunnel-section way set changed; re-check that underground mainline is still \
         classified by is_shutoko_motorway() alone"
    );

    let mut edges = 0usize;
    let mut offenders: Vec<(String, EdgeKind)> = Vec::new();
    for edge in &graph.edges {
        let Some(way) = way_of_edge(&edge.id) else {
            continue;
        };
        if !tunnel_ways.contains(&way) {
            continue;
        }
        edges += 1;
        if edge.kind != EdgeKind::Shutoko {
            offenders.push((edge.id.clone(), edge.kind));
        }
    }

    assert!(
        offenders.is_empty(),
        "underground C2 mainline must stay Shutoko; misclassified edges: {offenders:?}"
    );
    assert_eq!(
        edges, C2_TUNNEL_EDGES,
        "the number of edges on C2 tunnel-section ways changed"
    );
}
