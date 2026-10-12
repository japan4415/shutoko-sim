//! Issue #40: the ramp classification introduced by #32 must hold when the extract carries
//! more than one route. The issue was written while the extraction targeted C1 alone; the
//! checked-in `fixtures/osm/shutoko-all.json` now carries the whole network (C1 + C2 + the
//! radial / Kanagawa / Saitama lines), so the design claims can be pinned against real data
//! instead of being argued from the code.
//!
//! Claims under test, from the issue:
//!
//! | scenario | expected | test |
//! | --- | --- | --- |
//! | C1 and C2 both present | a junction connector touches mainline at both ends, so it is neither an Entry nor an Exit → Shutoko | [`junction_connectors_between_mainlines_are_shutoko`] |
//! | the whole extract at once | every ramp endpoint is decidable; Entry / Exit / Local counts stay put | [`full_network_kind_counts_and_ramp_decidability_are_pinned`] |
//! | underground sections (C2 山手トンネル) | the mainline is Shutoko through `is_shutoko_motorway()` without a surface check; only the segment reaching the street is Entry / Exit | [`tunnel_ramp_segments_split_at_the_street_end`] |
//!
//! A misclassification keeps the total edge count but moves edges between Entry / Exit /
//! Shutoko, which is why the pins are per kind and per edge id.
//!
//! The remaining claim — "a route added on one side only": the connector's far end leaves the
//! extract, but an expressway junction does not touch a surface street, so it stays Shutoko —
//! is already pinned by `builder_tests::test_real_c1_first_exit_and_benchmark`, which asserts
//! the C1 JCT connector `e:w4848898:0:f` is Shutoko in the C1-only extract.
//!
//! These are plain topology builds; the release CLI additionally runs the exit-reachability
//! filter (16 Shutoko edges become Exit for all-real-v5), whose result is pinned where the
//! CLI runs, in `builder_tests::test_cli_with_full_fixtures`.

use shutoko_graph_builder::{
    build_topology_with_report, EdgeKind, Graph, OverpassResponse, RestrictionReport,
    TopologyConfig,
};
use std::collections::HashMap;
use std::sync::OnceLock;

const OSM_BYTES: &[u8] = include_bytes!("../../../fixtures/osm/shutoko-all.json");

/// Junction connectors that link two mainlines, and therefore must never be an Entry or an
/// Exit. `江戸橋JCT` joins C1 to 6号向島線 (and on to C2); the `箱崎ロータリー` ways are the
/// 箱崎JCT rotary that links C1, 6号向島線 and the 箱崎 exit complex.
///
/// The way ids come from the checked-in extract; every edge of each way is asserted below.
const JCT_CONNECTOR_WAYS: &[(&str, &str)] = &[
    ("40971852", "江戸橋JCT"),
    ("44804643", "江戸橋JCT"),
    ("44805850", "江戸橋JCT"),
    ("817790406", "江戸橋JCT"),
    ("1051967534", "箱崎JCT (箱崎ロータリー)"),
    ("1051967535", "箱崎JCT (箱崎ロータリー)"),
    ("1051967536", "箱崎JCT (箱崎ロータリー)"),
    ("1051967537", "箱崎JCT (箱崎ロータリー)"),
    ("1051967538", "箱崎JCT (箱崎ロータリー)"),
    ("1051967539", "箱崎JCT (箱崎ロータリー)"),
    ("1051967540", "箱崎JCT (箱崎ロータリー)"),
    ("1287664260", "箱崎JCT (箱崎ロータリー)"),
];

/// Street-side segments of C2 山手トンネル ramps: exactly these edges leave the tunnel for a
/// surface street, so exactly these are Entry / Exit. Every other edge of the same way stays
/// Shutoko — the tunnel approach is mainline, not a street-level ramp.
const TUNNEL_RAMP_STREET_EDGES: &[(&str, EdgeKind, &str)] = &[
    (
        "e:w805171062:3:f",
        EdgeKind::Exit,
        "五反田出口 (ramp:c2-inner:gotanda-exit)",
    ),
    (
        "e:w381953860:0:f",
        EdgeKind::Entry,
        "富谷入口 (ramp:c2-outer:tomigaya-entry)",
    ),
    (
        "e:w381950205:0:f",
        EdgeKind::Entry,
        "中野長者橋入口 (ramp:c2-outer:nakano-chojabashi-entry)",
    ),
    (
        "e:w805171060:0:f",
        EdgeKind::Entry,
        "五反田入口 (ramp:c2-outer:gotanda-entry)",
    ),
];

fn real_topology() -> &'static (Graph, RestrictionReport) {
    static TOPOLOGY: OnceLock<(Graph, RestrictionReport)> = OnceLock::new();
    TOPOLOGY.get_or_init(|| {
        let response: OverpassResponse = serde_json::from_slice(OSM_BYTES).unwrap();
        let (graph, _snap_index, report) =
            build_topology_with_report(&response, &TopologyConfig::default()).unwrap();
        (graph, report)
    })
}

/// Edge ids are `e:w<way id>:<segment>:<direction>`, so the way id is the second field.
fn edges_by_way(graph: &Graph) -> HashMap<&str, Vec<(&str, EdgeKind)>> {
    let mut by_way: HashMap<&str, Vec<(&str, EdgeKind)>> = HashMap::new();
    for edge in &graph.edges {
        if let Some(way) = edge
            .id
            .strip_prefix("e:w")
            .and_then(|rest| rest.split(':').next())
        {
            by_way
                .entry(way)
                .or_default()
                .push((edge.id.as_str(), edge.kind));
        }
    }
    by_way
}

/// Edge counts per kind. `EdgeKind` does not implement `Hash`, so the counts are collected
/// into a small struct rather than a map.
#[derive(Debug, Default, Clone, Copy)]
struct KindCounts {
    shutoko: usize,
    entry: usize,
    exit: usize,
    local: usize,
}

fn kind_counts(graph: &Graph) -> KindCounts {
    let mut counts = KindCounts::default();
    for edge in &graph.edges {
        match edge.kind {
            EdgeKind::Shutoko => counts.shutoko += 1,
            EdgeKind::Entry => counts.entry += 1,
            EdgeKind::Exit => counts.exit += 1,
            EdgeKind::Local => counts.local += 1,
        }
    }
    counts
}

#[test]
fn junction_connectors_between_mainlines_are_shutoko() {
    let (graph, _report) = real_topology();
    let by_way = edges_by_way(graph);

    for (way, junction) in JCT_CONNECTOR_WAYS {
        let edges = by_way.get(way).unwrap_or_else(|| {
            panic!("way {way} ({junction}) produced no edges; the extract or the connector changed")
        });
        // A connector that keeps two mainlines joined is part of the ring; a phantom Entry or
        // Exit here would both cut the loop and invent a street-level ramp.
        let wrong = edges
            .iter()
            .filter(|(_, kind)| *kind != EdgeKind::Shutoko)
            .collect::<Vec<_>>();
        assert!(
            wrong.is_empty(),
            "way {way} ({junction}) must be Shutoko at every edge; misclassified: {wrong:?}"
        );
    }
}

#[test]
fn full_network_kind_counts_and_ramp_decidability_are_pinned() {
    let (graph, report) = real_topology();
    let counts = kind_counts(graph);

    // Edge counts, not ramp counts: one ramp is several edges. A silent misclassification
    // moves edges between the three kinds while the total stays the same.
    //
    // These are the numbers of a plain topology build. The release CLI additionally runs the
    // exit-reachability filter over the route memberships, which reclassifies 16 Shutoko edges
    // as Exit (22,637 / 168 / 182 → 22,621 / 168 / 198 for all-real-v5); the release-graph
    // counts are asserted where the CLI runs, in builder_tests' full-fixture test.
    assert_eq!(
        counts.shutoko, 22637,
        "Shutoko mainline / connector edge count changed"
    );
    assert_eq!(counts.entry, 168, "Entry edge count changed");
    assert_eq!(counts.exit, 182, "Exit edge count changed");
    // The output graph is Shutoko-only: surface roads are context, never edges.
    assert_eq!(counts.local, 0, "the graph must not contain Local edges");

    // Every ramp endpoint is decidable with the full extract: classification is structural
    // (surface connection or OSM node tag), so no endpoint falls back to the conservative
    // Shutoko default. A non-zero count means the extract lost surface context.
    assert_eq!(
        report.undecidable_ramp_edges, 0,
        "the full-network extract must classify every ramp endpoint (no undecidable edges)"
    );
}

#[test]
fn tunnel_ramp_segments_split_at_the_street_end() {
    let (graph, _report) = real_topology();
    let by_way = edges_by_way(graph);

    for (edge_id, expected_kind, label) in TUNNEL_RAMP_STREET_EDGES {
        let way = edge_id
            .strip_prefix("e:w")
            .and_then(|rest| rest.split(':').next())
            .expect("edge id carries its way id");
        let edges = by_way
            .get(way)
            .unwrap_or_else(|| panic!("way {way} ({label}) produced no edges"));

        let street = edges
            .iter()
            .filter(|(_, kind)| *kind != EdgeKind::Shutoko)
            .collect::<Vec<_>>();
        assert_eq!(
            street,
            vec![&(*edge_id, *expected_kind)],
            "{label}: exactly the street-side segment must be {expected_kind:?}, all other \
             segments of way {way} must stay Shutoko"
        );
    }
}
