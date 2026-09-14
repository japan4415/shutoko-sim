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
        json!(["entry", "ab", "bc", "ca", "exit"])
    );
    assert_eq!(candidate["loop"]["edgeIds"], json!(["ab", "bc", "ca"]));
    // originNodeId="i" is the Entry from-node: access distance = 0, access_secs = 0.
    assert_eq!(candidate["duration"]["accessSeconds"], 0);
    assert_eq!(candidate["duration"]["shutokoSeconds"], 1860);
    // return: ceil(dist(o→i) * 1.3 / (30/3.6)) = ceil(96.4 * 1.3 / 8.333) = 16
    assert_eq!(candidate["duration"]["returnSeconds"], 16);
    assert_eq!(candidate["duration"]["baseSeconds"], 1876);
    assert_eq!(candidate["duration"]["bufferSeconds"], 376);
    assert_eq!(candidate["duration"]["planSeconds"], 2252);
    assert_eq!(candidate["distanceMeters"], 30400);
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
    // Only transitions between edges that still exist in the graph (no local edges).
    for transition in [
        json!(["entry", "ab"]),
        json!(["ab", "bc"]),
        json!(["bc", "ca"]),
        json!(["ca", "exit"]),
        json!(["bc", "ca", "exit"]),
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
    // base = access(0) + shutoko(1860) + return(16) = 1876 s = 31.27 min
    // buffer = max(300, ceil(1876/5)) = 376 s; plan = 2252 s = 37.53 min
    let mut r = request();
    r["minMinutes"] = json!(31); // 31*60=1860 ≤ 1876 → passes
    assert_eq!(candidates(&run(&graph(), &r, json!({}))).len(), 1);
    r["minMinutes"] = json!(32); // 32*60=1920 > 1876 → rejected
    assert!(candidates(&run(&graph(), &r, json!({}))).is_empty());
    r["minMinutes"] = json!(30);
    r["maxMinutes"] = json!(37); // plan=2252 > 37*60=2220 → rejected
    assert!(candidates(&run(&graph(), &r, json!({}))).is_empty());
}

#[test]
fn maximum_boundary_is_inclusive_and_buffer_rounds_up() {
    let mut g = graph();
    // base = 0 + (X + 600+600+600+30) + 16 = X + 1846
    // With X=154: base=2000 -> buffer=400 -> plan=2400, exactly forty minutes.
    // With X=155: base=2001 -> buffer=401 -> plan=2402 > 2400 → rejected.
    g["edges"][0]["durationSeconds"] = json!(154);
    assert_eq!(candidates(&run(&g, &request(), json!({}))).len(), 1);
    g["edges"][0]["durationSeconds"] = json!(155);
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
    // Create a duplicate edge ID by giving "ab" the same ID as "entry".
    g["edges"][1]["id"] = json!("entry");
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
    // Change edges[0] (entry i→a) to go i→b, and edges[4] (exit a→o) to go b→o.
    // Direct path: i→b→c→a→b→o — node "b" appears twice → hidden lap.
    let mut g = graph();
    g["edges"][0]["to"] = json!("b");
    g["edges"][4]["from"] = json!("b");
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
        "maxAccessEntries",
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
    // Change edges[0] (entry i→a) to go i→b so the entry path is [entry(i→b), bc, ca].
    // The loop from anchor "a" is [ab, bc, ca], sharing bc and ca with the entry path.
    let mut g = graph();
    g["edges"][0]["to"] = json!("b");
    g["billingPairs"][0]["entryToAnchorEdgeIds"] = json!(["entry", "bc", "ca"]);
    let mut r = request();
    r["maxMinutes"] = json!(70);
    let result = run(&g, &r, json!({}));
    assert_eq!(candidates(&result).len(), 1);
    let c = &candidates(&result)[0];
    assert_eq!(
        c["edgeIds"],
        json!(["entry", "bc", "ca", "ab", "bc", "ca", "exit"])
    );
    // base = 0 + (30+600+600+600+600+600+30) + 16 = 3076
    assert_eq!(c["duration"]["baseSeconds"], 3076);
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

// A second independent road network shares only the Entry from-node "i" (the common
// access point), so diversity filtering cannot hide ranking mistakes between the two
// billing pairs.  Both billing pairs are accessible from originNodeId="i".
fn add_independent_pair(g: &mut Value, loop_edge_seconds: u64, amount: Option<u64>) {
    let original = graph();
    let rename = |id: &str| {
        if id == "i" {
            id.to_owned()
        } else {
            format!("second-{id}")
        }
    };
    for node in original["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["id"] != "i")
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
    // base = 0 + (30+1800+30) + 16 = 1876; buffer=376; plan=2252
    assert_eq!(candidates(&result)[0]["duration"]["planSeconds"], 2252);
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
        // Use edges[1] ("ab") which is not referenced by billing pair paths,
        // so changing its ID only tests the ID length limit.
        let mut g = graph();
        g["edges"][1]["id"] = json!(id);
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

/// Coordinate input snaps to the nearest Entry from-node in the snap grid.
/// Node "i" (lat:35.6815, lon:139.7675) is the sole Entry from-node in the
/// synthetic graph.  A query at (35.6815, 139.7675) should snap to "i" with
/// distance ≈ 0 m.
#[test]
fn coordinate_input_snaps_to_nearest_entry_node() {
    let mut r = request();
    r.as_object_mut().unwrap().remove("originNodeId");
    // Query at node "i"'s exact position.
    r["origin"] = json!({ "lat": 35.6815, "lon": 139.7675 });
    let result = run(&graph(), &r, json!({}));
    assert_eq!(result["status"], "ok");
    let c = candidates(&result);
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["snappedOrigin"]["nodeId"], "i");
    assert!(c[0]["snappedOrigin"]["distanceMeters"].as_f64().unwrap() < 1.0);
    assert_eq!(c[0]["origin"], json!({ "lat": 35.6815, "lon": 139.7675 }));
}

/// NO_CONNECTION is returned only when the graph contains no Entry edges at all
/// (snap grid is empty).  A far-away coordinate still snaps — there is no radius
/// cap — but may be filtered out by the time window instead.
#[test]
fn no_entry_edges_returns_no_connection() {
    let mut g = graph();
    // Remove all edges: no Entry edges → snap grid is empty → NO_CONNECTION.
    g["edges"] = json!([]);
    g["billingPairs"] = json!([]);
    let mut r = request();
    r.as_object_mut().unwrap().remove("originNodeId");
    r["origin"] = json!({ "lat": 35.6815, "lon": 139.7675 });
    let result = run(&g, &r, json!({}));
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
    // First coordinate: from-node of first edge = "i" [lon, lat]
    assert_eq!(coords.first().unwrap(), &json!([139.7675, 35.6815]));
    // Last coordinate: to-node of last edge = "o" [lon, lat]
    assert_eq!(coords.last().unwrap(), &json!([139.7685, 35.6818]));

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
    assert!(
        !warnings.contains(&"EXPERIMENTAL_NO_HANDOFF"),
        "warnings must not contain deprecated EXPERIMENTAL_NO_HANDOFF"
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

/// SearchLimits の maxGraphNodes / maxGraphEdges が小さい値に設定されたとき、
/// 合成グラフ（6 ノード, 7 エッジ）が正しく拒否されることを確認する。
#[test]
fn custom_graph_size_limits_reject_graph_that_exceeds_them() {
    let g = graph();
    let r = request();

    // synthetic graph has 5 nodes and 5 edges (after removing local edges).
    // maxGraphNodes を 4 に設定 → 5 ノードの合成グラフは超過して拒否されるべき
    assert!(
        search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxGraphNodes": 4}).to_string(),
        )
        .is_err(),
        "maxGraphNodes=4 should reject the 5-node synthetic graph"
    );

    // maxGraphEdges を 4 に設定 → 5 エッジの合成グラフは超過して拒否されるべき
    assert!(
        search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxGraphEdges": 4}).to_string(),
        )
        .is_err(),
        "maxGraphEdges=4 should reject the 5-edge synthetic graph"
    );

    // デフォルト limits（{}"）では同じグラフが受け付けられることも確認
    assert!(
        search_json(&g.to_string(), &r.to_string(), "{}").is_ok(),
        "default limits should accept the synthetic graph"
    );
}

/// Grid snap tie-breaking matches BTreeSet lex order for Entry from-nodes.
///
/// Two Entry from-nodes ("aaa-entry" and "zzz-entry") are placed symmetrically
/// at ≈90 m from a query point, giving them identical distances.  With
/// maxAccessEntries=1, k_nearest returns only one result; the lex-smaller ID
/// ("aaa-entry") must win — identical to what a BTreeSet linear scan returns.
#[test]
fn snap_grid_nearest_node_order_matches_linear_scan() {
    // "aaa-entry" < "zzz-entry" lexicographically.
    // Both are at (35.68, 139.760 ± 0.001°) → ≈90 m from query (35.68, 139.760).
    let g = json!({
        "schemaVersion": 2,
        "releaseId": "synthetic-v1",
        "vehicleProfile": "passenger-car-etc",
        "nodes": [
            // Entry from-nodes: equidistant from the query point.
            {"id": "aaa-entry", "lat": 35.68, "lon": 139.761},
            {"id": "zzz-entry", "lat": 35.68, "lon": 139.759},
            // Network 1 (from aaa-entry)
            {"id": "aaa-a",  "lat": 35.685, "lon": 139.761},
            {"id": "aaa-b",  "lat": 35.690, "lon": 139.771},
            {"id": "aaa-c",  "lat": 35.688, "lon": 139.781},
            {"id": "aaa-o",  "lat": 35.68,  "lon": 139.761},
            // Network 2 (from zzz-entry)
            {"id": "zzz-a",  "lat": 35.685, "lon": 139.759},
            {"id": "zzz-b",  "lat": 35.690, "lon": 139.749},
            {"id": "zzz-c",  "lat": 35.688, "lon": 139.739},
            {"id": "zzz-o",  "lat": 35.68,  "lon": 139.759}
        ],
        "edges": [
            // Network 1
            {"id":"aaa-entry-e","from":"aaa-entry","to":"aaa-a","kind":"entry",  "durationSeconds":30, "distanceMeters":200},
            {"id":"aaa-ab",     "from":"aaa-a",    "to":"aaa-b","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"aaa-bc",     "from":"aaa-b",    "to":"aaa-c","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"aaa-ca",     "from":"aaa-c",    "to":"aaa-a","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"aaa-exit-e", "from":"aaa-a",    "to":"aaa-o","kind":"exit",   "durationSeconds":30, "distanceMeters":200},
            // Network 2
            {"id":"zzz-entry-e","from":"zzz-entry","to":"zzz-a","kind":"entry",  "durationSeconds":30, "distanceMeters":200},
            {"id":"zzz-ab",     "from":"zzz-a",    "to":"zzz-b","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"zzz-bc",     "from":"zzz-b",    "to":"zzz-c","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"zzz-ca",     "from":"zzz-c",    "to":"zzz-a","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"zzz-exit-e", "from":"zzz-a",    "to":"zzz-o","kind":"exit",   "durationSeconds":30, "distanceMeters":200}
        ],
        "billingPairs": [
            {
                "id": "aaa-section",
                "entryId": "aaa-entry-e",
                "exitId": "aaa-exit-e",
                "anchorNodeId": "aaa-a",
                "entryToAnchorEdgeIds": ["aaa-entry-e"],
                "anchorToExitEdgeIds": ["aaa-exit-e"],
                "status": "verified",
                "vehicleProfile": "passenger-car-etc",
                "prices": [{"amountYen": 300, "effectiveFrom": "2026-01-01T00:00:00Z"}]
            },
            {
                "id": "zzz-section",
                "entryId": "zzz-entry-e",
                "exitId": "zzz-exit-e",
                "anchorNodeId": "zzz-a",
                "entryToAnchorEdgeIds": ["zzz-entry-e"],
                "anchorToExitEdgeIds": ["zzz-exit-e"],
                "status": "verified",
                "vehicleProfile": "passenger-car-etc",
                "prices": [{"amountYen": 300, "effectiveFrom": "2026-01-01T00:00:00Z"}]
            }
        ],
        "forbiddenTransitions": []
    });

    // Query equidistant from both Entry from-nodes.
    // maxAccessEntries=1: k_nearest returns only the lex-smaller node.
    let r = json!({
        "requestId": "snap-grid-order-test",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.68, "lon": 139.760},
        "minMinutes": 30,
        "maxMinutes": 60,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let limits = json!({"maxAccessEntries": 1});

    let result: serde_json::Value = serde_json::from_str(
        &search_json(&g.to_string(), &r.to_string(), &limits.to_string()).unwrap(),
    )
    .unwrap();

    // With maxAccessEntries=1, only "aaa-entry" (lex-smaller) is tried.
    assert_eq!(
        result["status"], "ok",
        "search must succeed: {:?}",
        result["reason"]
    );
    let c = &result["candidates"][0];
    assert_eq!(
        c["snappedOrigin"]["nodeId"], "aaa-entry",
        "lex-smaller Entry node must win on distance tie"
    );
    // Sanity: both nodes are ≈90 m away.
    let d = c["snappedOrigin"]["distanceMeters"]
        .as_f64()
        .unwrap()
        .round() as u64;
    assert!(
        (85..=95).contains(&d),
        "snap distance should be ≈90 m, got {d}"
    );
}
