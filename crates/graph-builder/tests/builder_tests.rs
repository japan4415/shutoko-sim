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
