//! Issue #38: the Shutoko mainline `ref` whitelist (`SHUTOKO_REFS`) has to cover every route
//! relation of the network, and no mainline-class way may leave the graph without declaring
//! another route.
//!
//! `SHUTOKO_REFS` decides which `highway=motorway` ways become Shutoko mainline edges when a
//! way carries only a `ref` (Overpass output commonly drops `operator` / `network`, and some
//! segments carry no Japanese name). A `ref` that is missing from the list is dropped from the
//! graph *silently*, so these tests turn that silence into a failure.
//!
//! The checked-in extract is the release input, so the invariants hold for the release this
//! repository ships. When the extract is refreshed (`scripts/fetch-osm.sh`) and the network
//! gains a route, `every_network_route_relation_ref_is_whitelisted` fails until the new `ref`
//! is added to `SHUTOKO_REFS` — the update procedure, enforced instead of only documented.

use shutoko_graph_builder::{is_shutoko_motorway, is_shutoko_ref, OverpassResponse, SHUTOKO_REFS};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const OSM_BYTES: &[u8] = include_bytes!("../../../fixtures/osm/shutoko-all.json");

/// `type=network` relation that lists every route relation of the Metropolitan Expressway.
/// It is the authority for "which lines does the network have"; the individual route
/// relations are its members.
const NETWORK_RELATION_ID: i64 = 4256559;

/// Mainline-class ways that stay out of the graph although they are members of a Shutoko
/// route relation: the through-route continuation of a connecting expressway at a junction
/// (中央自動車道 at 西新宿JCT, 東北自動車道 at 川口JCT, 第三京浜道路 at 三ツ沢JCT).
///
/// Expected and harmless — the graph is Shutoko-only — but pinned here so that a new
/// unaccounted drop, or a Shutoko line mis-tagged with another route's `ref`, cannot pass
/// silently.
const EXPECTED_FOREIGN_MAINLINE_WAYS: &[(&str, &str, usize)] = &[
    ("E20", "中央自動車道", 4),
    ("E4", "東北自動車道", 2),
    ("E83;466", "第三京浜道路", 3),
];

fn real_osm() -> &'static OverpassResponse {
    static OSM: OnceLock<OverpassResponse> = OnceLock::new();
    OSM.get_or_init(|| serde_json::from_slice(OSM_BYTES).unwrap())
}

fn sorted_refs<I, S>(refs: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut sorted = refs
        .into_iter()
        .map(|r| r.as_ref().to_string())
        .collect::<Vec<_>>();
    sorted.sort();
    sorted
}

#[test]
fn every_network_route_relation_ref_is_whitelisted() {
    let osm = real_osm();

    let network = osm
        .elements
        .iter()
        .find(|element| element.is_relation() && element.id == NETWORK_RELATION_ID)
        .unwrap_or_else(|| {
            panic!("the extract is missing the Shutoko network relation {NETWORK_RELATION_ID}")
        });
    let member_ids = network
        .members
        .as_ref()
        .expect("the network relation has no members")
        .iter()
        .filter(|member| member.member_type == "relation")
        .map(|member| member.ref_id)
        .collect::<Vec<_>>();
    assert!(
        member_ids.len() > 20,
        "the network relation should list the route relations, found {} members",
        member_ids.len()
    );

    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown = Vec::new();
    for id in member_ids {
        let relation = osm
            .elements
            .iter()
            .find(|element| element.is_relation() && element.id == id)
            .unwrap_or_else(|| panic!("route relation {id} is missing from the extract"));
        let reference = relation.get_tag("ref").unwrap_or_else(|| {
            panic!("route relation {id} has no ref; SHUTOKO_REFS cannot recognise its ways")
        });
        *seen.entry(reference.to_string()).or_insert(0) += 1;
        if !is_shutoko_ref(reference) {
            unknown.push(format!("{id} (ref {reference})"));
        }
    }
    assert!(
        unknown.is_empty(),
        "the network lists route relation(s) whose ref is not in SHUTOKO_REFS: {}. \
         Add the ref(s) to crates/graph-builder/src/topology.rs so their mainline ways are not \
         dropped silently, then update this test's expectations.",
        unknown.join(", ")
    );

    // The whitelist and the network agree in both directions: no route is missing (above) and
    // no stale code lingers in the list.
    assert_eq!(
        sorted_refs(seen.keys()),
        sorted_refs(SHUTOKO_REFS),
        "SHUTOKO_REFS and the network's route relation refs differ"
    );

    // K4 / S3 / S4 are absent because the network has no such lines — the reason the list
    // omits them. If OSM ever gains one, this assertion is the place that says so.
    for absent in ["K4", "S3", "S4"] {
        assert!(
            !seen.contains_key(absent),
            "the network gained a {absent} route relation; add it to SHUTOKO_REFS"
        );
        assert!(
            !SHUTOKO_REFS.contains(&absent),
            "SHUTOKO_REFS must not list {absent}: the network has no such line"
        );
    }
}

#[test]
fn mainline_ways_leave_the_graph_only_with_a_foreign_ref() {
    let osm = real_osm();

    let mut classified = 0usize;
    let mut foreign: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut unidentified: Vec<i64> = Vec::new();
    for way in osm.elements.iter().filter(|element| element.is_way()) {
        if way.get_tag("highway") != Some("motorway") {
            continue;
        }
        if is_shutoko_motorway(way) {
            classified += 1;
            continue;
        }
        match way.get_tag("ref") {
            // A mainline way that declares another route (an E-road or another operator's
            // expressway) is a junction continuation, not a Shutoko line we failed to match.
            Some(reference) if !is_shutoko_ref(reference) => {
                let name = way.get_tag("name").unwrap_or_default().to_string();
                *foreign.entry((reference.to_string(), name)).or_insert(0) += 1;
            }
            // No ref at all (or a Shutoko ref that the classifier failed to accept): nothing
            // identifies this way, so a Shutoko mainline could be lost without a trace.
            _ => unidentified.push(way.id),
        }
    }

    assert!(
        classified > 0,
        "no Shutoko mainline way was recognised in the extract"
    );
    assert!(
        unidentified.is_empty(),
        "mainline way(s) {unidentified:?} are neither Shutoko mainline nor tagged with another \
         route's ref; a Shutoko line would be dropped from the graph silently. Either the way \
         needs its ref in SHUTOKO_REFS, or the extract changed and this test's expectations \
         need review."
    );

    let expected = EXPECTED_FOREIGN_MAINLINE_WAYS
        .iter()
        .map(|(reference, name, count)| ((reference.to_string(), name.to_string()), *count))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        foreign, expected,
        "the junction continuations kept out of the graph changed; review them (they must be \
         other operators' expressways) and update EXPECTED_FOREIGN_MAINLINE_WAYS"
    );
}
