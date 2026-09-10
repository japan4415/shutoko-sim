use serde_json::json;
use shutoko_graph_builder::{
    build_topology, build_topology_with_report, haversine_distance_meters, to_deterministic_json,
    EdgeKind, OverpassResponse, TopologyConfig, LOCAL_SPEED_KMH, RAMP_SPEED_KMH, SHUTOKO_SPEED_KMH,
};

#[test]
fn test_haversine_and_speed_constants() {
    let d = haversine_distance_meters(35.681236, 139.767125, 35.671989, 139.763965);
    assert!(d > 1000 && d < 1200);

    assert_eq!(SHUTOKO_SPEED_KMH, 60.0);
    assert_eq!(RAMP_SPEED_KMH, 40.0);
    assert_eq!(LOCAL_SPEED_KMH, 30.0);
}

#[test]
fn test_oneway_expansion() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7600},
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7600},
            // Way 101: oneway=yes
            {
                "type": "way",
                "id": 101,
                "nodes": [1, 2],
                "tags": {
                    "highway": "primary",
                    "oneway": "yes"
                }
            },
            // Way 102: oneway=-1
            {
                "type": "way",
                "id": 102,
                "nodes": [2, 3],
                "tags": {
                    "highway": "primary",
                    "oneway": "-1"
                }
            },
            // Way 103: oneway=no (bidirectional)
            {
                "type": "way",
                "id": 103,
                "nodes": [3, 4],
                "tags": {
                    "highway": "primary",
                    "oneway": "no"
                }
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap) = build_topology(&resp, &config).unwrap();

    let edge_ids: Vec<String> = graph.edges.iter().map(|e| e.id.clone()).collect();

    // 101 forward only
    assert!(edge_ids.contains(&"e:w101:0:f".to_string()));
    assert!(!edge_ids.contains(&"e:w101:0:r".to_string()));

    // 102 reverse only
    assert!(!edge_ids.contains(&"e:w102:0:f".to_string()));
    assert!(edge_ids.contains(&"e:w102:0:r".to_string()));

    // 103 both forward and reverse
    assert!(edge_ids.contains(&"e:w103:0:f".to_string()));
    assert!(edge_ids.contains(&"e:w103:0:r".to_string()));

    assert_eq!(graph.edges.len(), 4);
}

#[test]
fn test_edge_kind_classification_shutoko_entry_exit_local() {
    let json_data = json!({
        "elements": [
            // Local nodes
            {"type": "node", "id": 10, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 11, "lat": 35.6810, "lon": 139.7600},
            // Shutoko mainline nodes
            {"type": "node", "id": 20, "lat": 35.6800, "lon": 139.7650},
            {"type": "node", "id": 21, "lat": 35.6810, "lon": 139.7650},
            // Entry ramp intermediate node
            {"type": "node", "id": 30, "lat": 35.6805, "lon": 139.7625},
            // Exit ramp intermediate node
            {"type": "node", "id": 40, "lat": 35.6805, "lon": 139.7635},
            // Footway node (should be ignored)
            {"type": "node", "id": 99, "lat": 35.6800, "lon": 139.7500},

            // 1. Local road (Way 1)
            {
                "type": "way",
                "id": 1,
                "nodes": [10, 11],
                "tags": {
                    "highway": "primary",
                    "oneway": "yes"
                }
            },
            // 2. Shutoko mainline (Way 2) with ref=C1 and operator
            {
                "type": "way",
                "id": 2,
                "nodes": [20, 21],
                "tags": {
                    "highway": "motorway",
                    "operator": "首都高速道路株式会社",
                    "ref": "C1",
                    "oneway": "yes"
                }
            },
            // 3. Entry ramp: Local (11) -> 30 -> Shutoko (20)
            {
                "type": "way",
                "id": 3,
                "nodes": [11, 30, 20],
                "tags": {
                    "highway": "motorway_link",
                    "oneway": "yes"
                }
            },
            // 4. Exit ramp: Shutoko (21) -> 40 -> Local (10)
            {
                "type": "way",
                "id": 4,
                "nodes": [21, 40, 10],
                "tags": {
                    "highway": "motorway_link",
                    "oneway": "yes"
                }
            },
            // 5. Excluded way: footway
            {
                "type": "way",
                "id": 5,
                "nodes": [10, 99],
                "tags": {
                    "highway": "footway"
                }
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, snap) = build_topology(&resp, &config).unwrap();

    // Verify kinds
    let edge_kinds: std::collections::HashMap<String, EdgeKind> =
        graph.edges.iter().map(|e| (e.id.clone(), e.kind)).collect();

    assert_eq!(edge_kinds.get("e:w1:0:f"), Some(&EdgeKind::Local));
    assert_eq!(edge_kinds.get("e:w2:0:f"), Some(&EdgeKind::Shutoko));

    // Entry ramp segments
    assert_eq!(edge_kinds.get("e:w3:0:f"), Some(&EdgeKind::Entry));
    assert_eq!(edge_kinds.get("e:w3:1:f"), Some(&EdgeKind::Shutoko));

    // Exit ramp segments
    assert_eq!(edge_kinds.get("e:w4:0:f"), Some(&EdgeKind::Shutoko));
    assert_eq!(edge_kinds.get("e:w4:1:f"), Some(&EdgeKind::Exit));

    // Footway was excluded
    assert!(!edge_kinds.contains_key("e:w5:0:f"));

    // SnapIndex contains local road nodes (n:10 and n:11)
    let snap_ids: Vec<String> = snap.nodes.iter().map(|n| n.id.clone()).collect();
    assert!(snap_ids.contains(&"n:10".to_string()));
    assert!(snap_ids.contains(&"n:11".to_string()));
    // Shutoko nodes should not be in snap index
    assert!(!snap_ids.contains(&"n:20".to_string()));
}

#[test]
fn test_grade_separation_different_layers_do_not_connect() {
    // Two ways geometrically cross at (approx 35.681, 139.761),
    // but they are on different layers (0 and 1) and do not share any OSM node IDs.
    let json_data = json!({
        "elements": [
            // Way 10: Local road (layer 0) running West-East
            {"type": "node", "id": 1, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7620},
            {
                "type": "way",
                "id": 10,
                "nodes": [1, 2],
                "tags": {
                    "highway": "primary",
                    "layer": "0",
                    "oneway": "yes"
                }
            },
            // Way 20: Shutoko elevated bridge (layer 1) running South-North
            {"type": "node", "id": 3, "lat": 35.6800, "lon": 139.7610},
            {"type": "node", "id": 4, "lat": 35.6820, "lon": 139.7610},
            {
                "type": "way",
                "id": 20,
                "nodes": [3, 4],
                "tags": {
                    "highway": "motorway",
                    "ref": "C1",
                    "layer": "1",
                    "bridge": "yes",
                    "oneway": "yes"
                }
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap) = build_topology(&resp, &config).unwrap();

    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.edges.len(), 2);

    // Edges are strictly from 1->2 and 3->4
    let edge_pairs: Vec<(String, String)> = graph
        .edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();
    assert_eq!(
        edge_pairs,
        vec![
            ("n:1".to_string(), "n:2".to_string()),
            ("n:3".to_string(), "n:4".to_string()),
        ]
    );

    // Verify that edges only connect nodes within the same way, not across crossing ways
    for edge in &graph.edges {
        if edge.from == "n:1" {
            assert_eq!(edge.to, "n:2");
        } else if edge.from == "n:3" {
            assert_eq!(edge.to, "n:4");
        } else {
            panic!("Unexpected edge from: {}", edge.from);
        }
    }
}

#[test]
fn test_turn_restriction_extraction() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600}, // via node
            {"type": "node", "id": 3, "lat": 35.6810, "lon": 139.7590}, // left turn destination
            {"type": "node", "id": 4, "lat": 35.6820, "lon": 139.7600}, // straight destination

            // Way 10: 1 -> 2
            {
                "type": "way",
                "id": 10,
                "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Way 20: 2 -> 3 (left turn)
            {
                "type": "way",
                "id": 20,
                "nodes": [2, 3],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Way 30: 2 -> 4 (straight)
            {
                "type": "way",
                "id": 30,
                "nodes": [2, 4],
                "tags": {"highway": "primary", "oneway": "yes"}
            },

            // Relation: no_left_turn from Way 10 to Way 20 via Node 2
            {
                "type": "relation",
                "id": 1000,
                "tags": {
                    "type": "restriction",
                    "restriction": "no_left_turn"
                },
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 20, "role": "to"},
                    {"type": "node", "ref": 2, "role": "via"}
                ]
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap) = build_topology(&resp, &config).unwrap();

    assert_eq!(graph.forbidden_transitions.len(), 1);
    assert_eq!(
        graph.forbidden_transitions[0],
        vec!["e:w10:0:f".to_string(), "e:w20:0:f".to_string()]
    );
}

#[test]
fn test_deterministic_output_and_sorting() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 50, "lat": 35.6850, "lon": 139.7650},
            {"type": "node", "id": 10, "lat": 35.6810, "lon": 139.7610},
            {"type": "node", "id": 30, "lat": 35.6830, "lon": 139.7630},
            {
                "type": "way",
                "id": 99,
                "nodes": [50, 10],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            {
                "type": "way",
                "id": 11,
                "nodes": [10, 30],
                "tags": {"highway": "primary", "oneway": "yes"}
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig {
        release_id: "test-rel".into(),
        vehicle_profile: "passenger-car-etc".into(),
    };
    let (graph, _snap) = build_topology(&resp, &config).unwrap();

    // Check node sorting
    let node_ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(node_ids, vec!["n:10", "n:30", "n:50"]);

    // Check edge sorting
    let edge_ids: Vec<&str> = graph.edges.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(edge_ids, vec!["e:w11:0:f", "e:w99:0:f"]);

    // Check deterministic serialization
    let json_str = to_deterministic_json(&graph).unwrap();
    assert!(json_str.ends_with('\n'));
    assert!(!json_str.contains(".0,") && !json_str.contains(".5,")); // no floats in Graph json
}

#[test]
fn test_empty_relations_input() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {
                "type": "way",
                "id": 10,
                "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap) = build_topology(&resp, &config).unwrap();

    assert_eq!(graph.forbidden_transitions.len(), 0);
    assert_eq!(graph.billing_pairs.len(), 0);
}

#[test]
fn test_routing_core_search_integration() {
    use shutoko_graph_builder::{BillingPair, Price, VerificationStatus};
    use shutoko_routing_core::{search, SearchLimits, SearchRequest};

    // Construct a complete route network:
    // Local: 1 -> 2
    // Entry: 2 -> 3 (anchor)
    // Shutoko cycle: 3 -> 4 -> 5 -> 3
    // Exit: 4 -> 6
    // Local return: 6 -> 1
    let json_data = json!({
        "elements": [
            // Local nodes
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 6, "lat": 35.6790, "lon": 139.7600},

            // Highway nodes
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7650}, // anchor
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7650},
            {"type": "node", "id": 5, "lat": 35.6825, "lon": 139.7670},

            // Ways
            // Local access: 1 -> 2
            {
                "type": "way", "id": 1, "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Entry ramp: 2 -> 3
            {
                "type": "way", "id": 2, "nodes": [2, 3],
                "tags": {"highway": "motorway_link", "oneway": "yes"}
            },
            // Shutoko loop: 3 -> 4 -> 5 -> 3
            {
                "type": "way", "id": 3, "nodes": [3, 4],
                "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}
            },
            {
                "type": "way", "id": 4, "nodes": [4, 5],
                "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}
            },
            {
                "type": "way", "id": 5, "nodes": [5, 3],
                "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}
            },
            // Exit ramp: 4 -> 6
            {
                "type": "way", "id": 6, "nodes": [4, 6],
                "tags": {"highway": "motorway_link", "oneway": "yes"}
            },
            // Local return: 6 -> 1
            {
                "type": "way", "id": 7, "nodes": [6, 1],
                "tags": {"highway": "primary", "oneway": "yes"}
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig {
        release_id: "integration-rel".into(),
        vehicle_profile: "passenger-car-etc".into(),
    };
    let (mut graph, _snap) = build_topology(&resp, &config).unwrap();

    // Attach verified BillingPair
    graph.billing_pairs.push(BillingPair {
        id: "bp-test-1".into(),
        entry_id: "e:w2:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w2:0:f".into()],
        anchor_to_exit_edge_ids: vec!["e:w3:0:f".into(), "e:w6:0:f".into()],
        status: VerificationStatus::Verified,
        vehicle_profile: "passenger-car-etc".into(),
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    });

    let req = SearchRequest {
        request_id: "req-test-1".into(),
        release_id: "integration-rel".into(),
        origin_node_id: "n:1".into(),
        min_minutes: 1,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };

    let limits = SearchLimits::default();
    let res = search(&graph, &req, &limits).expect("search should succeed");
    assert_eq!(res.status, "ok");
    assert!(!res.candidates.is_empty(), "expected candidates found");
    assert_eq!(res.candidates[0].entry_id, "e:w2:0:f");
    assert_eq!(res.candidates[0].exit_id, "e:w6:0:f");
    assert_eq!(res.candidates[0].toll.amount_yen, Some(300));
}

// =========================================================================
// Tests for Phase 2: BillingPair Generation, Validation, Manifest & CLI
// =========================================================================

use shutoko_graph_builder::{
    build_manifest, compute_sha256, generate_and_validate_billing_pairs, generate_billing_pair,
    manifest_to_deterministic_json, snap_index_to_deterministic_json, validate_billing_pair,
    BillingError, BillingPair, BillingPairSeed, BillingPairsSeedFile, ManifestConfig, Price,
    SeedPrice, SeedProvenance, VerificationStatus,
};

fn create_test_loop_graph() -> (
    shutoko_graph_builder::Graph,
    shutoko_graph_builder::SnapIndex,
) {
    let json_data = json!({
        "elements": [
            // Local nodes
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 6, "lat": 35.6790, "lon": 139.7600},

            // Highway nodes
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7650}, // anchor
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7650},
            {"type": "node", "id": 5, "lat": 35.6825, "lon": 139.7670},

            // Ways
            // Local access: 1 -> 2
            {
                "type": "way", "id": 1, "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Entry ramp: 2 -> 3
            {
                "type": "way", "id": 2, "nodes": [2, 3],
                "tags": {"highway": "motorway_link", "oneway": "yes"}
            },
            // Shutoko loop: 3 -> 4 -> 5 -> 3
            {
                "type": "way", "id": 3, "nodes": [3, 4],
                "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}
            },
            {
                "type": "way", "id": 4, "nodes": [4, 5],
                "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}
            },
            {
                "type": "way", "id": 5, "nodes": [5, 3],
                "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}
            },
            // Exit ramp: 4 -> 6
            {
                "type": "way", "id": 6, "nodes": [4, 6],
                "tags": {"highway": "motorway_link", "oneway": "yes"}
            },
            // Local return: 6 -> 1
            {
                "type": "way", "id": 7, "nodes": [6, 1],
                "tags": {"highway": "primary", "oneway": "yes"}
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig {
        release_id: "test-rel".into(),
        vehicle_profile: "passenger-car-etc".into(),
    };
    build_topology(&resp, &config).unwrap()
}

#[test]
fn test_billing_pair_seed_and_pathfinding_success() {
    let (mut graph, _snap) = create_test_loop_graph();

    let seed = BillingPairSeed {
        id: "bp:c1:shibakoen-kasumigaseki".into(),
        entry_osm_way_id: 2,
        exit_osm_way_id: 6,
        anchor_osm_node_id: 3,
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        one_section_ahead_verified: true,
        provenance: SeedProvenance {
            source: "https://www.shutoko.jp/tariff".into(),
            source_date: "2026-09-10".into(),
            notes: Some("Verified manually".into()),
        },
        prices: vec![
            SeedPrice {
                amount_yen: 300,
                effective_from: "2026-01-01T00:00:00Z".into(),
                effective_to: Some("2026-10-01T00:00:00Z".into()),
            },
            SeedPrice {
                amount_yen: 350,
                effective_from: "2026-10-01T00:00:00Z".into(),
                effective_to: None,
            },
        ],
    };

    let pair = generate_billing_pair(&graph, &seed).expect("generation should succeed");
    assert_eq!(pair.entry_id, "e:w2:0:f");
    assert_eq!(pair.exit_id, "e:w6:0:f");
    assert_eq!(pair.anchor_node_id, "n:3");
    assert_eq!(pair.entry_to_anchor_edge_ids, vec!["e:w2:0:f"]);
    assert_eq!(pair.anchor_to_exit_edge_ids, vec!["e:w3:0:f", "e:w6:0:f"]);
    assert_eq!(pair.prices.len(), 2);

    // Verify it passes validation
    assert!(validate_billing_pair(&graph, &pair).is_ok());

    // Integrate with routing core search
    graph.billing_pairs.push(pair);
    let req = shutoko_routing_core::SearchRequest {
        request_id: "req-1".into(),
        release_id: "test-rel".into(),
        origin_node_id: "n:1".into(),
        min_minutes: 1,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let res =
        shutoko_routing_core::search(&graph, &req, &shutoko_routing_core::SearchLimits::default())
            .expect("search should succeed");
    assert_eq!(res.status, "ok");
    assert_eq!(res.candidates[0].toll.amount_yen, Some(300));
}

#[test]
fn test_reject_hidden_loop_in_billing_pair() {
    let (graph, _snap) = create_test_loop_graph();

    // Construct a billing pair where the direct entry-to-exit path visits node 3 twice
    // (i.e. contains a hidden loop: 3 -> 4 -> 5 -> 3 before going to exit 4 -> 6)
    let invalid_pair = BillingPair {
        id: "bp-hidden-loop".into(),
        entry_id: "e:w2:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w2:0:f".into()],
        // Hidden loop in anchorToExit: traverses 3 -> 4 -> 5 -> 3 -> 4 -> 6
        anchor_to_exit_edge_ids: vec![
            "e:w3:0:f".into(),
            "e:w4:0:f".into(),
            "e:w5:0:f".into(),
            "e:w3:0:f".into(),
            "e:w6:0:f".into(),
        ],
        status: VerificationStatus::Verified,
        vehicle_profile: "passenger-car-etc".into(),
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let res = validate_billing_pair(&graph, &invalid_pair);
    assert!(res.is_err(), "should reject hidden loop in direct path");
    let err = res.unwrap_err();
    assert_eq!(err.rule, "HIDDEN_LOOP_OR_CYCLE");
    assert!(err.message.contains("visited more than once"));
}

#[test]
fn test_reject_graph_with_no_loop_from_anchor() {
    // Create a linear graph with NO loop (1 -> 2 -> 3 -> 4 -> 6)
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7650}, // anchor
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7650},
            {"type": "node", "id": 6, "lat": 35.6790, "lon": 139.7600},

            // Local access: 1 -> 2
            {
                "type": "way", "id": 1, "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Entry ramp: 2 -> 3
            {
                "type": "way", "id": 2, "nodes": [2, 3],
                "tags": {"highway": "motorway_link", "oneway": "yes"}
            },
            // Shutoko segment: 3 -> 4 (no return loop!)
            {
                "type": "way", "id": 3, "nodes": [3, 4],
                "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}
            },
            // Exit ramp: 4 -> 6
            {
                "type": "way", "id": 6, "nodes": [4, 6],
                "tags": {"highway": "motorway_link", "oneway": "yes"}
            },
            // Local return: 6 -> 1
            {
                "type": "way", "id": 7, "nodes": [6, 1],
                "tags": {"highway": "primary", "oneway": "yes"}
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap) = build_topology(&resp, &config).unwrap();

    let pair = BillingPair {
        id: "bp-no-loop".into(),
        entry_id: "e:w2:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w2:0:f".into()],
        anchor_to_exit_edge_ids: vec!["e:w3:0:f".into(), "e:w6:0:f".into()],
        status: VerificationStatus::Verified,
        vehicle_profile: "passenger-car-etc".into(),
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let res = validate_billing_pair(&graph, &pair);
    assert!(res.is_err(), "should reject graph without loop from anchor");
    let err = res.unwrap_err();
    assert_eq!(err.rule, "NO_SHUTOKO_LOOP");
    assert!(err.message.contains("no valid non-empty Shutoko loop"));
}

#[test]
fn test_forbidden_transitions_not_adopted_and_rejected() {
    let (mut graph, _snap) = create_test_loop_graph();

    // Add forbidden transition between entry (e:w2:0:f) and loop entry (e:w3:0:f)
    graph
        .forbidden_transitions
        .push(vec!["e:w2:0:f".into(), "e:w3:0:f".into()]);

    let seed = BillingPairSeed {
        id: "bp-forbidden".into(),
        entry_osm_way_id: 2,
        exit_osm_way_id: 6,
        anchor_osm_node_id: 3,
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        one_section_ahead_verified: true,
        provenance: SeedProvenance {
            source: "https://test.example.com".into(),
            source_date: "2026-09-10".into(),
            notes: None,
        },
        prices: vec![SeedPrice {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    // Case 1: Generator fails because transition into exit path from anchor is forbidden
    let res = generate_billing_pair(&graph, &seed);
    assert!(res.is_err());
    let err = res.unwrap_err();
    match err {
        BillingError::NoPathAnchorToExit(_) | BillingError::ValidationFailed(_) => {}
        other => panic!(
            "expected NoPathAnchorToExit or ValidationFailed, got {:?}",
            other
        ),
    }

    // Case 2: Validator directly rejects pair that contains forbidden transition
    let explicit_pair = BillingPair {
        id: "bp-forbidden-manual".into(),
        entry_id: "e:w2:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w2:0:f".into()],
        anchor_to_exit_edge_ids: vec!["e:w3:0:f".into(), "e:w6:0:f".into()],
        status: VerificationStatus::Verified,
        vehicle_profile: "passenger-car-etc".into(),
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let val_res = validate_billing_pair(&graph, &explicit_pair);
    assert!(val_res.is_err());
    assert_eq!(val_res.unwrap_err().rule, "FORBIDDEN_TRANSITION");
}

#[test]
fn test_reject_disconnected_path() {
    let (graph, _snap) = create_test_loop_graph();

    let mut pair = BillingPair {
        id: "bp-disc".into(),
        entry_id: "e:w2:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w2:0:f".into()],
        anchor_to_exit_edge_ids: vec!["e:w4:0:f".into(), "e:w6:0:f".into()], // e:w4 starts at n:4, but anchor is n:3!
        status: VerificationStatus::Verified,
        vehicle_profile: "passenger-car-etc".into(),
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let err = validate_billing_pair(&graph, &pair).unwrap_err();
    assert_eq!(err.rule, "ANCHOR_CONNECTION_MISMATCH");

    // Internal disconnection
    pair.anchor_to_exit_edge_ids = vec!["e:w3:0:f".into(), "e:w5:0:f".into(), "e:w6:0:f".into()]; // e:w3 ends at 4, but e:w5 starts at 5!
    pair.exit_id = "e:w6:0:f".into();
    let err2 = validate_billing_pair(&graph, &pair).unwrap_err();
    assert_eq!(err2.rule, "DISCONNECTED_PATH");
}

#[test]
fn test_reject_invalid_edge_kinds() {
    let (graph, _snap) = create_test_loop_graph();

    // Entry edge is actually Local
    let pair = BillingPair {
        id: "bp-wrong-kind".into(),
        entry_id: "e:w1:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w1:0:f".into()],
        anchor_to_exit_edge_ids: vec!["e:w3:0:f".into(), "e:w6:0:f".into()],
        status: VerificationStatus::Verified,
        vehicle_profile: "passenger-car-etc".into(),
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let err = validate_billing_pair(&graph, &pair).unwrap_err();
    assert_eq!(err.rule, "INVALID_EDGE_KIND");
}

#[test]
fn test_reject_invalid_prices_and_overlapping_intervals() {
    let (graph, _snap) = create_test_loop_graph();

    let mut pair = BillingPair {
        id: "bp-prices".into(),
        entry_id: "e:w2:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w2:0:f".into()],
        anchor_to_exit_edge_ids: vec!["e:w3:0:f".into(), "e:w6:0:f".into()],
        status: VerificationStatus::Verified,
        vehicle_profile: "passenger-car-etc".into(),
        prices: vec![Price {
            amount_yen: 0, // Invalid 0 amount!
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let err = validate_billing_pair(&graph, &pair).unwrap_err();
    assert_eq!(err.rule, "INVALID_PRICE_AMOUNT");

    // Overlapping intervals
    pair.prices = vec![
        Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: Some("2026-07-01T00:00:00Z".into()),
        },
        Price {
            amount_yen: 350,
            effective_from: "2026-06-01T00:00:00Z".into(), // Overlaps with June!
            effective_to: None,
        },
    ];
    let err2 = validate_billing_pair(&graph, &pair).unwrap_err();
    assert_eq!(err2.rule, "OVERLAPPING_PRICES");

    // Reversed interval (effective_to < effective_from)
    pair.prices = vec![Price {
        amount_yen: 300,
        effective_from: "2026-10-01T00:00:00Z".into(),
        effective_to: Some("2026-01-01T00:00:00Z".into()),
    }];
    let err3 = validate_billing_pair(&graph, &pair).unwrap_err();
    assert_eq!(err3.rule, "INVALID_PRICE_INTERVAL");
}

#[test]
fn test_reject_mismatched_vehicle_profile() {
    let (graph, _snap) = create_test_loop_graph();

    let pair = BillingPair {
        id: "bp-mismatch-profile".into(),
        entry_id: "e:w2:0:f".into(),
        exit_id: "e:w6:0:f".into(),
        anchor_node_id: "n:3".into(),
        entry_to_anchor_edge_ids: vec!["e:w2:0:f".into()],
        anchor_to_exit_edge_ids: vec!["e:w3:0:f".into(), "e:w6:0:f".into()],
        status: VerificationStatus::Verified,
        vehicle_profile: "heavy-truck".into(), // graph is passenger-car-etc
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let err = validate_billing_pair(&graph, &pair).unwrap_err();
    assert_eq!(err.rule, "VEHICLE_PROFILE_MISMATCH");
}

#[test]
fn test_reject_unverified_section_marked_verified() {
    let (graph, _snap) = create_test_loop_graph();

    let seed = BillingPairSeed {
        id: "bp-unverified-flag".into(),
        entry_osm_way_id: 2,
        exit_osm_way_id: 6,
        anchor_osm_node_id: 3,
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        one_section_ahead_verified: false, // Inconsistent with status=verified!
        provenance: SeedProvenance {
            source: "https://test.example.com".into(),
            source_date: "2026-09-10".into(),
            notes: None,
        },
        prices: vec![SeedPrice {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let err = generate_billing_pair(&graph, &seed).unwrap_err();
    assert!(matches!(
        err,
        BillingError::UnverifiedSectionMarkedAsVerified(_)
    ));
}

#[test]
fn test_manifest_generation_and_checksum_verification() {
    let (graph, snap) = create_test_loop_graph();
    let graph_json = to_deterministic_json(&graph).unwrap();
    let snap_json = snap_index_to_deterministic_json(&snap).unwrap();

    let config = ManifestConfig {
        release_id: "rel-manifest-test".into(),
        engine_version: "0.1.0".into(),
        graph_version: "1.0.0".into(),
        built_at: "2026-09-10T00:00:00Z".into(),
        source_date: "2026-09-10".into(),
        coverage_area: "Tokyo C1 Inner Circular".into(),
        vehicle_profile: "passenger-car-etc".into(),
        time_model_version: "v1-static-speeds".into(),
        billing_pairs_version: "v1".into(),
        unverified_sections: vec!["unverified-ramp-x".into()],
        provenance: vec![],
    };

    let manifest = build_manifest(
        &config,
        vec!["e:w2:0:f".into()],
        vec!["e:w6:0:f".into()],
        vec![
            ("graph.json", graph_json.as_bytes()),
            ("snap-index.json", snap_json.as_bytes()),
        ],
    );

    assert_eq!(manifest.release_id, "rel-manifest-test");
    assert_eq!(manifest.attribution, "© OpenStreetMap contributors");
    assert_eq!(manifest.built_at, "2026-09-10T00:00:00Z");
    assert_eq!(manifest.artifacts.len(), 2);

    let graph_art = manifest
        .artifacts
        .iter()
        .find(|a| a.path == "graph.json")
        .unwrap();
    assert_eq!(graph_art.sha256, compute_sha256(graph_json.as_bytes()));
    assert_eq!(graph_art.byte_length, graph_json.len() as u64);

    let manifest_json = manifest_to_deterministic_json(&manifest).unwrap();
    assert!(manifest_json.ends_with('\n'));
}

#[test]
fn test_deterministic_byte_identical_output_two_runs() {
    let (mut graph, snap) = create_test_loop_graph();

    let seed_file = BillingPairsSeedFile {
        schema_version: 1,
        description: Some("Determinism test seed".into()),
        billing_pairs: vec![BillingPairSeed {
            id: "bp:test:deterministic".into(),
            entry_osm_way_id: 2,
            exit_osm_way_id: 6,
            anchor_osm_node_id: 3,
            vehicle_profile: "passenger-car-etc".into(),
            status: VerificationStatus::Verified,
            one_section_ahead_verified: true,
            provenance: SeedProvenance {
                source: "https://test.example.com".into(),
                source_date: "2026-09-10".into(),
                notes: None,
            },
            prices: vec![SeedPrice {
                amount_yen: 300,
                effective_from: "2026-01-01T00:00:00Z".into(),
                effective_to: None,
            }],
        }],
    };

    let rep = generate_and_validate_billing_pairs(&graph, &seed_file);
    assert_eq!(rep.valid_pairs.len(), 1);
    graph.billing_pairs = rep.valid_pairs;

    // Run 1 serialization
    let graph_json_1 = to_deterministic_json(&graph).unwrap();
    let snap_json_1 = snap_index_to_deterministic_json(&snap).unwrap();
    let manifest_cfg = ManifestConfig {
        release_id: "fixed-release-id".into(),
        engine_version: "0.1.0".into(),
        graph_version: "1.0.0".into(),
        built_at: "2026-09-10T12:00:00Z".into(),
        source_date: "2026-09-10".into(),
        coverage_area: "Deterministic Area".into(),
        vehicle_profile: "passenger-car-etc".into(),
        time_model_version: "v1-static-speeds".into(),
        billing_pairs_version: "v1".into(),
        unverified_sections: vec![],
        provenance: vec![],
    };
    let manifest_1 = build_manifest(
        &manifest_cfg,
        vec!["e:w2:0:f".into()],
        vec!["e:w6:0:f".into()],
        vec![
            ("graph.json", graph_json_1.as_bytes()),
            ("snap-index.json", snap_json_1.as_bytes()),
        ],
    );
    let manifest_json_1 = manifest_to_deterministic_json(&manifest_1).unwrap();

    // Run 2 serialization
    let graph_json_2 = to_deterministic_json(&graph).unwrap();
    let snap_json_2 = snap_index_to_deterministic_json(&snap).unwrap();
    let manifest_2 = build_manifest(
        &manifest_cfg,
        vec!["e:w2:0:f".into()],
        vec!["e:w6:0:f".into()],
        vec![
            ("graph.json", graph_json_2.as_bytes()),
            ("snap-index.json", snap_json_2.as_bytes()),
        ],
    );
    let manifest_json_2 = manifest_to_deterministic_json(&manifest_2).unwrap();

    // Assert exact byte equality
    assert_eq!(graph_json_1.as_bytes(), graph_json_2.as_bytes());
    assert_eq!(snap_json_1.as_bytes(), snap_json_2.as_bytes());
    assert_eq!(manifest_json_1.as_bytes(), manifest_json_2.as_bytes());
}

#[test]
fn test_cli_full_end_to_end_execution() {
    let tmp_dir = std::env::temp_dir().join(format!("shutoko-test-{}", std::process::id()));
    let osm_path = tmp_dir.join("osm.json");
    let seed_path = tmp_dir.join("seed.json");
    let out_dir_1 = tmp_dir.join("out1");
    let out_dir_2 = tmp_dir.join("out2");

    let _ = std::fs::create_dir_all(&tmp_dir);

    let osm_json = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 6, "lat": 35.6790, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7650},
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7650},
            {"type": "node", "id": 5, "lat": 35.6825, "lon": 139.7670},

            {"type": "way", "id": 1, "nodes": [1, 2], "tags": {"highway": "primary", "oneway": "yes"}},
            {"type": "way", "id": 2, "nodes": [2, 3], "tags": {"highway": "motorway_link", "oneway": "yes"}},
            {"type": "way", "id": 3, "nodes": [3, 4], "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}},
            {"type": "way", "id": 4, "nodes": [4, 5], "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}},
            {"type": "way", "id": 5, "nodes": [5, 3], "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}},
            {"type": "way", "id": 6, "nodes": [4, 6], "tags": {"highway": "motorway_link", "oneway": "yes"}},
            {"type": "way", "id": 7, "nodes": [6, 1], "tags": {"highway": "primary", "oneway": "yes"}}
        ]
    });
    std::fs::write(&osm_path, serde_json::to_string_pretty(&osm_json).unwrap()).unwrap();

    let seed_json = json!({
        "schemaVersion": 1,
        "description": "CLI test seed",
        "billingPairs": [
            {
                "id": "bp:test:cli",
                "entryOsmWayId": 2,
                "exitOsmWayId": 6,
                "anchorOsmNodeId": 3,
                "vehicleProfile": "passenger-car-etc",
                "status": "verified",
                "oneSectionAheadVerified": true,
                "provenance": {
                    "source": "https://test.example.com",
                    "sourceDate": "2026-09-10"
                },
                "prices": [
                    {
                        "amountYen": 300,
                        "effectiveFrom": "2026-01-01T00:00:00Z"
                    }
                ]
            }
        ]
    });
    std::fs::write(
        &seed_path,
        serde_json::to_string_pretty(&seed_json).unwrap(),
    )
    .unwrap();

    let bin_path = env!("CARGO_BIN_EXE_shutoko-graph-builder");

    // Run 1
    let status1 = std::process::Command::new(bin_path)
        .args([
            "--osm",
            osm_path.to_str().unwrap(),
            "--seed",
            seed_path.to_str().unwrap(),
            "--out-dir",
            out_dir_1.to_str().unwrap(),
            "--release-id",
            "cli-test-rel",
            "--built-at",
            "2026-09-10T00:00:00Z",
            "--source-date",
            "2026-09-10",
        ])
        .status()
        .expect("failed to execute binary (run 1)");
    assert!(status1.success(), "run 1 failed");

    // Run 2
    let status2 = std::process::Command::new(bin_path)
        .args([
            "--osm",
            osm_path.to_str().unwrap(),
            "--seed",
            seed_path.to_str().unwrap(),
            "--out-dir",
            out_dir_2.to_str().unwrap(),
            "--release-id",
            "cli-test-rel",
            "--built-at",
            "2026-09-10T00:00:00Z",
            "--source-date",
            "2026-09-10",
        ])
        .status()
        .expect("failed to execute binary (run 2)");
    assert!(status2.success(), "run 2 failed");

    // Verify byte-for-byte identical output files
    for filename in &["graph.json", "snap-index.json", "manifest.json"] {
        let b1 = std::fs::read(out_dir_1.join(filename)).expect("missing file in out1");
        let b2 = std::fs::read(out_dir_2.join(filename)).expect("missing file in out2");
        assert_eq!(b1, b2, "byte mismatch in {}", filename);
    }

    // Clean up
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_turn_restriction_only_straight_on_forbids_alternative_outgoings() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600}, // via node
            {"type": "node", "id": 3, "lat": 35.6810, "lon": 139.7590}, // left branch
            {"type": "node", "id": 4, "lat": 35.6820, "lon": 139.7600}, // straight branch

            // Way 10: 1 -> 2
            {
                "type": "way", "id": 10, "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Way 20: 2 -> 3 (left branch)
            {
                "type": "way", "id": 20, "nodes": [2, 3],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Way 30: 2 -> 4 (straight branch)
            {
                "type": "way", "id": 30, "nodes": [2, 4],
                "tags": {"highway": "primary", "oneway": "yes"}
            },

            // Relation: only_straight_on from Way 10 to Way 30 via Node 2
            {
                "type": "relation",
                "id": 2000,
                "tags": {
                    "type": "restriction",
                    "restriction": "only_straight_on"
                },
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 30, "role": "to"},
                    {"type": "node", "ref": 2, "role": "via"}
                ]
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap, report) = build_topology_with_report(&resp, &config).unwrap();

    assert_eq!(report.only_turn_via_node, 1);
    assert_eq!(report.only_turn_edge_pairs, 1);
    // Should forbid Way 10 -> Way 20, NOT Way 10 -> Way 30
    assert_eq!(graph.forbidden_transitions.len(), 1);
    assert_eq!(
        graph.forbidden_transitions[0],
        vec!["e:w10:0:f".to_string(), "e:w20:0:f".to_string()]
    );
}

#[test]
fn test_turn_restriction_via_way_sequence() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7600},
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7600},

            // Way 10: 1 -> 2 (from)
            {
                "type": "way", "id": 10, "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Way 20: 2 -> 3 (via)
            {
                "type": "way", "id": 20, "nodes": [2, 3],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            // Way 30: 3 -> 4 (to)
            {
                "type": "way", "id": 30, "nodes": [3, 4],
                "tags": {"highway": "primary", "oneway": "yes"}
            },

            // Relation: no_u_turn from Way 10 to Way 30 via Way 20
            {
                "type": "relation",
                "id": 3000,
                "tags": {
                    "type": "restriction",
                    "restriction": "no_u_turn"
                },
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 20, "role": "via"},
                    {"type": "way", "ref": 30, "role": "to"}
                ]
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap, report) = build_topology_with_report(&resp, &config).unwrap();

    assert_eq!(report.via_way, 1);
    assert_eq!(graph.forbidden_transitions.len(), 1);
    assert_eq!(
        graph.forbidden_transitions[0],
        vec![
            "e:w10:0:f".to_string(),
            "e:w20:0:f".to_string(),
            "e:w30:0:f".to_string(),
        ]
    );
}

#[test]
fn test_turn_restriction_conditional_skipped() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6810, "lon": 139.7590},

            {
                "type": "way", "id": 10, "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            {
                "type": "way", "id": 20, "nodes": [2, 3],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            {
                "type": "relation",
                "id": 4000,
                "tags": {
                    "type": "restriction",
                    "restriction:conditional": "no_right_turn @ (07:00-09:00)"
                },
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 20, "role": "to"},
                    {"type": "node", "ref": 2, "role": "via"}
                ]
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap, report) = build_topology_with_report(&resp, &config).unwrap();

    assert_eq!(report.skipped_conditional, 1);
    assert_eq!(graph.forbidden_transitions.len(), 0);
}

#[test]
fn test_turn_restriction_only_via_way_skipped() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7600},
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7600},

            {
                "type": "way", "id": 10, "nodes": [1, 2],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            {
                "type": "way", "id": 20, "nodes": [2, 3],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            {
                "type": "way", "id": 30, "nodes": [3, 4],
                "tags": {"highway": "primary", "oneway": "yes"}
            },
            {
                "type": "relation",
                "id": 5000,
                "tags": {
                    "type": "restriction",
                    "restriction": "only_straight_on"
                },
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 20, "role": "via"},
                    {"type": "way", "ref": 30, "role": "to"}
                ]
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap, report) = build_topology_with_report(&resp, &config).unwrap();

    assert_eq!(report.skipped_only_via_way, 1);
    assert_eq!(graph.forbidden_transitions.len(), 0);
}

#[test]
fn test_turn_restriction_unrecognized_skipped_and_balanced() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6810, "lon": 139.7590},

            {"type": "way", "id": 10, "nodes": [1, 2], "tags": {"highway": "primary", "oneway": "yes"}},
            {"type": "way", "id": 20, "nodes": [2, 3], "tags": {"highway": "primary", "oneway": "yes"}},

            // Relation with unrecognized restriction value "no_entry"
            {
                "type": "relation",
                "id": 6000,
                "tags": {
                    "type": "restriction",
                    "restriction": "no_entry"
                },
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 20, "role": "to"},
                    {"type": "node", "ref": 2, "role": "via"}
                ]
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (_graph, _snap, report) = build_topology_with_report(&resp, &config).unwrap();

    assert_eq!(report.total_relations, 1);
    assert_eq!(report.skipped_unrecognized, 1);
    assert!(report.is_balanced());
    assert_eq!(report.total_accounted(), 1);
}

#[test]
fn test_turn_restriction_only_turn_deduplication() {
    let json_data = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7600},
            {"type": "node", "id": 4, "lat": 35.6810, "lon": 139.7610},

            // From: Way 10 (1 -> 2)
            {"type": "way", "id": 10, "nodes": [1, 2], "tags": {"highway": "primary", "oneway": "yes"}},
            // To: Way 20 (2 -> 3)
            {"type": "way", "id": 20, "nodes": [2, 3], "tags": {"highway": "primary", "oneway": "yes"}},
            // Alternative outgoing: Way 30 (2 -> 4)
            {"type": "way", "id": 30, "nodes": [2, 4], "tags": {"highway": "primary", "oneway": "yes"}},

            // Relation 1: only_straight_on (Way 10 -> Way 20) => forbids Way 10 -> Way 30
            {
                "type": "relation",
                "id": 7001,
                "tags": {"type": "restriction", "restriction": "only_straight_on"},
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 20, "role": "to"},
                    {"type": "node", "ref": 2, "role": "via"}
                ]
            },
            // Relation 2: duplicate only_straight_on (same from, to, via)
            {
                "type": "relation",
                "id": 7002,
                "tags": {"type": "restriction", "restriction": "only_straight_on"},
                "members": [
                    {"type": "way", "ref": 10, "role": "from"},
                    {"type": "way", "ref": 20, "role": "to"},
                    {"type": "node", "ref": 2, "role": "via"}
                ]
            }
        ]
    });

    let resp: OverpassResponse = serde_json::from_value(json_data).unwrap();
    let config = TopologyConfig::default();
    let (graph, _snap, report) = build_topology_with_report(&resp, &config).unwrap();

    assert_eq!(report.total_relations, 2);
    assert_eq!(report.only_turn_via_node, 2);
    // Even though 2 relations applied, only 1 unique forbidden pair (10 -> 30) was inserted
    assert_eq!(report.only_turn_edge_pairs, 1);
    assert_eq!(graph.forbidden_transitions.len(), 1);
    assert!(report.is_balanced());
}

#[test]
fn test_real_c1_turn_restriction_balance() {
    let osm_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/osm/shutoko-c1.json");
    let osm_content = std::fs::read_to_string(&osm_path).expect("failed to read shutoko-c1.json");
    let resp: OverpassResponse = serde_json::from_str(&osm_content).unwrap();

    let config = TopologyConfig::default();
    let (_graph, _snap, report) = build_topology_with_report(&resp, &config).unwrap();

    assert_eq!(
        report.total_relations, 151,
        "C1 real dataset contains exactly 151 turn restriction relations"
    );
    assert!(
        report.is_balanced(),
        "all 151 relations must be accounted for without leakage: accounted={}, total={}",
        report.total_accounted(),
        report.total_relations
    );
    assert_eq!(report.no_turn_via_node, 35);
    assert_eq!(report.only_turn_via_node, 28);
    assert_eq!(report.only_turn_edge_pairs, 23);
    assert_eq!(report.via_way, 7);
    assert_eq!(report.skipped_conditional, 8);
    assert_eq!(report.skipped_no_via, 5);
    assert_eq!(report.skipped_missing_elements, 67);
    assert_eq!(report.skipped_disconnected, 1);
    assert_eq!(report.skipped_only_via_way, 0);
    assert_eq!(report.skipped_unrecognized, 0);
}

#[test]
fn test_real_c1_first_exit_and_benchmark() {
    use std::time::Instant;

    let osm_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/osm/shutoko-c1.json");
    let osm_content = std::fs::read_to_string(&osm_path).expect("failed to read shutoko-c1.json");
    let resp: OverpassResponse = serde_json::from_str(&osm_content).unwrap();

    let config = TopologyConfig::default();
    let (graph, _snap) = build_topology(&resp, &config).unwrap();
    assert_eq!(graph.edges.len(), 7422, "C1 graph must have 7,422 edges");

    // Anchor node for Kandabashi entry is n:499831338
    let anchor = "n:499831338";

    let start = Instant::now();
    let (min_dist, first_exits) =
        shutoko_graph_builder::find_first_exits_from_anchor(&graph, anchor)
            .expect("first exit search must succeed on real C1 graph");
    let elapsed = start.elapsed();

    eprintln!(
        "Real C1 (7422 edges) find_first_exits_from_anchor elapsed: {:?}, dist: {}m, exits: {:?}",
        elapsed, min_dist, first_exits
    );

    // Verify requirement 4: (2027, ["e:w297864314:11:f"])
    assert_eq!(min_dist, 2027, "first exit distance must be exactly 2027m");
    assert_eq!(
        first_exits,
        vec!["e:w297864314:11:f".to_string()],
        "first exit must remain Takaracho exit e:w297864314:11:f"
    );
}

#[test]
fn test_refutation_first_exit_mismatch_shintomicho() {
    let osm_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/osm/shutoko-c1.json");
    let osm_content = std::fs::read_to_string(&osm_path).expect("failed to read shutoko-c1.json");
    let resp: OverpassResponse = serde_json::from_str(&osm_content).unwrap();

    let config = TopologyConfig::default();
    let (graph, _snap) = build_topology(&resp, &config).unwrap();

    // Valid seed uses Takaracho exit (297864314)
    // Refutation test uses Shintomicho exit (760760233) which is downstream after Takaracho
    let invalid_seed = BillingPairSeed {
        id: "bp-refutation-shintomicho".into(),
        entry_osm_way_id: 92243921,    // Kandabashi entry
        exit_osm_way_id: 760760233,    // Shintomicho exit
        anchor_osm_node_id: 499831338, // Anchor node
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        one_section_ahead_verified: true,
        provenance: SeedProvenance {
            source: "https://www.shutoko.jp/tariff".into(),
            source_date: "2026-09-10".into(),
            notes: Some("Deliberately trying to verify a second exit".into()),
        },
        prices: vec![SeedPrice {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let result = generate_billing_pair(&graph, &invalid_seed);
    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        BillingError::ValidationFailed(validation_err) => {
            assert_eq!(validation_err.rule, "FIRST_EXIT_MISMATCH");
            assert!(
                validation_err.message.contains("e:w297864314:11:f"),
                "error message should indicate expected first exit: {}",
                validation_err.message
            );
        }
        other => panic!(
            "expected ValidationFailed(FIRST_EXIT_MISMATCH), got {:?}",
            other
        ),
    }
}

#[test]
fn test_provenance_url_and_date_validation() {
    let (graph, _snap) = create_test_loop_graph();

    // 1. Invalid URL scheme (ftp)
    let seed_ftp = BillingPairSeed {
        id: "bp-prov-ftp".into(),
        entry_osm_way_id: 2,
        exit_osm_way_id: 6,
        anchor_osm_node_id: 3,
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        one_section_ahead_verified: true,
        provenance: SeedProvenance {
            source: "ftp://example.com/rates".into(),
            source_date: "2026-09-10".into(),
            notes: None,
        },
        prices: vec![SeedPrice {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };
    let err = generate_billing_pair(&graph, &seed_ftp).unwrap_err();
    assert!(matches!(err, BillingError::InvalidProvenance(_)));

    // 2. Not a URL (plain string)
    let mut seed_plain = seed_ftp.clone();
    seed_plain.provenance.source = "official-guide-v1".into();
    let err = generate_billing_pair(&graph, &seed_plain).unwrap_err();
    assert!(matches!(err, BillingError::InvalidProvenance(_)));

    // 3. Invalid date (Feb 31)
    let mut seed_bad_date = seed_ftp.clone();
    seed_bad_date.provenance.source = "https://www.shutoko.jp".into();
    seed_bad_date.provenance.source_date = "2026-02-31".into();
    let err = generate_billing_pair(&graph, &seed_bad_date).unwrap_err();
    assert!(matches!(err, BillingError::InvalidProvenance(_)));

    // 4. Invalid date format
    let mut seed_bad_fmt = seed_bad_date.clone();
    seed_bad_fmt.provenance.source_date = "2026/09/10".into();
    let err = generate_billing_pair(&graph, &seed_bad_fmt).unwrap_err();
    assert!(matches!(err, BillingError::InvalidProvenance(_)));

    // 5. Invalid domains (single dot, empty labels, trailing dot, invalid chars)
    for bad_url in [
        "https://.",
        "https://..",
        "https://.com",
        "https://example.",
        "https://example..com",
        "https://-example.com",
        "https://example-.com",
        "https://exam ple.com",
    ] {
        let mut seed_bad_url = seed_ftp.clone();
        seed_bad_url.provenance.source = bad_url.into();
        let err = generate_billing_pair(&graph, &seed_bad_url).unwrap_err();
        assert!(
            matches!(err, BillingError::InvalidProvenance(_)),
            "expected URL \"{}\" to be rejected as invalid provenance",
            bad_url
        );
    }
}

#[test]
fn test_cli_strict_mode() {
    let tmp_dir = std::env::temp_dir().join(format!("shutoko-test-strict-{}", std::process::id()));
    let osm_path = tmp_dir.join("osm.json");
    let out_dir_normal = tmp_dir.join("out-normal");
    let out_dir_strict_ok = tmp_dir.join("out-strict-ok");
    let out_dir_strict_fail = tmp_dir.join("out-strict-fail");

    let _ = std::fs::create_dir_all(&tmp_dir);

    let osm_json = json!({
        "elements": [
            {"type": "node", "id": 1, "lat": 35.6800, "lon": 139.7600},
            {"type": "node", "id": 2, "lat": 35.6810, "lon": 139.7600},
            {"type": "node", "id": 6, "lat": 35.6790, "lon": 139.7600},
            {"type": "node", "id": 3, "lat": 35.6820, "lon": 139.7650},
            {"type": "node", "id": 4, "lat": 35.6830, "lon": 139.7650},
            {"type": "node", "id": 5, "lat": 35.6825, "lon": 139.7670},

            {"type": "way", "id": 1, "nodes": [1, 2], "tags": {"highway": "primary", "oneway": "yes"}},
            {"type": "way", "id": 2, "nodes": [2, 3], "tags": {"highway": "motorway_link", "oneway": "yes"}},
            {"type": "way", "id": 3, "nodes": [3, 4], "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}},
            {"type": "way", "id": 4, "nodes": [4, 5], "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}},
            {"type": "way", "id": 5, "nodes": [5, 3], "tags": {"highway": "motorway", "ref": "C1", "oneway": "yes"}},
            {"type": "way", "id": 6, "nodes": [4, 6], "tags": {"highway": "motorway_link", "oneway": "yes"}},
            {"type": "way", "id": 7, "nodes": [6, 1], "tags": {"highway": "primary", "oneway": "yes"}}
        ]
    });
    std::fs::write(&osm_path, serde_json::to_string_pretty(&osm_json).unwrap()).unwrap();

    let verified_seed_path = tmp_dir.join("verified-seed.json");
    let verified_seed_json = json!({
        "schemaVersion": 1,
        "description": "Verified seed",
        "billingPairs": [
            {
                "id": "bp:test:verified",
                "entryOsmWayId": 2,
                "exitOsmWayId": 6,
                "anchorOsmNodeId": 3,
                "vehicleProfile": "passenger-car-etc",
                "status": "verified",
                "oneSectionAheadVerified": true,
                "provenance": {
                    "source": "https://test.example.com",
                    "sourceDate": "2026-09-10"
                },
                "prices": [
                    {
                        "amountYen": 300,
                        "effectiveFrom": "2026-01-01T00:00:00Z"
                    }
                ]
            }
        ]
    });
    std::fs::write(
        &verified_seed_path,
        serde_json::to_string_pretty(&verified_seed_json).unwrap(),
    )
    .unwrap();

    let unverified_seed_path = tmp_dir.join("unverified-seed.json");
    let unverified_seed_json = json!({
        "schemaVersion": 1,
        "description": "Unverified seed",
        "billingPairs": []
    });
    std::fs::write(
        &unverified_seed_path,
        serde_json::to_string_pretty(&unverified_seed_json).unwrap(),
    )
    .unwrap();

    let bin_path = env!("CARGO_BIN_EXE_shutoko-graph-builder");

    // Case 1: Verified seed with --strict succeeds
    let status1 = std::process::Command::new(bin_path)
        .args([
            "--osm",
            osm_path.to_str().unwrap(),
            "--seed",
            verified_seed_path.to_str().unwrap(),
            "--out-dir",
            out_dir_strict_ok.to_str().unwrap(),
            "--release-id",
            "test-strict-ok",
            "--built-at",
            "2026-09-10T00:00:00Z",
            "--source-date",
            "2026-09-10",
            "--strict",
        ])
        .status()
        .expect("failed to execute binary (case 1)");
    assert!(
        status1.success(),
        "expected --strict to succeed when verified billing pairs are generated"
    );

    // Case 2: Unverified seed without --strict succeeds
    let status2 = std::process::Command::new(bin_path)
        .args([
            "--osm",
            osm_path.to_str().unwrap(),
            "--seed",
            unverified_seed_path.to_str().unwrap(),
            "--out-dir",
            out_dir_normal.to_str().unwrap(),
            "--release-id",
            "test-normal",
            "--built-at",
            "2026-09-10T00:00:00Z",
            "--source-date",
            "2026-09-10",
        ])
        .status()
        .expect("failed to execute binary (case 2)");
    assert!(
        status2.success(),
        "expected non-strict mode to succeed even when no verified pairs exist"
    );

    // Case 3: Unverified seed with --strict fails
    let status3 = std::process::Command::new(bin_path)
        .args([
            "--osm",
            osm_path.to_str().unwrap(),
            "--seed",
            unverified_seed_path.to_str().unwrap(),
            "--out-dir",
            out_dir_strict_fail.to_str().unwrap(),
            "--release-id",
            "test-strict-fail",
            "--built-at",
            "2026-09-10T00:00:00Z",
            "--source-date",
            "2026-09-10",
            "--strict",
        ])
        .status()
        .expect("failed to execute binary (case 3)");
    assert!(
        !status3.success(),
        "expected --strict to fail when no verified billing pairs are generated"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_refutation_unsound_pruning_codex_counterexample() {
    use shutoko_graph_builder::validate_billing_pair;
    use shutoko_graph_builder::{
        BillingPair, Edge, EdgeKind, Graph, Node, Price, VerificationStatus,
    };

    // Codex counterexample topology:
    // Anchor: n:1
    // Node N: n:2
    // Path 1 to n:2: e1 (dist: 1)
    // Path 2 to n:2: e2 (dist: 2) -> n:3 -> e3 (dist: 2) (total dist: 4)
    // Exit 1 from n:2: exit1 (dist: 1) -> n:4
    // Forbidden transition: ["e1", "exit1"]
    //   -> Path 1 to exit1 is forbidden.
    //   -> Path 2 to exit1 is valid (total dist: 4 + 1 = 5).
    // Later exit from n:2: e4 (dist: 5) -> n:5 -> exit2 (dist: 5) -> n:6
    //   -> Path 1 to exit2 is valid (total dist: 1 + 5 + 5 = 11).
    // Shutoko loop from n:1: n:1 -> n:2 -> n:5 -> n:1
    let graph = Graph {
        schema_version: 1,
        release_id: "test-codex-counterexample".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node {
                id: "n:entry".into(),
            },
            Node { id: "n:1".into() },
            Node { id: "n:2".into() },
            Node { id: "n:3".into() },
            Node { id: "n:4".into() },
            Node { id: "n:5".into() },
            Node { id: "n:6".into() },
        ],
        edges: vec![
            Edge {
                id: "entry".into(),
                from: "n:entry".into(),
                to: "n:1".into(),
                distance_meters: 10,
                duration_seconds: 1,
                kind: EdgeKind::Entry,
            },
            Edge {
                id: "e1".into(),
                from: "n:1".into(),
                to: "n:2".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e2".into(),
                from: "n:1".into(),
                to: "n:3".into(),
                distance_meters: 2,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e3".into(),
                from: "n:3".into(),
                to: "n:2".into(),
                distance_meters: 2,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e4".into(),
                from: "n:2".into(),
                to: "n:5".into(),
                distance_meters: 5,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e_loop".into(),
                from: "n:5".into(),
                to: "n:1".into(),
                distance_meters: 10,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "exit1".into(),
                from: "n:2".into(),
                to: "n:4".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Exit,
            },
            Edge {
                id: "exit2".into(),
                from: "n:5".into(),
                to: "n:6".into(),
                distance_meters: 5,
                duration_seconds: 1,
                kind: EdgeKind::Exit,
            },
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["e1".into(), "exit1".into()]],
    };

    // 1. find_first_exits_from_anchor must return the legitimate first exit (exit1 at distance 5),
    // NOT the later exit (exit2 at distance 11) caused by unsound node-level pruning.
    let (min_dist, first_exits) =
        shutoko_graph_builder::find_first_exits_from_anchor(&graph, "n:1").unwrap();
    assert_eq!(
        min_dist, 5,
        "first exit distance must be 5m via e2->e3->exit1, not 11m"
    );
    assert_eq!(
        first_exits,
        vec!["exit1".to_string()],
        "first exit must be exit1"
    );

    // 2. A verified billing pair targeting later-exit (exit2) must be rejected with FIRST_EXIT_MISMATCH
    let pair_later_exit = BillingPair {
        id: "bp-codex-later-exit".into(),
        entry_id: "entry".into(),
        exit_id: "exit2".into(),
        anchor_node_id: "n:1".into(),
        entry_to_anchor_edge_ids: vec!["entry".into()],
        anchor_to_exit_edge_ids: vec!["e1".into(), "e4".into(), "exit2".into()],
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };

    let result = validate_billing_pair(&graph, &pair_later_exit);
    assert!(
        result.is_err(),
        "verified seed targeting later exit must fail"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.rule, "FIRST_EXIT_MISMATCH",
        "expected rule FIRST_EXIT_MISMATCH, got {}",
        err.rule
    );
    assert!(
        err.message.contains("exit1"),
        "error message must mention true first exit exit1: {}",
        err.message
    );
}

#[test]
fn test_refutation_counterexample_a_opus5() {
    use shutoko_graph_builder::{find_first_exits_from_anchor, Edge, EdgeKind, Graph, Node};

    // Counterexample A (Claude Opus 5):
    // Topology:
    // Shutoko edges:
    //   e1: A -> B (dist: 10)
    //   e2: B -> C (dist: 1)
    //   e3: A -> W (dist: 1)
    //   e4: W -> C (dist: 1)
    //   e5: C -> V (dist: 1)
    //   e6: V -> W (dist: 1)
    //   e7: W -> X (dist: 1)
    // Exit edge:
    //   e8: X -> OUT (dist: 1)
    // Forbidden transitions: [["e3", "e7"]] (L=2, history_len=1)
    //
    // Calculation under simple-path semantics with visited-set state keys (issue #6 item 2):
    // The 6m walk A -e3-> W -e4-> C -e5-> V -e6-> W revisits W, so it is not a simple path.
    // The shortest legal simple path to OUT is:
    //   A -(e1:10)-> B -(e2:1)-> C -(e5:1)-> V -(e6:1)-> W -(e7:1)-> X -(e8:1)-> OUT
    // Cost calculation:
    //   e1 (10) + e2 (1) + e5 (1) + e6 (1) + e7 (1) + e8 (1) = 15m.
    // Forbidden transition verification:
    //   Sequence of edges: [e1, e2, e5, e6, e7, e8]
    //   The forbidden transition ["e3", "e7"] never appears as a consecutive window.
    //   The alternative simple path A -e3-> W -e7-> X matches the forbidden window
    //   [e3, e7] and is rejected.
    //
    // History of this fixture:
    //   The old (node, suffix)-only key reported 6m: the cheap walk reached (V, [e5]) at
    //   cost 3 and pruned the simple alternative A->e1->B->e2->C->e5->V (cost 12, same
    //   key), but the two arrivals have different futures under the simple-path rule.
    //   With the visited-node set included in the state key the 6m looping walk is no
    //   longer legal and the answer is the 15m simple path.
    let graph = Graph {
        schema_version: 1,
        release_id: "test-counterexample-a".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node { id: "A".into() },
            Node { id: "B".into() },
            Node { id: "C".into() },
            Node { id: "W".into() },
            Node { id: "V".into() },
            Node { id: "X".into() },
            Node { id: "OUT".into() },
        ],
        edges: vec![
            Edge {
                id: "e1".into(),
                from: "A".into(),
                to: "B".into(),
                distance_meters: 10,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e2".into(),
                from: "B".into(),
                to: "C".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e3".into(),
                from: "A".into(),
                to: "W".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e4".into(),
                from: "W".into(),
                to: "C".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e5".into(),
                from: "C".into(),
                to: "V".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e6".into(),
                from: "V".into(),
                to: "W".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e7".into(),
                from: "W".into(),
                to: "X".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e8".into(),
                from: "X".into(),
                to: "OUT".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Exit,
            },
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["e3".into(), "e7".into()]],
    };

    let result = find_first_exits_from_anchor(&graph, "A");
    assert!(
        result.is_ok(),
        "search must not fail with Err: {:?}",
        result.err()
    );
    let (min_dist, first_exits) = result.unwrap();
    assert_eq!(
        min_dist, 15,
        "first exit distance under simple-path semantics must be 15m"
    );
    assert_eq!(first_exits, vec!["e8".to_string()], "first exit must be e8");
}

#[test]
fn test_refutation_counterexample_b_opus5() {
    use shutoko_graph_builder::{
        find_first_exits_from_anchor, validate_billing_pair, BillingPair, Edge, EdgeKind, Graph,
        Node, Price, VerificationStatus,
    };

    // Counterexample B (Claude Opus 5):
    // Extends Counterexample A by adding:
    //   e9: A -> Z (dist: 40, Shutoko)
    //   e10: Z -> OUT2 (dist: 1, Exit)
    //   e_loop: X -> A (dist: 10, Shutoko) -- to provide a valid Shutoko loop from anchor A
    //
    // Under simple-path semantics with visited-set state keys (issue #6 item 2):
    //   True first exit is e8 at distance 15m via simple path
    //   A-e1-B-e2-C-e5-V-e6-W-e7-X-e8 (the 6m walk A-e3-W-e4-C-e5-V-e6-W revisits W).
    //   A verified billing pair targeting downstream/alternative exit e10 must be REJECTED
    //   with FIRST_EXIT_MISMATCH.
    //   A verified billing pair targeting true exit e8 along simple path
    //   A-e1-B-e2-C-e5-V-e6-W-e7-X-e8 must be ACCEPTED (simple path contract on billing pair is satisfied).
    //
    // Prior walk-semantics behavior: the same topology returned (6, ["e8"]) because the
    // looping 6m walk was allowed.
    let graph = Graph {
        schema_version: 1,
        release_id: "test-counterexample-b".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node {
                id: "entry_node".into(),
            },
            Node { id: "A".into() },
            Node { id: "B".into() },
            Node { id: "C".into() },
            Node { id: "W".into() },
            Node { id: "V".into() },
            Node { id: "X".into() },
            Node { id: "Z".into() },
            Node { id: "OUT".into() },
            Node { id: "OUT2".into() },
        ],
        edges: vec![
            Edge {
                id: "entry_e".into(),
                from: "entry_node".into(),
                to: "A".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Entry,
            },
            Edge {
                id: "e1".into(),
                from: "A".into(),
                to: "B".into(),
                distance_meters: 10,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e2".into(),
                from: "B".into(),
                to: "C".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e3".into(),
                from: "A".into(),
                to: "W".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e4".into(),
                from: "W".into(),
                to: "C".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e5".into(),
                from: "C".into(),
                to: "V".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e6".into(),
                from: "V".into(),
                to: "W".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e7".into(),
                from: "W".into(),
                to: "X".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e8".into(),
                from: "X".into(),
                to: "OUT".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Exit,
            },
            Edge {
                id: "e9".into(),
                from: "A".into(),
                to: "Z".into(),
                distance_meters: 40,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "e10".into(),
                from: "Z".into(),
                to: "OUT2".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Exit,
            },
            Edge {
                id: "e_loop".into(),
                from: "X".into(),
                to: "A".into(),
                distance_meters: 10,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["e3".into(), "e7".into()]],
    };

    let (min_dist, first_exits) = find_first_exits_from_anchor(&graph, "A").unwrap();
    assert_eq!(min_dist, 15);
    assert_eq!(first_exits, vec!["e8".to_string()]);

    // 1. Target incorrect downstream exit e10 -> MUST FAIL with FIRST_EXIT_MISMATCH
    let pair_e10 = BillingPair {
        id: "bp-e10".into(),
        entry_id: "entry_e".into(),
        exit_id: "e10".into(),
        anchor_node_id: "A".into(),
        entry_to_anchor_edge_ids: vec!["entry_e".into()],
        anchor_to_exit_edge_ids: vec!["e9".into(), "e10".into()],
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };
    let err = validate_billing_pair(&graph, &pair_e10).unwrap_err();
    assert_eq!(err.rule, "FIRST_EXIT_MISMATCH");
    assert!(
        err.message.contains("e8"),
        "error should identify e8 as the true first exit: {}",
        err.message
    );

    // 2. Target correct first exit e8 along simple path e1->e2->e5->e6->e7->e8 -> MUST SUCCEED
    let pair_e8 = BillingPair {
        id: "bp-e8".into(),
        entry_id: "entry_e".into(),
        exit_id: "e8".into(),
        anchor_node_id: "A".into(),
        entry_to_anchor_edge_ids: vec!["entry_e".into()],
        anchor_to_exit_edge_ids: vec![
            "e1".into(),
            "e2".into(),
            "e5".into(),
            "e6".into(),
            "e7".into(),
            "e8".into(),
        ],
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };
    let ok = validate_billing_pair(&graph, &pair_e8);
    assert!(
        ok.is_ok(),
        "valid simple path to true first exit e8 must be accepted: {:?}",
        ok.err()
    );
}

#[test]
fn test_refutation_counterexample_c_codex() {
    use shutoko_graph_builder::{
        find_first_exits_from_anchor, validate_billing_pair, BillingPair, Edge, EdgeKind, Graph,
        Node, Price, VerificationStatus,
    };

    // Counterexample C (Codex topology):
    // Graph:
    // Anchor: a
    // Nodes: a, x, y, c, n, z, out1, out2
    // Edges:
    //   entry_e: entry -> a (dist: 1, Entry)
    //   ax: a -> x (dist: 1, Shutoko)
    //   xc: x -> c (dist: 1, Shutoko)
    //   ay: a -> y (dist: 2, Shutoko)
    //   yc: y -> c (dist: 1, Shutoko)
    //   cn: c -> n (dist: 1, Shutoko)
    //   nx: n -> x (dist: 1, Shutoko)
    //   exit1: x -> out1 (dist: 2, Exit)
    //   nz: n -> z (dist: 4, Shutoko)
    //   exit2: z -> out2 (dist: 6, Exit)
    //   loop_e: z -> a (dist: 10, Shutoko)
    // Forbidden transitions: [["ax", "exit1"]] (L=2)
    //
    // Calculation under simple-path semantics with visited-set state keys (issue #6 item 2):
    // The 6m walk a -(ax)-> x -(xc)-> c -(cn)-> n -(nx)-> x revisits x and is not simple.
    // Shortest legal simple path to exit1:
    //   a -(ay:2)-> y -(yc:1)-> c -(cn:1)-> n -(nx:1)-> x -(exit1:2)-> out1
    // Cost calculation:
    //   ay(2) + yc(1) + cn(1) + nx(1) + exit1(2) = 7m.
    // Forbidden transition check:
    //   Path edges: [ay, yc, cn, nx, exit1].
    //   Contiguous 2-edge windows: (ay,yc), (yc,cn), (cn,nx), (nx,exit1).
    //   None match ["ax", "exit1"]. Legal!
    //   Alternative simple path a->x->c->n->z->exit2 costs 1+1+1+4+6 = 13m.
    //   The shortest simple path yields exit1 at distance 7m.
    //
    // History of this fixture:
    //   With the old (node, suffix)-only key the cheap walk a->x->c->n (cost 3, suffix
    //   [cn]) pruned a->y->c->n (cost 4, same key) and the answer depended on which
    //   arrival's future was kept. Including the visited-node set in the key gives the
    //   distinct keys (x, [nx], {a,y,c,n,x}) etc. and yields the simple-path 7m.
    let graph = Graph {
        schema_version: 1,
        release_id: "test-counterexample-c".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node { id: "entry".into() },
            Node { id: "a".into() },
            Node { id: "x".into() },
            Node { id: "y".into() },
            Node { id: "c".into() },
            Node { id: "n".into() },
            Node { id: "z".into() },
            Node { id: "out1".into() },
            Node { id: "out2".into() },
        ],
        edges: vec![
            Edge {
                id: "entry_e".into(),
                from: "entry".into(),
                to: "a".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Entry,
            },
            Edge {
                id: "ax".into(),
                from: "a".into(),
                to: "x".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "xc".into(),
                from: "x".into(),
                to: "c".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "ay".into(),
                from: "a".into(),
                to: "y".into(),
                distance_meters: 2,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "yc".into(),
                from: "y".into(),
                to: "c".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "cn".into(),
                from: "c".into(),
                to: "n".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "nx".into(),
                from: "n".into(),
                to: "x".into(),
                distance_meters: 1,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "exit1".into(),
                from: "x".into(),
                to: "out1".into(),
                distance_meters: 2,
                duration_seconds: 1,
                kind: EdgeKind::Exit,
            },
            Edge {
                id: "nz".into(),
                from: "n".into(),
                to: "z".into(),
                distance_meters: 4,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
            Edge {
                id: "exit2".into(),
                from: "z".into(),
                to: "out2".into(),
                distance_meters: 6,
                duration_seconds: 1,
                kind: EdgeKind::Exit,
            },
            Edge {
                id: "loop_e".into(),
                from: "z".into(),
                to: "a".into(),
                distance_meters: 10,
                duration_seconds: 1,
                kind: EdgeKind::Shutoko,
            },
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["ax".into(), "exit1".into()]],
    };

    let (min_dist, first_exits) = find_first_exits_from_anchor(&graph, "a").unwrap();
    assert_eq!(
        min_dist, 7,
        "first exit must be exit1 at distance 7m (simple path), not exit2 at 13m"
    );
    assert_eq!(first_exits, vec!["exit1".to_string()]);

    // Later exit2 must be rejected with FIRST_EXIT_MISMATCH
    let pair_exit2 = BillingPair {
        id: "bp-codex-exit2".into(),
        entry_id: "entry_e".into(),
        exit_id: "exit2".into(),
        anchor_node_id: "a".into(),
        entry_to_anchor_edge_ids: vec!["entry_e".into()],
        anchor_to_exit_edge_ids: vec![
            "ax".into(),
            "xc".into(),
            "cn".into(),
            "nz".into(),
            "exit2".into(),
        ],
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        prices: vec![Price {
            amount_yen: 300,
            effective_from: "2026-01-01T00:00:00Z".into(),
            effective_to: None,
        }],
    };
    let err = validate_billing_pair(&graph, &pair_exit2).unwrap_err();
    assert_eq!(err.rule, "FIRST_EXIT_MISMATCH");
    assert!(
        err.message.contains("exit1"),
        "error message must point to true first exit exit1: {}",
        err.message
    );
}

#[test]
fn test_issue6_item1_walk_semantics_node_revisit() {
    use shutoko_graph_builder::{has_non_empty_shutoko_loop, Edge, EdgeKind, Graph, Node};

    let mk = |id: &str, from: &str, to: &str| Edge {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        distance_meters: 1,
        duration_seconds: 1,
        kind: EdgeKind::Shutoko,
    };

    // (a) Scout minimal counterexample (issue #6 item 1).
    // (a) は scout 提示グラフ。旧コードでも true になる非判別ケースで、判別性は (b)(c) が担う。
    // (In the old code, outgoing["Anchor"]=[e1, e3] popped (A, [e1]) first, and since
    // outgoing["A"] contained e8 (A->Anchor), it returned true at depth 2 before pruning.
    // The 6-hop closed walk Anchor->B1->B2->M->N2->A->Anchor is detected under walk semantics.)
    let graph = Graph {
        schema_version: 1,
        release_id: "test-issue6-item1-a".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node {
                id: "Anchor".into(),
            },
            Node { id: "A".into() },
            Node { id: "B1".into() },
            Node { id: "B2".into() },
            Node { id: "M".into() },
            Node { id: "N2".into() },
        ],
        edges: vec![
            mk("e1", "Anchor", "A"),
            mk("e2", "A", "M"),
            mk("e3", "Anchor", "B1"),
            mk("e4", "B1", "B2"),
            mk("e5", "B2", "M"),
            mk("e6", "M", "N2"),
            mk("e7", "N2", "A"),
            mk("e8", "A", "Anchor"),
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: Vec::new(),
    };
    assert!(
        has_non_empty_shutoko_loop(&graph, "Anchor"),
        "closed walk Anchor->B1->B2->M->N2->A->Anchor must be detected"
    );

    // (b) Same last edge, different penultimate edge: the old key (node, last_edge)
    // collapses two arrivals at R whose futures differ because the length-3
    // forbidden transition [u, z, w1] depends on the penultimate edge.
    // Old code: false (second arrival at (R, "z") pruned, first arrival dead-ends).
    // New (node, suffix=[u|v, z]) keys keep them apart: the second arrival closes.
    let graph_b = Graph {
        schema_version: 1,
        release_id: "test-issue6-item1-b".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node {
                id: "Anchor".into(),
            },
            Node { id: "N1".into() },
            Node { id: "N2".into() },
            Node { id: "X".into() },
            Node { id: "R".into() },
            Node { id: "S".into() },
        ],
        edges: vec![
            mk("a", "Anchor", "N1"),
            mk("u", "N1", "X"),
            mk("z", "X", "R"),
            mk("b", "Anchor", "N2"),
            mk("v", "N2", "X"),
            mk("w1", "R", "S"),
            mk("s", "S", "Anchor"),
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["u".into(), "z".into(), "w1".into()]],
    };
    assert!(
        has_non_empty_shutoko_loop(&graph_b, "Anchor"),
        "closed walk via the suffix-distinguished arrival at R must be detected"
    );

    // (c) Walk semantics: a closed walk that revisits A (N2-style revisit is legal
    // for a closed walk) must be detected even though the per-node no-revisit rule
    // blocks it in the old code (old: false because e1 already touches node A).
    let graph_c = Graph {
        schema_version: 1,
        release_id: "test-issue6-item1-c".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node {
                id: "Anchor".into(),
            },
            Node { id: "A".into() },
            Node { id: "B".into() },
        ],
        edges: vec![
            mk("e1", "Anchor", "A"),
            mk("e2", "A", "B"),
            mk("e3", "B", "A"),
            mk("e4", "A", "Anchor"),
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["e1".into(), "e4".into()]],
    };
    assert!(
        has_non_empty_shutoko_loop(&graph_c, "Anchor"),
        "closed walk Anchor->A->B->A->Anchor (revisit of A) must be detected"
    );
}

#[test]
fn test_issue6_item2_simple_path_first_exit() {
    use shutoko_graph_builder::{
        find_first_exits_from_anchor, validate_billing_pair, BillingPair, Edge, EdgeKind, Graph,
        Node, VerificationStatus,
    };

    // Issue #6 item 2 counterexample:
    //   walk  A -e1-> B -e2-> C -e3-> B -exit_walk->  = 4m (loops through B)
    //   simple A -e4-> D -exit_simple->               = 11m
    // Under the old walk semantics the search returned (4, ["exit_walk"]) and the
    // correct simple-path pair for exit_simple was rejected with FIRST_EXIT_MISMATCH.
    // Under simple-path semantics the result is (11, ["exit_simple"]).
    let mk = |id: &str, from: &str, to: &str, kind: EdgeKind| Edge {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        distance_meters: if id == "e4" { 10 } else { 1 },
        duration_seconds: 1,
        kind,
    };
    let graph = Graph {
        schema_version: 1,
        release_id: "test-issue6-item2".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node { id: "entry".into() },
            Node { id: "A".into() },
            Node { id: "B".into() },
            Node { id: "C".into() },
            Node { id: "D".into() },
            Node {
                id: "out_walk".into(),
            },
            Node {
                id: "out_simple".into(),
            },
        ],
        edges: vec![
            mk("entry_e", "entry", "A", EdgeKind::Entry),
            mk("e1", "A", "B", EdgeKind::Shutoko),
            mk("e2", "B", "C", EdgeKind::Shutoko),
            mk("e3", "C", "B", EdgeKind::Shutoko),
            mk("exit_walk", "B", "out_walk", EdgeKind::Exit),
            mk("e4", "A", "D", EdgeKind::Shutoko),
            mk("exit_simple", "D", "out_simple", EdgeKind::Exit),
            mk("loop_back", "D", "A", EdgeKind::Shutoko),
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["e1".into(), "exit_walk".into()]],
    };

    let (min_dist, first_exits) = find_first_exits_from_anchor(&graph, "A").unwrap();
    assert_eq!(
        min_dist, 11,
        "simple-path first exit must be exit_simple at 11m"
    );
    assert_eq!(first_exits, vec!["exit_simple".to_string()]);

    let pair = BillingPair {
        id: "bp-issue6-item2".into(),
        entry_id: "entry_e".into(),
        exit_id: "exit_simple".into(),
        anchor_node_id: "A".into(),
        entry_to_anchor_edge_ids: vec!["entry_e".into()],
        anchor_to_exit_edge_ids: vec!["e4".into(), "exit_simple".into()],
        vehicle_profile: "passenger-car-etc".into(),
        status: VerificationStatus::Verified,
        prices: Vec::new(),
    };
    let ok = validate_billing_pair(&graph, &pair);
    assert!(
        ok.is_ok(),
        "correct simple-path pair must be accepted under simple-path first-exit semantics: {:?}",
        ok.err()
    );
}

#[test]
fn test_issue6_item2_first_exit_search_budget_exceeded() {
    use shutoko_graph_builder::{
        find_first_exits_from_anchor_with_budget, Edge, EdgeKind, Graph, Node,
        FIRST_EXIT_STATE_BUDGET,
    };

    assert_eq!(FIRST_EXIT_STATE_BUDGET, 200_000);

    let mk = |id: &str, from: &str, to: &str, dist: u64, kind: EdgeKind| Edge {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        distance_meters: dist,
        duration_seconds: 1,
        kind,
    };
    // Counterexample A topology (see test_refutation_counterexample_a_opus5).
    let graph = Graph {
        schema_version: 1,
        release_id: "test-issue6-item2-budget".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node { id: "A".into() },
            Node { id: "B".into() },
            Node { id: "C".into() },
            Node { id: "W".into() },
            Node { id: "V".into() },
            Node { id: "X".into() },
            Node { id: "OUT".into() },
        ],
        edges: vec![
            mk("e1", "A", "B", 10, EdgeKind::Shutoko),
            mk("e2", "B", "C", 1, EdgeKind::Shutoko),
            mk("e3", "A", "W", 1, EdgeKind::Shutoko),
            mk("e4", "W", "C", 1, EdgeKind::Shutoko),
            mk("e5", "C", "V", 1, EdgeKind::Shutoko),
            mk("e6", "V", "W", 1, EdgeKind::Shutoko),
            mk("e7", "W", "X", 1, EdgeKind::Shutoko),
            mk("e8", "X", "OUT", 1, EdgeKind::Exit),
        ],
        billing_pairs: Vec::new(),
        forbidden_transitions: vec![vec!["e3".into(), "e7".into()]],
    };

    // Full budget: succeeds with the simple-path answer.
    let (min_dist, first_exits) =
        find_first_exits_from_anchor_with_budget(&graph, "A", FIRST_EXIT_STATE_BUDGET).unwrap();
    assert_eq!(min_dist, 15);
    assert_eq!(first_exits, vec!["e8".to_string()]);

    // Tiny budget: the search must stop with a reported error, never a silent cut.
    let err = find_first_exits_from_anchor_with_budget(&graph, "A", 2)
        .expect_err("budget of 2 must stop the search with Err");
    assert!(
        err.contains("探索予算超過"),
        "budget error must record 探索予算超過: {}",
        err
    );
    assert!(
        err.contains("popped 2 states"),
        "budget error must record the popped state count: {}",
        err
    );
}

#[test]
fn test_issue6_item3_silent_cap_2001_hop_loop_detected() {
    use shutoko_graph_builder::{has_non_empty_shutoko_loop, Edge, EdgeKind, Graph, Node};

    // Serial simple cycle with 2002 Shutoko edges (2002 nodes).
    // Old code: silent `path.len() > 2000` cut -> false.
    // New code: finite (node, suffix) state space, no cap -> true.
    let n = 2002usize;
    let nodes: Vec<Node> = (0..n)
        .map(|i| Node {
            id: format!("n{}", i),
        })
        .collect();
    let edges: Vec<Edge> = (0..n)
        .map(|i| Edge {
            id: format!("e{}", i),
            from: format!("n{}", i),
            to: format!("n{}", (i + 1) % n),
            distance_meters: 1,
            duration_seconds: 1,
            kind: EdgeKind::Shutoko,
        })
        .collect();
    let graph = Graph {
        schema_version: 1,
        release_id: "test-issue6-item3-silent-cap".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes,
        edges,
        billing_pairs: Vec::new(),
        forbidden_transitions: Vec::new(),
    };
    assert!(
        has_non_empty_shutoko_loop(&graph, "n0"),
        "2002-edge closed walk must be detected without the old 2000-hop silent cap"
    );
}

#[test]
fn test_issue6_item4_invalid_dates_and_engine_version() {
    use shutoko_graph_builder::manifest::ManifestConfig;
    use shutoko_graph_builder::{parse_iso_date, parse_utc_timestamp};

    // Non-existent calendar dates must be rejected (time crate semantics).
    assert!(parse_utc_timestamp("2026-02-31T00:00:00Z").is_err());
    assert!(parse_iso_date("2026-02-31").is_err());
    // Sanity: valid dates still parse.
    assert!(parse_utc_timestamp("2026-02-28T00:00:00Z").is_ok());
    assert!(parse_iso_date("2026-02-28").is_ok());

    // manifest.engineVersion must carry the routing-core engine version constant.
    assert_eq!(
        ManifestConfig::default().engine_version,
        shutoko_routing_core::VERSION
    );
}

#[test]
fn test_issue9_billing_pairs_seed_prices_verified_and_output_to_graph() {
    use shutoko_graph_builder::{BillingPairsSeedFile, Graph};

    // 1. Verify that the declarative seed parses and contains the two expected price records
    let seed_str = include_str!("../../../data/billing-pairs-seed.json");
    let seed_file: BillingPairsSeedFile =
        serde_json::from_str(seed_str).expect("data/billing-pairs-seed.json must deserialize");
    assert_eq!(seed_file.billing_pairs.len(), 1);
    let seed_pair = &seed_file.billing_pairs[0];
    assert_eq!(seed_pair.id, "bp:c1-outer:kandabashi-takaracho");
    assert_eq!(
        seed_pair.prices.len(),
        2,
        "seed must have exactly 2 price records"
    );
    assert_eq!(seed_pair.prices[0].amount_yen, 300);
    assert_eq!(seed_pair.prices[0].effective_from, "2022-03-31T15:00:00Z");
    assert_eq!(
        seed_pair.prices[0].effective_to.as_deref(),
        Some("2026-09-30T15:00:00Z")
    );
    assert_eq!(seed_pair.prices[1].amount_yen, 300);
    assert_eq!(seed_pair.prices[1].effective_from, "2026-09-30T15:00:00Z");
    assert_eq!(seed_pair.prices[1].effective_to, None);

    // 2. Verify that the generated graph.json fixture retains both price records
    let graph_str = include_str!("../../../fixtures/generated/graph.json");
    let graph: Graph =
        serde_json::from_str(graph_str).expect("fixtures/generated/graph.json must deserialize");
    assert_eq!(graph.billing_pairs.len(), 1);
    let graph_pair = &graph.billing_pairs[0];
    assert_eq!(graph_pair.id, "bp:c1-outer:kandabashi-takaracho");
    assert_eq!(
        graph_pair.prices.len(),
        2,
        "graph.json billingPairs[0].prices must contain 2 records"
    );
    assert_eq!(graph_pair.prices[0].amount_yen, 300);
    assert_eq!(graph_pair.prices[0].effective_from, "2022-03-31T15:00:00Z");
    assert_eq!(
        graph_pair.prices[0].effective_to.as_deref(),
        Some("2026-09-30T15:00:00Z")
    );
    assert_eq!(graph_pair.prices[1].amount_yen, 300);
    assert_eq!(graph_pair.prices[1].effective_from, "2026-09-30T15:00:00Z");
    assert_eq!(graph_pair.prices[1].effective_to, None);
}
