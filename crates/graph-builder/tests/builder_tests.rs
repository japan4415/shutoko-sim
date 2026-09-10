use serde_json::json;
use shutoko_graph_builder::{
    build_topology, haversine_distance_meters, to_deterministic_json, EdgeKind, OverpassResponse,
    TopologyConfig, LOCAL_SPEED_KMH, RAMP_SPEED_KMH, SHUTOKO_SPEED_KMH,
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
    assert_eq!(edge_kinds.get("e:w3:1:f"), Some(&EdgeKind::Entry));

    // Exit ramp segments
    assert_eq!(edge_kinds.get("e:w4:0:f"), Some(&EdgeKind::Exit));
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
            source: "test".into(),
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
            source: "test".into(),
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
