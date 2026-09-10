use serde_json::{json, Value};
use shutoko_routing_core::search_json;

fn graph() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/synthetic-graph.json")).unwrap()
}

fn request() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/synthetic-request.json")).unwrap()
}

fn run(g: &Value, r: &Value, limits: Value) -> Value {
    serde_json::from_str(&search_json(&g.to_string(), &r.to_string(), &limits.to_string()).unwrap())
        .unwrap()
}

fn candidates(result: &Value) -> &[Value] {
    result["candidates"].as_array().unwrap()
}

#[test]
fn one_loop_then_one_section_exit_accounts_for_every_traversal() {
    let result = run(&graph(), &request(), json!({}));
    assert_eq!(result["status"], "ok");
    assert_eq!(result["requestId"], "synthetic-request");
    assert_eq!(result["releaseId"], "synthetic-v1");
    assert_eq!(candidates(&result).len(), 1);
    let candidate = &candidates(&result)[0];
    assert_eq!(
        candidate["edgeIds"],
        json!(["access", "entry", "ab", "bc", "ca", "exit", "return"])
    );
    assert_eq!(candidate["loop"]["edgeIds"], json!(["ab", "bc", "ca"]));
    assert_eq!(candidate["duration"]["accessSeconds"], 60);
    assert_eq!(candidate["duration"]["shutokoSeconds"], 1860);
    assert_eq!(candidate["duration"]["returnSeconds"], 60);
    assert_eq!(candidate["duration"]["baseSeconds"], 1980);
    assert_eq!(candidate["duration"]["bufferSeconds"], 396);
    assert_eq!(candidate["duration"]["planSeconds"], 2376);
    assert_eq!(candidate["distanceMeters"], 31400);
    assert_eq!(candidate["shutokoDistanceMeters"], 30400);
    assert_eq!(candidate["toll"]["amountYen"], 300);
    assert_eq!(candidate["toll"]["chargedSectionCount"], 1);
}

#[test]
fn direct_exit_is_not_an_empty_loop() {
    let mut g = graph();
    g["edges"]
        .as_array_mut()
        .unwrap()
        .retain(|edge| edge["kind"] != "shutoko");
    let mut r = request();
    r["minMinutes"] = json!(1);
    assert!(candidates(&run(&g, &r, json!({}))).is_empty());
}

#[test]
fn second_lap_cannot_satisfy_a_longer_minimum() {
    let mut r = request();
    r["minMinutes"] = json!(60);
    r["maxMinutes"] = json!(90);
    assert!(candidates(&run(&graph(), &r, json!({}))).is_empty());
}

#[test]
fn checks_forbidden_transitions_across_all_segment_boundaries() {
    for transition in [
        json!(["access", "entry"]),
        json!(["entry", "ab"]),
        json!(["ab", "bc"]),
        json!(["bc", "ca"]),
        json!(["ca", "exit"]),
        json!(["exit", "return"]),
        json!(["bc", "ca", "exit"]),
        json!(["access", "entry", "ab"]),
    ] {
        let mut g = graph();
        g["forbiddenTransitions"] = json!([transition]);
        assert!(
            candidates(&run(&g, &request(), json!({}))).is_empty(),
            "restriction {transition}"
        );
    }
}

#[test]
fn minimum_uses_base_and_maximum_uses_buffered_seconds() {
    let mut r = request();
    r["minMinutes"] = json!(33);
    assert_eq!(candidates(&run(&graph(), &r, json!({}))).len(), 1);
    r["minMinutes"] = json!(34);
    assert!(candidates(&run(&graph(), &r, json!({}))).is_empty());
    r["minMinutes"] = json!(30);
    r["maxMinutes"] = json!(39);
    assert!(candidates(&run(&graph(), &r, json!({}))).is_empty());
}

#[test]
fn maximum_boundary_is_inclusive_and_buffer_rounds_up() {
    let mut g = graph();
    // base=2000 -> buffer=400 -> plan=2400, exactly forty minutes.
    g["edges"][0]["durationSeconds"] = json!(80);
    assert_eq!(candidates(&run(&g, &request(), json!({}))).len(), 1);
    g["edges"][0]["durationSeconds"] = json!(81);
    assert!(candidates(&run(&g, &request(), json!({}))).is_empty());
}

#[test]
fn pricing_intervals_are_half_open_and_outside_is_unknown() {
    let mut g = graph();
    g["billingPairs"][0]["prices"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "amountYen":400,"effectiveFrom":"2026-10-01T00:00:00Z"
        }));
    for (timestamp, amount) in [
        ("2025-12-31T23:59:59Z", Value::Null),
        ("2026-01-01T00:00:00Z", json!(300)),
        ("2026-09-30T23:59:59Z", json!(300)),
        ("2026-10-01T00:00:00Z", json!(400)),
    ] {
        let mut r = request();
        r["pricingAt"] = json!(timestamp);
        assert_eq!(
            candidates(&run(&g, &r, json!({})))[0]["toll"]["amountYen"],
            amount
        );
    }
}

#[test]
fn unverified_billing_pair_is_not_searchable() {
    let mut g = graph();
    g["billingPairs"][0]["status"] = json!("unverified");
    assert!(candidates(&run(&g, &request(), json!({}))).is_empty());
}

#[test]
fn rejects_graph_corruption_and_overlapping_prices() {
    let mut cases = Vec::new();
    let mut g = graph();
    g["edges"][0]["to"] = json!("missing");
    cases.push(g);
    let mut g = graph();
    g["edges"][1]["id"] = json!("access");
    cases.push(g);
    let mut g = graph();
    g["billingPairs"][0]["prices"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "amountYen":400,"effectiveFrom":"2026-09-01T00:00:00Z"
        }));
    cases.push(g);
    let mut g = graph();
    g["billingPairs"][0]["prices"][0]["amountYen"] = json!(0);
    cases.push(g);
    for g in cases {
        assert!(search_json(&g.to_string(), &request().to_string(), "{}").is_err());
    }
}

#[test]
fn rejects_hidden_extra_laps_inside_fixed_connection_segments() {
    for field in ["entryToAnchorEdgeIds", "anchorToExitEdgeIds"] {
        let mut g = graph();
        g["billingPairs"][0][field] = if field == "entryToAnchorEdgeIds" {
            json!(["entry", "ab", "bc", "ca"])
        } else {
            json!(["ab", "bc", "ca", "exit"])
        };
        let mut r = request();
        r["minMinutes"] = json!(60);
        r["maxMinutes"] = json!(90);
        assert!(
            search_json(&g.to_string(), &r.to_string(), "{}").is_err(),
            "{field}"
        );
    }
}

#[test]
fn rejects_a_hidden_lap_spanning_both_fixed_connection_segments() {
    let mut g = graph();
    g["edges"][1]["to"] = json!("b");
    g["edges"][5]["from"] = json!("b");
    g["billingPairs"][0]["entryToAnchorEdgeIds"] = json!(["entry", "bc", "ca"]);
    g["billingPairs"][0]["anchorToExitEdgeIds"] = json!(["ab", "exit"]);
    let mut r = request();
    r["minMinutes"] = json!(60);
    r["maxMinutes"] = json!(90);
    assert!(search_json(&g.to_string(), &r.to_string(), "{}").is_err());
}

#[test]
fn billing_identity_must_match_the_actual_entry_and_exit_edges() {
    for field in ["entryId", "exitId"] {
        let mut g = graph();
        g["billingPairs"][0][field] = json!("wrong-ramp");
        assert!(
            search_json(&g.to_string(), &request().to_string(), "{}").is_err(),
            "{field}"
        );
    }
}

#[test]
fn rejects_time_window_beyond_the_four_hour_product_limit() {
    let mut r = request();
    r["minMinutes"] = json!(1);
    r["maxMinutes"] = json!(241);
    assert!(search_json(&graph().to_string(), &r.to_string(), "{}").is_err());
}

#[test]
fn rejects_invalid_request_and_zero_limits() {
    for (field, value) in [
        ("releaseId", json!("wrong")),
        ("vehicleProfile", json!("wrong")),
        ("pricingAt", json!("invalid-date")),
        ("minMinutes", json!(41)),
        ("minMinutes", json!(-1)),
        ("minMinutes", json!(0)),
        ("maxMinutes", json!(0.5)),
    ] {
        let mut r = request();
        r[field] = value;
        assert!(
            search_json(&graph().to_string(), &r.to_string(), "{}").is_err(),
            "{field}"
        );
    }
    for limit in [
        "maxExpandedStates",
        "beamWidth",
        "maxLoopEdges",
        "maxLocalEdges",
        "maxPairs",
        "maxCandidates",
    ] {
        let mut limits = json!({});
        limits[limit] = json!(0);
        assert!(
            search_json(
                &graph().to_string(),
                &request().to_string(),
                &limits.to_string()
            )
            .is_err(),
            "{limit}"
        );
    }
}

#[test]
fn repeated_searches_are_deterministic_and_exhaustion_is_explicit() {
    let first = run(&graph(), &request(), json!({}));
    assert_eq!(first, run(&graph(), &request(), json!({})));
    let limited = run(&graph(), &request(), json!({"maxExpandedStates":1}));
    assert_eq!(limited["status"], "truncated");
    assert!(candidates(&limited).is_empty());
    assert_eq!(
        limited,
        run(&graph(), &request(), json!({"maxExpandedStates":1}))
    );
}

#[test]
fn connection_segments_may_share_edges_with_the_loop() {
    let mut g = graph();
    g["edges"][1]["to"] = json!("b");
    g["billingPairs"][0]["entryToAnchorEdgeIds"] = json!(["entry", "bc", "ca"]);
    let mut r = request();
    r["maxMinutes"] = json!(70);
    let result = run(&g, &r, json!({}));
    assert_eq!(candidates(&result).len(), 1);
    let c = &candidates(&result)[0];
    assert_eq!(
        c["edgeIds"],
        json!(["access", "entry", "bc", "ca", "ab", "bc", "ca", "exit", "return"])
    );
    assert_eq!(c["duration"]["baseSeconds"], 3180);
    assert_eq!(c["shutokoDistanceMeters"], 50400);
}

// Independent exhaustive DFS oracle for tiny directed graphs, with no beam or time pruning.
fn enumerate_cycles(
    edges: &[Value],
    at: &str,
    visited: &mut Vec<String>,
    path: &mut Vec<String>,
    result: &mut Vec<Vec<String>>,
) {
    for edge in edges
        .iter()
        .filter(|e| e["kind"] == "shutoko" && e["from"] == at)
    {
        let to = edge["to"].as_str().unwrap();
        path.push(edge["id"].as_str().unwrap().to_string());
        if to == "a" {
            result.push(path.clone());
        } else if !visited.iter().any(|v| v == to) {
            visited.push(to.to_string());
            enumerate_cycles(edges, to, visited, path, result);
            visited.pop();
        }
        path.pop();
    }
}

#[test]
fn untruncated_search_matches_all_simple_cycles_in_a_tiny_graph() {
    let mut g = graph();
    for (id, from, to) in [("ba", "b", "a"), ("ac", "a", "c")] {
        g["edges"].as_array_mut().unwrap().push(json!({
            "id":id,"from":from,"to":to,"kind":"shutoko","durationSeconds":600,"distanceMeters":10000
        }));
    }
    let mut expected = Vec::new();
    enumerate_cycles(
        g["edges"].as_array().unwrap(),
        "a",
        &mut vec!["a".into()],
        &mut Vec::new(),
        &mut expected,
    );
    expected.sort();
    assert_eq!(expected.len(), 3);
    let mut r = request();
    r["minMinutes"] = json!(1);
    let result = run(&g, &r, json!({}));
    assert_eq!(result["status"], "ok");
    let mut actual: Vec<Vec<String>> = candidates(&result)
        .iter()
        .map(|c| serde_json::from_value(c["loop"]["edgeIds"].clone()).unwrap())
        .collect();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(
        candidates(&result)[0]["loop"]["edgeIds"],
        json!(["ab", "bc", "ca"])
    );
    g["edges"].as_array_mut().unwrap().reverse();
    assert_eq!(result, run(&g, &r, json!({})));
}

// A second independent road network shares only the origin, so diversity filtering
// cannot hide ranking mistakes between the two billing pairs.
fn add_independent_pair(g: &mut Value, loop_edge_seconds: u64, amount: Option<u64>) {
    let original = graph();
    let rename = |id: &str| {
        if id == "s" {
            id.to_owned()
        } else {
            format!("second-{id}")
        }
    };
    for node in original["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["id"] != "s")
    {
        g["nodes"].as_array_mut().unwrap().push(json!({
            "id": rename(node["id"].as_str().unwrap()),
            "lat": node["lat"],
            "lon": node["lon"],
        }));
    }
    for edge in original["edges"].as_array().unwrap() {
        let mut edge = edge.clone();
        for field in ["id", "from", "to"] {
            edge[field] = json!(rename(edge[field].as_str().unwrap()));
        }
        if edge["kind"] == "shutoko" {
            edge["durationSeconds"] = json!(loop_edge_seconds);
        }
        g["edges"].as_array_mut().unwrap().push(edge);
    }
    let mut pair = original["billingPairs"][0].clone();
    for field in ["id", "entryId", "exitId", "anchorNodeId"] {
        pair[field] = json!(rename(pair[field].as_str().unwrap()));
    }
    pair["entryToAnchorEdgeIds"] = json!(["second-entry"]);
    pair["anchorToExitEdgeIds"] = json!(["second-exit"]);
    pair["prices"] = match amount {
        Some(amount) => json!([{"amountYen":amount,"effectiveFrom":"2026-01-01T00:00:00Z"}]),
        None => json!([]),
    };
    g["billingPairs"].as_array_mut().unwrap().push(pair);
}

#[test]
fn known_prices_rank_by_time_per_yen_instead_of_duration_alone() {
    for (seconds, price, first_pair) in [
        (600, 200, "second-one-section"), // Equal duration: cheaper pair wins.
        (700, 600, "one-section"),        // Longer drive loses on time per yen.
        (500, 200, "second-one-section"), // Shorter drive wins on time per yen.
    ] {
        let mut g = graph();
        add_independent_pair(&mut g, seconds, Some(price));
        let mut r = request();
        r["minMinutes"] = json!(1);
        r["maxMinutes"] = json!(60);
        let result = run(&g, &r, json!({}));
        assert_eq!(result["rankingMode"], "time_per_yen");
        assert_eq!(candidates(&result).len(), 2);
        assert_eq!(candidates(&result)[0]["toll"]["billingPairId"], first_pair);
    }
}

#[test]
fn any_unknown_price_switches_the_whole_candidate_set_to_duration_ranking() {
    for (seconds, first_pair) in [(700, "second-one-section"), (500, "one-section")] {
        let mut g = graph();
        add_independent_pair(&mut g, seconds, None);
        let mut r = request();
        r["minMinutes"] = json!(1);
        r["maxMinutes"] = json!(60);
        let result = run(&g, &r, json!({}));
        assert_eq!(result["rankingMode"], "shutoko_time");
        assert_eq!(candidates(&result).len(), 2);
        assert_eq!(candidates(&result)[0]["toll"]["billingPairId"], first_pair);
        assert!(candidates(&result)
            .iter()
            .any(|c| c["toll"]["amountYen"].is_null()));
    }
}

#[test]
fn identical_highway_routes_are_not_padded_into_multiple_choices() {
    let mut g = graph();
    let mut pair = g["billingPairs"][0].clone();
    pair["id"] = json!("same-route-cheaper");
    pair["prices"][0]["amountYen"] = json!(200);
    g["billingPairs"].as_array_mut().unwrap().push(pair);
    let result = run(&g, &request(), json!({}));
    assert_eq!(result["status"], "ok");
    assert_eq!(candidates(&result).len(), 1);
    assert_eq!(
        candidates(&result)[0]["toll"]["billingPairId"],
        "same-route-cheaper"
    );
}

#[test]
fn distance_weighted_similarity_excludes_at_exactly_eighty_percent() {
    for (alternate_distance, expected_count) in [(5000, 1), (5001, 2)] {
        let mut g = graph();
        g["edges"][3]["distanceMeters"] = json!(100);
        let mut alternate = g["edges"][3].clone();
        alternate["id"] = json!("bc-alternate");
        alternate["distanceMeters"] = json!(alternate_distance);
        g["edges"].as_array_mut().unwrap().push(alternate);
        // Shared distance = entry + ab + ca + exit = 20400 m.
        // Union = 20400 + 100 + 5000 = 25500 m: exactly 80%.
        let result = run(&g, &request(), json!({}));
        assert_eq!(result["status"], "ok");
        assert_eq!(candidates(&result).len(), expected_count);
    }
}

#[test]
fn retained_successful_paths_limit_preserves_valid_candidates_and_reports_truncation() {
    let mut g = graph();
    // Both self loops complete immediately; neither needs a frontier slot. The
    // successful-path storage cap itself must report the discarded second result.
    for id in ["a-loop-1", "a-loop-2"] {
        g["edges"].as_array_mut().unwrap().push(json!({
            "id":id,"from":"a","to":"a","kind":"shutoko","durationSeconds":1800,"distanceMeters":30000
        }));
    }
    let result = run(&g, &request(), json!({"beamWidth":1}));
    assert_eq!(result["status"], "truncated");
    assert_eq!(result["reason"], "SEARCH_LIMIT");
    assert_eq!(candidates(&result).len(), 1);
    assert_eq!(
        candidates(&result)[0]["loop"]["edgeIds"],
        json!(["a-loop-1"])
    );
    assert_eq!(candidates(&result)[0]["duration"]["planSeconds"], 2376);
}

#[test]
fn global_candidate_storage_limit_reports_truncation() {
    let mut g = graph();
    add_independent_pair(&mut g, 600, Some(200));
    let result = run(&g, &request(), json!({"beamWidth":1}));
    assert_eq!(result["status"], "truncated");
    assert_eq!(result["reason"], "SEARCH_LIMIT");
    assert_eq!(candidates(&result).len(), 1);
    assert_eq!(candidates(&result)[0]["loop"]["validated"], true);
}

#[test]
fn identifiers_enforce_the_256_byte_boundary() {
    for length in [256, 257] {
        let id = "x".repeat(length);
        let mut r = request();
        r["requestId"] = json!(id);
        assert_eq!(
            search_json(&graph().to_string(), &r.to_string(), "{}").is_ok(),
            length == 256
        );
        let mut g = graph();
        g["billingPairs"][0]["id"] = json!(id);
        assert_eq!(
            search_json(&g.to_string(), &request().to_string(), "{}").is_ok(),
            length == 256
        );
        let mut g = graph();
        g["nodes"].as_array_mut().unwrap().push(json!({
            "id": id,
            "lat": 35.68,
            "lon": 139.76,
        }));
        assert_eq!(
            search_json(&g.to_string(), &request().to_string(), "{}").is_ok(),
            length == 256
        );
        let mut g = graph();
        g["edges"][0]["id"] = json!(id);
        assert_eq!(
            search_json(&g.to_string(), &request().to_string(), "{}").is_ok(),
            length == 256
        );
    }
    let mut r = request();
    r["requestId"] = json!("あ".repeat(86));
    assert!(search_json(&graph().to_string(), &r.to_string(), "{}").is_err());
}

#[test]
fn release_and_vehicle_profile_enforce_the_256_byte_boundary() {
    for field in ["releaseId", "vehicleProfile"] {
        for length in [256, 257] {
            let mut g = graph();
            let mut r = request();
            let value = json!("x".repeat(length));
            g[field] = value.clone();
            r[field] = value.clone();
            if field == "vehicleProfile" {
                g["billingPairs"][0][field] = value;
            }
            assert_eq!(
                search_json(&g.to_string(), &r.to_string(), "{}").is_ok(),
                length == 256,
                "{field}"
            );
        }
    }
}

#[test]
fn timestamp_length_cap_rejects_oversized_fractional_seconds() {
    for fractional_digits in [21, 100_000] {
        let timestamp = format!("2026-01-01T00:00:00.{}Z", "0".repeat(fractional_digits));
        assert!(timestamp.len() > 40);
        for field in ["effectiveFrom", "effectiveTo"] {
            let mut g = graph();
            // Keep the interval otherwise valid so the end-date check cannot
            // accidentally pass due to an inverted interval.
            g["billingPairs"][0]["prices"][0][field] = json!(if field == "effectiveTo" {
                timestamp.replace("2026", "2030")
            } else {
                timestamp.clone()
            });
            assert!(
                search_json(&g.to_string(), &request().to_string(), "{}").is_err(),
                "{field}"
            );
        }
        let mut r = request();
        r["pricingAt"] = json!(timestamp);
        assert!(search_json(&graph().to_string(), &r.to_string(), "{}").is_err());
    }
}

fn edge_json(
    id: &str,
    from: &str,
    to: &str,
    kind: &str,
    duration_seconds: u64,
    distance_meters: u64,
) -> Value {
    json!({
        "id": id,
        "from": from,
        "to": to,
        "kind": kind,
        "durationSeconds": duration_seconds,
        "distanceMeters": distance_meters,
    })
}

fn standard_highway_edges() -> Vec<Value> {
    vec![
        edge_json("entry", "i", "a", "entry", 30, 200),
        edge_json("ab", "a", "b", "shutoko", 600, 10000),
        edge_json("bc", "b", "c", "shutoko", 600, 10000),
        edge_json("ca", "c", "a", "shutoko", 600, 10000),
        edge_json("exit", "a", "o", "exit", 30, 200),
        edge_json("return", "o", "s", "local", 60, 500),
    ]
}

fn build_test_graph(nodes: &[&str], edges: Vec<Value>, forbidden_transitions: Value) -> Value {
    json!({
        "schemaVersion": 1,
        "releaseId": "synthetic-v1",
        "vehicleProfile": "passenger-car-etc",
        "nodes": nodes.iter().map(|n| json!({"id": n, "lat": 35.68, "lon": 139.76})).collect::<Vec<_>>(),
        "edges": edges,
        "billingPairs": [{
            "id": "one-section",
            "entryId": "entry",
            "exitId": "exit",
            "anchorNodeId": "a",
            "entryToAnchorEdgeIds": ["entry"],
            "anchorToExitEdgeIds": ["exit"],
            "status": "verified",
            "vehicleProfile": "passenger-car-etc",
            "prices": [{
                "amountYen": 300,
                "effectiveFrom": "2026-01-01T00:00:00Z",
                "effectiveTo": "2026-10-01T00:00:00Z"
            }]
        }],
        "forbiddenTransitions": forbidden_transitions
    })
}

fn standard_test_request(origin: &str) -> Value {
    json!({
        "requestId": "test-req",
        "releaseId": "synthetic-v1",
        "originNodeId": origin,
        "minMinutes": 30,
        "maxMinutes": 45,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    })
}

/// (a) forward: forbiddenTransitions=[["l2","entry"]] で合法迂回 l3,l4 があるとき status: ok で迂回路が選ばれる
#[test]
fn forbidden_transitions_forward_junction_detour_selected() {
    let mut edges = vec![
        edge_json("l1", "s", "p", "local", 60, 500),
        edge_json("l2", "p", "i", "local", 60, 500),
        edge_json("l3", "s", "q", "local", 90, 700),
        edge_json("l4", "q", "i", "local", 90, 700),
    ];
    edges.extend(standard_highway_edges());
    let g = build_test_graph(
        &["s", "p", "q", "i", "a", "b", "c", "o"],
        edges,
        json!([["l2", "entry"]]),
    );
    let result = run(&g, &standard_test_request("s"), json!({}));
    assert_eq!(result["status"], "ok");
    let c = candidates(&result);
    assert_eq!(c.len(), 1);
    let ids: Vec<&str> = c[0]["edgeIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids.starts_with(&["l3", "l4", "entry"]),
        "must select detour l3,l4, got: {:?}",
        ids
    );
}

/// (b) backward: [["exit","r1"]]（または [["exit","l9"]] 相当）で合法迂回 r3,r4 があるとき選ばれる
#[test]
fn forbidden_transitions_backward_junction_detour_selected() {
    let nodes = ["s", "p", "i", "a", "b", "c", "o", "o2", "q"];
    let edges = vec![
        edge_json("l1", "s", "p", "local", 60, 500),
        edge_json("l2", "p", "i", "local", 60, 500),
        edge_json("entry", "i", "a", "entry", 30, 200),
        edge_json("ab", "a", "b", "shutoko", 600, 10000),
        edge_json("bc", "b", "c", "shutoko", 600, 10000),
        edge_json("ca", "c", "a", "shutoko", 600, 10000),
        edge_json("exit", "a", "o", "exit", 30, 200),
        edge_json("r1", "o", "o2", "local", 60, 500),
        edge_json("r2", "o2", "s", "local", 60, 500),
        edge_json("r3", "o", "q", "local", 90, 700),
        edge_json("r4", "q", "s", "local", 90, 700),
    ];
    let g = build_test_graph(&nodes, edges, json!([["exit", "r1"]]));
    let result = run(&g, &standard_test_request("s"), json!({}));
    assert_eq!(result["status"], "ok");
    let c = candidates(&result);
    assert_eq!(c.len(), 1);
    let ids: Vec<&str> = c[0]["edgeIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids.ends_with(&["exit", "r3", "r4"]),
        "must select return detour r3,r4, got: {:?}",
        ids
    );
}

/// (c) 長さ 3: s→p が l1(60s) / l1b(70s) の 2 本、p→m=l2、m→i=l5、[["l1","l2","l5"]] で唯一の合法路 l1b,l2,l5 が選ばれる
#[test]
fn forbidden_transitions_length_3_purely_local_preserves_legal_history() {
    let mut edges = vec![
        edge_json("l1", "s", "p", "local", 60, 500),
        edge_json("l1b", "s", "p", "local", 70, 600),
        edge_json("l2", "p", "m", "local", 60, 500),
        edge_json("l5", "m", "i", "local", 60, 500),
    ];
    edges.extend(standard_highway_edges());
    let g = build_test_graph(
        &["s", "p", "m", "i", "a", "b", "c", "o"],
        edges,
        json!([["l1", "l2", "l5"]]),
    );
    let result = run(&g, &standard_test_request("s"), json!({}));
    assert_eq!(result["status"], "ok");
    let c = candidates(&result);
    assert_eq!(c.len(), 1);
    let ids: Vec<&str> = c[0]["edgeIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids.starts_with(&["l1b", "l2", "l5", "entry"]),
        "must select legal history l1b,l2,l5, got: {:?}",
        ids
    );
}

/// (d) 対照として長さ 2 [["l1","l2"]] でも同じ結果
#[test]
fn forbidden_transitions_length_2_control_matches_length_3_result() {
    let mut edges = vec![
        edge_json("l1", "s", "p", "local", 60, 500),
        edge_json("l1b", "s", "p", "local", 70, 600),
        edge_json("l2", "p", "m", "local", 60, 500),
        edge_json("l5", "m", "i", "local", 60, 500),
    ];
    edges.extend(standard_highway_edges());
    let g = build_test_graph(
        &["s", "p", "m", "i", "a", "b", "c", "o"],
        edges,
        json!([["l1", "l2"]]),
    );
    let result = run(&g, &standard_test_request("s"), json!({}));
    assert_eq!(result["status"], "ok");
    let c = candidates(&result);
    assert_eq!(c.len(), 1);
    let ids: Vec<&str> = c[0]["edgeIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids.starts_with(&["l1b", "l2", "l5", "entry"]),
        "must select legal history l1b,l2,l5, got: {:?}",
        ids
    );
}

/// (e) local→local 長さ 2 の forward / backward 迂回
#[test]
fn forbidden_transitions_local_to_local_len_2_forward_and_backward_detours() {
    // (e1) Forward local->local detour
    let mut edges_fwd = vec![
        edge_json("l1", "s", "p", "local", 60, 500),
        edge_json("l2", "p", "i", "local", 60, 500),
        edge_json("l3", "s", "q", "local", 90, 700),
        edge_json("l4", "q", "i", "local", 90, 700),
    ];
    edges_fwd.extend(standard_highway_edges());
    let g_fwd = build_test_graph(
        &["s", "p", "q", "i", "a", "b", "c", "o"],
        edges_fwd,
        json!([["l1", "l2"]]),
    );
    let result_fwd = run(&g_fwd, &standard_test_request("s"), json!({}));
    assert_eq!(result_fwd["status"], "ok");
    let c_fwd = candidates(&result_fwd);
    assert_eq!(c_fwd.len(), 1);
    let ids_fwd: Vec<&str> = c_fwd[0]["edgeIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids_fwd.starts_with(&["l3", "l4", "entry"]),
        "forward detour must be l3,l4, got: {:?}",
        ids_fwd
    );

    // (e2) Backward local->local detour
    let nodes_bwd = ["s", "p", "i", "a", "b", "c", "o", "o2", "q"];
    let edges_bwd = vec![
        edge_json("l1", "s", "p", "local", 60, 500),
        edge_json("l2", "p", "i", "local", 60, 500),
        edge_json("entry", "i", "a", "entry", 30, 200),
        edge_json("ab", "a", "b", "shutoko", 600, 10000),
        edge_json("bc", "b", "c", "shutoko", 600, 10000),
        edge_json("ca", "c", "a", "shutoko", 600, 10000),
        edge_json("exit", "a", "o", "exit", 30, 200),
        edge_json("r1", "o", "o2", "local", 60, 500),
        edge_json("r2", "o2", "s", "local", 60, 500),
        edge_json("r3", "o", "q", "local", 90, 700),
        edge_json("r4", "q", "s", "local", 90, 700),
    ];
    let g_bwd = build_test_graph(&nodes_bwd, edges_bwd, json!([["r1", "r2"]]));
    let result_bwd = run(&g_bwd, &standard_test_request("s"), json!({}));
    assert_eq!(result_bwd["status"], "ok");
    let c_bwd = candidates(&result_bwd);
    assert_eq!(c_bwd.len(), 1);
    let ids_bwd: Vec<&str> = c_bwd[0]["edgeIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids_bwd.ends_with(&["exit", "r3", "r4"]),
        "backward detour must be r3,r4, got: {:?}",
        ids_bwd
    );
}

#[test]
fn coordinate_input_snaps_to_nearest_local_node() {
    let mut r = request();
    r.as_object_mut().unwrap().remove("originNodeId");
    r["origin"] = json!({ "lat": 35.681, "lon": 139.7671 });
    let result = run(&graph(), &r, json!({}));
    assert_eq!(result["status"], "ok");
    let c = candidates(&result);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["snappedOrigin"]["nodeId"], "s");
    assert!(c[0]["snappedOrigin"]["distanceMeters"].as_f64().unwrap() < 1.0);
    assert_eq!(c[0]["origin"], json!({ "lat": 35.681, "lon": 139.7671 }));
}

#[test]
fn coordinate_input_beyond_200m_returns_no_connection() {
    let mut r = request();
    r.as_object_mut().unwrap().remove("originNodeId");
    r["origin"] = json!({ "lat": 35.0, "lon": 139.0 });
    let result = run(&graph(), &r, json!({}));
    assert_eq!(result["status"], "no_candidates");
    assert_eq!(result["reason"], "NO_CONNECTION");
    assert!(candidates(&result).is_empty());
}

#[test]
fn search_request_both_origins_or_neither_returns_invalid_input() {
    // Both specified
    let mut r_both = request();
    r_both["origin"] = json!({ "lat": 35.681, "lon": 139.7671 });
    let err_both = search_json(&graph().to_string(), &r_both.to_string(), "{}").unwrap_err();
    assert_eq!(err_both.code, "INVALID_INPUT");

    // Neither specified
    let mut r_neither = request();
    r_neither.as_object_mut().unwrap().remove("originNodeId");
    let err_neither = search_json(&graph().to_string(), &r_neither.to_string(), "{}").unwrap_err();
    assert_eq!(err_neither.code, "INVALID_INPUT");
}

#[test]
fn coordinate_input_out_of_bounds_or_non_finite_returns_invalid_input() {
    for (lat, lon) in [(91.0, 139.0), (-91.0, 139.0), (35.0, 181.0), (35.0, -181.0)] {
        let mut r = request();
        r.as_object_mut().unwrap().remove("originNodeId");
        r["origin"] = json!({ "lat": lat, "lon": lon });
        let err = search_json(&graph().to_string(), &r.to_string(), "{}").unwrap_err();
        assert_eq!(err.code, "INVALID_INPUT");
    }

    let mut r_nan = request();
    r_nan.as_object_mut().unwrap().remove("originNodeId");
    let invalid_json = r_nan.to_string().replace(
        "\"pricingAt\"",
        "\"origin\":{\"lat\":\"NaN\",\"lon\":139.0},\"pricingAt\"",
    );
    assert!(search_json(&graph().to_string(), &invalid_json, "{}").is_err());
}

#[test]
fn candidate_geometry_and_handoff_and_warnings_contract() {
    let result = run(&graph(), &request(), json!({}));
    assert_eq!(result["status"], "ok");
    let c = &candidates(&result)[0];

    // Geometry validation
    assert_eq!(c["geometry"]["type"], "LineString");
    let coords = c["geometry"]["coordinates"].as_array().unwrap();
    let edge_ids = c["edgeIds"].as_array().unwrap();
    assert_eq!(coords.len(), edge_ids.len() + 1);
    // First coordinate corresponds to node "s" [lon, lat]
    assert_eq!(coords.first().unwrap(), &json!([139.7671, 35.681]));
    // Last coordinate corresponds to node "s" [lon, lat]
    assert_eq!(coords.last().unwrap(), &json!([139.7671, 35.681]));

    // Handoff Maps URL validation
    let maps_url = c["handoff"]["mapsUrl"].as_str().unwrap();
    assert!(
        maps_url.starts_with("https://www.google.com/maps/dir/?api=1&origin="),
        "Maps URL must begin with Google Maps directions API base"
    );
    assert!(
        maps_url.len() <= 2048,
        "Maps URL must not exceed 2048 chars, got {}",
        maps_url.len()
    );
    let waypoints = c["handoff"]["waypoints"].as_array().unwrap();
    assert!(waypoints.len() <= 3, "Waypoints must be <= 3");

    // Warnings validation
    let warnings: Vec<&str> = c["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        warnings.contains(&"HANDOFF_WAYPOINTS_UNVERIFIED"),
        "warnings must contain HANDOFF_WAYPOINTS_UNVERIFIED"
    );
}

#[test]
fn road_names_ordered_and_deduplicated() {
    let mut g = graph();
    for edge in g["edges"].as_array_mut().unwrap() {
        let id = edge["id"].as_str().unwrap();
        match id {
            "ab" | "bc" => edge["name"] = json!("都心環状線"),
            "ca" => edge["name"] = json!("八重洲線"),
            _ => {}
        }
    }
    let result = run(&g, &request(), json!({}));
    assert_eq!(result["status"], "ok");
    let c = &candidates(&result)[0];
    let road_names: Vec<&str> = c["roadNames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(road_names, vec!["都心環状線", "八重洲線"]);
}
