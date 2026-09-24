use serde_json::{json, Value};
use shutoko_routing_core::{search_json, MAX_PRODUCT_MINUTES};

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
    // maxAccessEntries is excluded from this list: 0 now means "unlimited" (all entries).
    for limit in [
        "maxExpandedStates",
        "beamWidth",
        "maxLoopEdges",
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
    // maxAccessEntries=0 is valid (unlimited); verify it does not error.
    {
        let limits = json!({"maxAccessEntries": 0});
        assert!(
            search_json(
                &graph().to_string(),
                &request().to_string(),
                &limits.to_string()
            )
            .is_ok(),
            "maxAccessEntries=0 must be accepted as unlimited"
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

// ---------------------------------------------------------------------------
// Task A: max_access_distance_meters
// ---------------------------------------------------------------------------

/// Helper — minimal two-entry graph used by several access-limit tests.
///
/// Two independent networks ("aaa-*" and "zzz-*") are equidistant from a query
/// at (35.68, 139.760).  Neither network shares any edges, so Jaccard similarity
/// is 0 and the deduplication filter keeps both candidates.
fn two_network_graph() -> Value {
    json!({
        "schemaVersion": 2,
        "releaseId": "synthetic-v1",
        "vehicleProfile": "passenger-car-etc",
        "nodes": [
            {"id": "aaa-entry", "lat": 35.68, "lon": 139.761},
            {"id": "zzz-entry", "lat": 35.68, "lon": 139.759},
            {"id": "aaa-a",  "lat": 35.685, "lon": 139.761},
            {"id": "aaa-b",  "lat": 35.690, "lon": 139.771},
            {"id": "aaa-c",  "lat": 35.688, "lon": 139.781},
            {"id": "aaa-o",  "lat": 35.68,  "lon": 139.761},
            {"id": "zzz-a",  "lat": 35.685, "lon": 139.759},
            {"id": "zzz-b",  "lat": 35.690, "lon": 139.749},
            {"id": "zzz-c",  "lat": 35.688, "lon": 139.739},
            {"id": "zzz-o",  "lat": 35.68,  "lon": 139.759}
        ],
        "edges": [
            {"id":"aaa-entry-e","from":"aaa-entry","to":"aaa-a","kind":"entry",  "durationSeconds":30, "distanceMeters":200},
            {"id":"aaa-ab",     "from":"aaa-a",    "to":"aaa-b","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"aaa-bc",     "from":"aaa-b",    "to":"aaa-c","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"aaa-ca",     "from":"aaa-c",    "to":"aaa-a","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"aaa-exit-e", "from":"aaa-a",    "to":"aaa-o","kind":"exit",   "durationSeconds":30, "distanceMeters":200},
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
    })
}

/// max_access_distance_meters must be finite and non-negative;
/// negative, NaN, and Infinity must be rejected at prepare time.
#[test]
fn rejects_invalid_max_access_distance_meters() {
    let g = graph();
    let r = request();

    // negative
    assert!(
        search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessDistanceMeters": -1.0}).to_string()
        )
        .is_err(),
        "negative max_access_distance_meters must be rejected"
    );
    // NaN: serde_json does not serialize f64::NAN to JSON; inject as string
    assert!(
        search_json(
            &g.to_string(),
            &r.to_string(),
            // f64::NAN cannot appear in JSON; only finite + 0 are valid
            &json!({"maxAccessDistanceMeters": -0.001}).to_string()
        )
        .is_err(),
        "sub-zero max_access_distance_meters must be rejected"
    );
    // Infinity: not representable as JSON number, so the smallest invalid
    // case we can inject is a very large (but finite) value — that is valid.
    // Confirm 0.0 (unlimited) is accepted.
    assert!(
        search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessDistanceMeters": 0.0}).to_string()
        )
        .is_ok(),
        "0.0 (unlimited) must be accepted"
    );
    // A normal cap is also accepted.
    assert!(
        search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessDistanceMeters": 30000.0}).to_string()
        )
        .is_ok(),
        "30 000 m cap must be accepted"
    );
}

/// A graph with entry at ~4 km from the origin.
/// With a 3 km cap the entry is too far → NO_CONNECTION.
/// With a 5 km cap the entry is within range → search proceeds (not NO_CONNECTION).
/// With 0.0 (unlimited) the entry is reachable regardless of distance.
#[test]
fn distance_cap_enforced_and_zero_means_unlimited() {
    // Entry node "e" is roughly 4 km north of origin (35.0, 139.0).
    // 0.036° latitude ≈ 0.036 × 111 320 ≈ 4 007 m.
    let g = json!({
        "schemaVersion": 2,
        "releaseId": "synthetic-v1",
        "vehicleProfile": "passenger-car-etc",
        "nodes": [
            {"id": "e", "lat": 35.036, "lon": 139.0},
            {"id": "a", "lat": 35.038, "lon": 139.0},
            {"id": "b", "lat": 35.040, "lon": 139.001},
            {"id": "c", "lat": 35.039, "lon": 139.002},
            {"id": "o", "lat": 35.037, "lon": 139.0}
        ],
        "edges": [
            {"id":"entry","from":"e","to":"a","kind":"entry",  "durationSeconds":30, "distanceMeters":200},
            {"id":"ab",   "from":"a","to":"b","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"bc",   "from":"b","to":"c","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"ca",   "from":"c","to":"a","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"exit", "from":"a","to":"o","kind":"exit",   "durationSeconds":30, "distanceMeters":200}
        ],
        "billingPairs": [{
            "id": "sec",
            "entryId": "entry",
            "exitId": "exit",
            "anchorNodeId": "a",
            "entryToAnchorEdgeIds": ["entry"],
            "anchorToExitEdgeIds": ["exit"],
            "status": "verified",
            "vehicleProfile": "passenger-car-etc",
            "prices": [{"amountYen": 300, "effectiveFrom": "2026-01-01T00:00:00Z"}]
        }],
        "forbiddenTransitions": []
    });
    // Origin at (35.0, 139.0), entry "e" at ~4 007 m.
    let r = json!({
        "requestId": "dist-cap-test",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.0, "lon": 139.0},
        "minMinutes": 30,
        "maxMinutes": 120,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });

    // 3 km cap: entry ~4 007 m away → NO_CONNECTION.
    let res_inside: Value = serde_json::from_str(
        &search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessDistanceMeters": 3000.0}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(res_inside["status"], "no_candidates");
    assert_eq!(
        res_inside["reason"], "NO_CONNECTION",
        "3 km cap: entry at ~4 km must trigger NO_CONNECTION"
    );
    // Cap-exceeded NO_CONNECTION must still report the nearest Entry access
    // point (distance in metres) so the UI can explain why the origin is out
    // of range.  No loop enumeration runs, so minPlanSeconds stays null.
    assert_eq!(
        res_inside["nearestAccess"]["nodeId"], "e",
        "cap-exceeded NO_CONNECTION must report the nearest Entry from-node"
    );
    let capped_distance = res_inside["nearestAccess"]["distanceMeters"]
        .as_f64()
        .expect("nearestAccess.distanceMeters must be a number");
    assert!(
        (3900.0..=4100.0).contains(&capped_distance),
        "nearest entry is ~4 007 m away, got {capped_distance}"
    );
    assert!(
        res_inside["minPlanSeconds"].is_null(),
        "cap-exceeded early return must not report minPlanSeconds"
    );

    // 5 km cap: entry ~4 007 m is within range → search proceeds (status ≠ NO_CONNECTION).
    let res_within: Value = serde_json::from_str(
        &search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessDistanceMeters": 5000.0}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(
        res_within["reason"], "NO_CONNECTION",
        "5 km cap: entry at ~4 km must NOT trigger NO_CONNECTION"
    );
    // Coordinate input always reports the nearest access point, even when the
    // search succeeds, and the only loop found equals the single candidate.
    let within_distance = res_within["nearestAccess"]["distanceMeters"]
        .as_f64()
        .expect("nearestAccess must be reported for coordinate input");
    assert!(
        (3900.0..=4100.0).contains(&within_distance),
        "nearest access must be the same ~4 007 m entry, got {within_distance}"
    );
    assert_eq!(
        res_within["minPlanSeconds"].as_u64(),
        res_within["candidates"][0]["duration"]["planSeconds"].as_u64(),
        "single-candidate search: minPlanSeconds must equal the candidate planSeconds"
    );

    // 0.0 (unlimited): entry accessible regardless of distance.
    let res_unlimited: Value = serde_json::from_str(
        &search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessDistanceMeters": 0.0}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(
        res_unlimited["reason"], "NO_CONNECTION",
        "unlimited cap (0.0): must NOT trigger NO_CONNECTION"
    );
    // Both 5 km and unlimited cases should find the same candidate (same graph, same route).
    assert_eq!(
        res_within["status"], res_unlimited["status"],
        "5 km cap and unlimited must produce the same search outcome"
    );
    assert!(
        res_unlimited["nearestAccess"].is_object(),
        "unlimited cap: nearestAccess must still be reported for coordinate input"
    );
}

// ---------------------------------------------------------------------------
// engine-001: nearestAccess / minPlanSeconds diagnostics
// ---------------------------------------------------------------------------

/// With `originNodeId` input no snapping happens, so `nearestAccess` is null.
/// `minPlanSeconds` is independent of the input mode and must equal the single
/// candidate's plan when that candidate's loop is the only legal loop.
#[test]
fn origin_node_input_has_no_nearest_access_but_reports_min_plan_seconds() {
    let result = run(&graph(), &request(), json!({}));
    assert_eq!(result["status"], "ok");
    assert!(
        result["nearestAccess"].is_null(),
        "originNodeId input must not report nearestAccess, got {}",
        result["nearestAccess"]
    );
    assert_eq!(
        result["minPlanSeconds"].as_u64(),
        candidates(&result)[0]["duration"]["planSeconds"].as_u64(),
        "minPlanSeconds must equal the only legal loop's plan_seconds"
    );
}

/// When no legal loop exists, `minPlanSeconds` is null and the reason is
/// `NO_LOOP`, keeping the existing reason semantics unchanged.
#[test]
fn min_plan_seconds_is_null_when_no_legal_loop_exists() {
    let mut g = graph();
    g["edges"]
        .as_array_mut()
        .unwrap()
        .retain(|edge| edge["kind"] != "shutoko");
    let mut r = request();
    r["minMinutes"] = json!(1);
    let result = run(&g, &r, json!({}));
    assert_eq!(result["reason"], "NO_LOOP");
    assert!(
        result["minPlanSeconds"].is_null(),
        "no legal loop must yield null minPlanSeconds, got {}",
        result["minPlanSeconds"]
    );
    assert!(result["nearestAccess"].is_null());
}

// ---------------------------------------------------------------------------
// Task B: access time with coordinate origin affects base_seconds / time window
// ---------------------------------------------------------------------------

/// Origin at the exact entry-node coordinates yields access_seconds=0 and
/// base_seconds=1876 (matching the originNodeId contract).  An origin ~4 km
/// away yields a larger base and is rejected by the same narrow time window.
/// Widening the window to 70 min accepts the farther origin.
///
/// Entry node "i" in the synthetic graph is at (35.6815, 139.7675).
/// Far origin used: (35.65, 139.75) ≈ 3 842 m from "i".
///
/// Computed values (equirectangular approx):
///   access_secs = ⌈3 842 × 1.3 / (30/3.6)⌉ = ⌈4 994.6/8.333⌉ = 600 s
///   return_dist ("o"→origin) ≈ 3 910 m  →  return_secs = 610 s
///   base = 600 + 1 860 + 610 = 3 070 s   plan = 3 070 + 614 = 3 684 s
///   → rejected at max_minutes=40 (2 400 s), accepted at max_minutes=70 (4 200 s).
#[test]
fn access_time_from_coordinate_origin_affects_base_seconds_and_time_window() {
    let g = graph(); // synthetic graph: entry "i" at (35.6815, 139.7675)

    // ── Case 1: origin at exact entry-node coordinates ──
    let r_near = json!({
        "requestId": "coord-access-near",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.6815, "lon": 139.7675},
        "minMinutes": 30,
        "maxMinutes": 40,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let res_near: Value =
        serde_json::from_str(&search_json(&g.to_string(), &r_near.to_string(), "{}").unwrap())
            .unwrap();
    assert_eq!(
        res_near["status"], "ok",
        "origin at entry node, window [30,40]: expected candidate, got reason={:?}",
        res_near["reason"]
    );
    let c_near = &res_near["candidates"][0];
    assert_eq!(
        c_near["duration"]["accessSeconds"], 0,
        "origin at entry node must have 0 access seconds"
    );
    assert_eq!(
        c_near["duration"]["baseSeconds"], 1876,
        "origin at entry node: base must match the originNodeId contract (1876 s)"
    );

    // ── Case 2: origin ~4 km away — same narrow window rejected (TIME_WINDOW) ──
    let r_far_narrow = json!({
        "requestId": "coord-access-far-narrow",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.65, "lon": 139.75},
        "minMinutes": 30,
        "maxMinutes": 40,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let res_far_narrow: Value = serde_json::from_str(
        &search_json(&g.to_string(), &r_far_narrow.to_string(), "{}").unwrap(),
    )
    .unwrap();
    // base ≈ 3 070 s → plan ≈ 3 684 s > 40×60=2 400 → TIME_WINDOW
    assert_eq!(
        res_far_narrow["status"], "no_candidates",
        "far origin, window [30,40]: expected no_candidates"
    );
    assert_eq!(
        res_far_narrow["reason"], "TIME_WINDOW",
        "far origin, window [30,40]: rejection reason must be TIME_WINDOW, not NO_CONNECTION"
    );
    // TIME_WINDOW diagnostics: the nearest entry distance and the shortest plan
    // time must be reported so the UI can explain why no candidate fits.
    let narrow_nearest = res_far_narrow["nearestAccess"]["distanceMeters"]
        .as_f64()
        .expect("coordinate input must report nearestAccess");
    assert!(
        (3800.0..=3900.0).contains(&narrow_nearest),
        "far origin nearest entry is ~3 842 m away, got {narrow_nearest}"
    );
    let narrow_min_plan = res_far_narrow["minPlanSeconds"]
        .as_u64()
        .expect("TIME_WINDOW must report minPlanSeconds");
    assert!(
        narrow_min_plan > 40 * 60,
        "minPlanSeconds ({narrow_min_plan}) must exceed the 40 min window (2 400 s)"
    );

    // ── Case 3: same far origin, wider window [30, 70] — candidate found ──
    let r_far_wide = json!({
        "requestId": "coord-access-far-wide",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.65, "lon": 139.75},
        "minMinutes": 30,
        "maxMinutes": 70,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let res_far_wide: Value =
        serde_json::from_str(&search_json(&g.to_string(), &r_far_wide.to_string(), "{}").unwrap())
            .unwrap();
    assert_eq!(
        res_far_wide["status"], "ok",
        "far origin, window [30,70]: expected candidate, got reason={:?}",
        res_far_wide["reason"]
    );
    let c_far = &res_far_wide["candidates"][0];
    let access_far = c_far["duration"]["accessSeconds"].as_u64().unwrap();
    let base_far = c_far["duration"]["baseSeconds"].as_u64().unwrap();
    assert!(
        access_far > 0,
        "far origin must have non-zero access_seconds, got {access_far}"
    );
    assert!(
        base_far > 1876,
        "far origin base_seconds ({base_far}) must exceed the zero-access case (1876)"
    );
    assert_eq!(
        res_far_wide["minPlanSeconds"].as_u64(),
        c_far["duration"]["planSeconds"].as_u64(),
        "single legal loop must be reported as minPlanSeconds"
    );
}

// ---------------------------------------------------------------------------
// Task B: max_access_entries=0 is unlimited; positive value caps candidates
// ---------------------------------------------------------------------------

/// With max_access_entries=0 (unlimited, the default) both entries in a
/// two-network graph are tried and both produce candidates.
/// With max_access_entries=1 only the lex-smaller entry is tried, so only
/// one network's candidates appear.
#[test]
fn max_access_entries_zero_means_unlimited_tries_all_entries() {
    let g = two_network_graph();
    // Query equidistant from both Entry from-nodes.
    let r = json!({
        "requestId": "two-entry-unlimited",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.68, "lon": 139.760},
        "minMinutes": 30,
        "maxMinutes": 60,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });

    // ── 0 (unlimited): both billing pairs must appear ──
    let res_unlimited: Value = serde_json::from_str(
        &search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessEntries": 0}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        res_unlimited["status"], "ok",
        "unlimited entries: expected ok, got reason={:?}",
        res_unlimited["reason"]
    );
    let ids_unlimited: Vec<&str> = res_unlimited["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["toll"]["billingPairId"].as_str().unwrap())
        .collect();
    assert!(
        ids_unlimited.contains(&"aaa-section"),
        "unlimited: aaa-section must be present, candidates={ids_unlimited:?}"
    );
    assert!(
        ids_unlimited.contains(&"zzz-section"),
        "unlimited: zzz-section must be present, candidates={ids_unlimited:?}"
    );

    // ── 1 (capped): only the lex-smaller "aaa-entry" is tried ──
    let res_capped: Value = serde_json::from_str(
        &search_json(
            &g.to_string(),
            &r.to_string(),
            &json!({"maxAccessEntries": 1}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        res_capped["status"], "ok",
        "capped entries: expected ok, got reason={:?}",
        res_capped["reason"]
    );
    let ids_capped: Vec<&str> = res_capped["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["toll"]["billingPairId"].as_str().unwrap())
        .collect();
    assert!(
        ids_capped.contains(&"aaa-section"),
        "cap=1: aaa-section must be present"
    );
    assert!(
        !ids_capped.contains(&"zzz-section"),
        "cap=1: zzz-section must NOT be present (lex-larger entry excluded)"
    );
}

// ---------------------------------------------------------------------------
// correct-001: `minPlanSeconds` must cover every legal loop the product can
// ever accept, not only loops inside the requested window (review TEST-01 /
// opus5 F1).
// ---------------------------------------------------------------------------

/// A legal loop of 61 minutes only becomes visible when the *diagnostic*
/// enumeration is not bounded by the requested 60-minute window.  Before the
/// fix `minPlanSeconds` was `null` (the 3 660 s loop was pruned at 3 600 s),
/// so the UI could not tell "widen the window" from "no loop at all".
#[test]
fn min_plan_seconds_covers_loops_pruned_by_the_request_window() {
    let mut g = graph();
    // Loop ab+bc+ca = 3 × 1220 s = 3 660 s (61 min) > 60 min window.
    for edge in g["edges"].as_array_mut().unwrap() {
        if edge["kind"] == "shutoko" {
            edge["durationSeconds"] = json!(1220);
        }
    }
    // originNodeId "i": access = 0, return = 16 s (same as the base fixture).
    let r = json!({
        "requestId": "diag-pruned-loop",
        "releaseId": "synthetic-v1",
        "originNodeId": "i",
        "minMinutes": 30,
        "maxMinutes": 60,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let result = run(&g, &r, json!({}));

    // The requested window rejects the loop, but a legal loop exists.
    assert_eq!(
        result["status"], "no_candidates",
        "61-min loop cannot fit a 60-min window"
    );
    assert_eq!(
        result["reason"], "TIME_WINDOW",
        "a legal loop outside the window must be TIME_WINDOW, not NO_LOOP"
    );
    // base = 0 + (30 + 3660 + 30) + 16 = 3736; buffer = max(300, ceil(3736/5)) = 748.
    assert_eq!(
        result["minPlanSeconds"].as_u64(),
        Some(4484),
        "minPlanSeconds must report the pruned 61-min loop (plan 4 484 s)"
    );
    assert!(
        result["minPlanSeconds"].as_u64().unwrap() <= MAX_PRODUCT_MINUTES * 60,
        "minPlanSeconds must stay inside the product cap"
    );
}

/// Cross-pair: the loop-time pruning is a single bound shared by all pairs,
/// while `planSeconds` adds per-pair access/return.  A shorter *plan* on a
/// longer-loop pair can therefore be hidden by the requested window.  With a
/// 45-min window the near pair's 46-min loop is pruned while the far pair's
/// 44-min loop is enumerated; the reported minimum must still be the near
/// pair's smaller plan, not the far pair's larger one.
#[test]
fn min_plan_seconds_is_not_hidden_by_cross_pair_pruning() {
    // `MAX_PRODUCT_MINUTES_SECONDS` is asserted below; keep the local alias.
    const MAX_PRODUCT_MINUTES_SECONDS: u64 = MAX_PRODUCT_MINUTES * 60;

    let g = json!({
        "schemaVersion": 2,
        "releaseId": "synthetic-v1",
        "vehicleProfile": "passenger-car-etc",
        "nodes": [
            {"id": "near-e", "lat": 35.0, "lon": 139.0},
            {"id": "near-a", "lat": 35.001, "lon": 139.0},
            {"id": "near-b", "lat": 35.002, "lon": 139.001},
            {"id": "near-c", "lat": 35.001, "lon": 139.002},
            {"id": "near-o", "lat": 35.0, "lon": 139.0},
            {"id": "far-e", "lat": 35.0, "lon": 139.05},
            {"id": "far-a", "lat": 35.001, "lon": 139.05},
            {"id": "far-b", "lat": 35.002, "lon": 139.051},
            {"id": "far-c", "lat": 35.001, "lon": 139.052},
            {"id": "far-o", "lat": 35.0, "lon": 139.05}
        ],
        "edges": [
            {"id":"near-entry","from":"near-e","to":"near-a","kind":"entry","durationSeconds":30,"distanceMeters":200},
            {"id":"near-ab","from":"near-a","to":"near-b","kind":"shutoko","durationSeconds":920,"distanceMeters":10000},
            {"id":"near-bc","from":"near-b","to":"near-c","kind":"shutoko","durationSeconds":920,"distanceMeters":10000},
            {"id":"near-ca","from":"near-c","to":"near-a","kind":"shutoko","durationSeconds":920,"distanceMeters":10000},
            {"id":"near-exit","from":"near-a","to":"near-o","kind":"exit","durationSeconds":30,"distanceMeters":200},
            {"id":"far-entry","from":"far-e","to":"far-a","kind":"entry","durationSeconds":30,"distanceMeters":200},
            {"id":"far-ab","from":"far-a","to":"far-b","kind":"shutoko","durationSeconds":880,"distanceMeters":10000},
            {"id":"far-bc","from":"far-b","to":"far-c","kind":"shutoko","durationSeconds":880,"distanceMeters":10000},
            {"id":"far-ca","from":"far-c","to":"far-a","kind":"shutoko","durationSeconds":880,"distanceMeters":10000},
            {"id":"far-exit","from":"far-a","to":"far-o","kind":"exit","durationSeconds":30,"distanceMeters":200}
        ],
        "billingPairs": [
            {
                "id": "near-section",
                "entryId": "near-entry",
                "exitId": "near-exit",
                "anchorNodeId": "near-a",
                "entryToAnchorEdgeIds": ["near-entry"],
                "anchorToExitEdgeIds": ["near-exit"],
                "status": "verified",
                "vehicleProfile": "passenger-car-etc",
                "prices": [{"amountYen": 300, "effectiveFrom": "2026-01-01T00:00:00Z"}]
            },
            {
                "id": "far-section",
                "entryId": "far-entry",
                "exitId": "far-exit",
                "anchorNodeId": "far-a",
                "entryToAnchorEdgeIds": ["far-entry"],
                "anchorToExitEdgeIds": ["far-exit"],
                "status": "verified",
                "vehicleProfile": "passenger-car-etc",
                "prices": [{"amountYen": 300, "effectiveFrom": "2026-01-01T00:00:00Z"}]
            }
        ],
        "forbiddenTransitions": []
    });

    let wide = json!({
        "requestId": "diag-cross-wide",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.0, "lon": 139.0},
        "minMinutes": 30,
        "maxMinutes": 240,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let wide_result = run(&g, &wide, json!({}));
    assert_eq!(wide_result["status"], "ok");
    assert_eq!(
        wide_result["candidates"].as_array().unwrap().len(),
        2,
        "both independent networks must be candidate-visible in a wide window"
    );
    // The near pair has the shorter plan; ranking is time-per-yen so it leads.
    let near_plan = candidates(&wide_result)[0]["duration"]["planSeconds"]
        .as_u64()
        .unwrap();

    let narrow = json!({
        "requestId": "diag-cross-narrow",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.0, "lon": 139.0},
        "minMinutes": 30,
        "maxMinutes": 45,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let narrow_result = run(&g, &narrow, json!({}));
    assert_eq!(narrow_result["status"], "no_candidates");
    assert_eq!(narrow_result["reason"], "TIME_WINDOW");
    assert_eq!(
        narrow_result["minPlanSeconds"].as_u64(),
        Some(near_plan),
        "minPlanSeconds must reflect the near pair's smaller plan even though its \
         46-min loop was pruned by the 45-min window"
    );
    assert!(
        near_plan < MAX_PRODUCT_MINUTES_SECONDS,
        "the near pair must fit the product cap"
    );
}

// ---------------------------------------------------------------------------
// correct-001: NO_CONNECTION has a second origin — a reachable nearest Entry
// that belongs to no accessible verified pair (opus5 F2).  The UI must be able
// to tell this apart from the cap-exceeded case.
// ---------------------------------------------------------------------------

/// The nearest Entry from-node exists and is close (≈111 m), but it has no
/// verified billing pair.  With `maxAccessEntries = 1` only that entry is
/// selected, so no verified pair is reachable → `NO_CONNECTION` with a *close*
/// `nearestAccess` and `minPlanSeconds = null`.  The UI must not claim "the
/// access round trip alone takes four hours" for such a result.
#[test]
fn no_connection_can_report_a_close_nearest_entry_without_a_verified_pair() {
    let g = json!({
        "schemaVersion": 2,
        "releaseId": "synthetic-v1",
        "vehicleProfile": "passenger-car-etc",
        "nodes": [
            {"id": "close-e", "lat": 35.001, "lon": 139.0},
            {"id": "close-a", "lat": 35.0011, "lon": 139.0},
            {"id": "far-e", "lat": 35.01, "lon": 139.0},
            {"id": "far-a", "lat": 35.011, "lon": 139.0},
            {"id": "far-b", "lat": 35.012, "lon": 139.001},
            {"id": "far-c", "lat": 35.011, "lon": 139.002},
            {"id": "far-o", "lat": 35.01, "lon": 139.0}
        ],
        "edges": [
            {"id":"close-entry","from":"close-e","to":"close-a","kind":"entry","durationSeconds":30,"distanceMeters":200},
            {"id":"far-entry","from":"far-e","to":"far-a","kind":"entry","durationSeconds":30,"distanceMeters":200},
            {"id":"far-ab","from":"far-a","to":"far-b","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"far-bc","from":"far-b","to":"far-c","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"far-ca","from":"far-c","to":"far-a","kind":"shutoko","durationSeconds":600,"distanceMeters":10000},
            {"id":"far-exit","from":"far-a","to":"far-o","kind":"exit","durationSeconds":30,"distanceMeters":200}
        ],
        "billingPairs": [{
            "id": "far-section",
            "entryId": "far-entry",
            "exitId": "far-exit",
            "anchorNodeId": "far-a",
            "entryToAnchorEdgeIds": ["far-entry"],
            "anchorToExitEdgeIds": ["far-exit"],
            "status": "verified",
            "vehicleProfile": "passenger-car-etc",
            "prices": [{"amountYen": 300, "effectiveFrom": "2026-01-01T00:00:00Z"}]
        }],
        "forbiddenTransitions": []
    });
    let r = json!({
        "requestId": "no-connection-close-entry",
        "releaseId": "synthetic-v1",
        "origin": {"lat": 35.0, "lon": 139.0},
        "minMinutes": 30,
        "maxMinutes": 240,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    });
    let result = run(&g, &r, json!({"maxAccessEntries": 1}));

    assert_eq!(result["status"], "no_candidates");
    assert_eq!(
        result["reason"], "NO_CONNECTION",
        "only the close, unpaired entry is accessible → no verified pair is reachable"
    );
    // The nearest entry is reported and is *close* (well inside the 46 km cap).
    let nearest = &result["nearestAccess"];
    assert_eq!(nearest["nodeId"], "close-e");
    let distance = nearest["distanceMeters"].as_f64().unwrap();
    assert!(
        (50.0..=300.0).contains(&distance),
        "nearest entry must be ~111 m away, got {distance}"
    );
    assert!(
        distance < 46_000.0,
        "close-entry NO_CONNECTION must not look like the cap-exceeded case"
    );
    assert!(
        result["minPlanSeconds"].is_null(),
        "no loop enumeration runs when no verified pair is reachable"
    );
}

// ---------------------------------------------------------------------------
// correct_2: a truncated diagnostic enumeration must not report an unprovable
// minimum (review TEST-01-FINAL) and `time_rejected` must be scoped to the
// requested window (review V1 / V2).
// ---------------------------------------------------------------------------

/// Adds the lexicographically-first self-loop from the fixture anchor:
/// `a-loop-0` costs 14 000 s and is enumerated before `ab`, so with
/// `beamWidth = 1` the diagnostic pass records it and stops before the shorter
/// `ab → bc → ca` loop (1 860 s) is ever seen.  This is exactly the synthetic
/// counterexample from review TEST-01-FINAL.
fn graph_with_long_anchor_self_loop() -> Value {
    let mut g = graph();
    g["edges"].as_array_mut().unwrap().push(json!({
        "id": "a-loop-0",
        "from": "a",
        "to": "a",
        "kind": "shutoko",
        "durationSeconds": 14000,
        "distanceMeters": 100000
    }));
    g
}

fn one_minute_request(request_id: &str) -> Value {
    json!({
        "requestId": request_id,
        "releaseId": "synthetic-v1",
        "originNodeId": "i",
        "minMinutes": 1,
        "maxMinutes": 1,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    })
}

/// `beamWidth = 1`: the diagnostic pass keeps the 14 000 s self-loop and stops
/// (`paths_pg` records one result and reports truncation).  The unprovable
/// 16 892 s value must NOT be reported as the shortest plan — before the fix
/// the UI turned it into "even four hours cannot work".  `null` states the
/// truth: no minimum was proven.
#[test]
fn min_plan_seconds_is_null_when_the_diagnostic_is_beam_truncated() {
    let result = run(
        &graph_with_long_anchor_self_loop(),
        &one_minute_request("diag-beam-truncated"),
        json!({"beamWidth": 1}),
    );

    assert_eq!(result["status"], "no_candidates");
    assert_eq!(
        result["reason"], "TIME_WINDOW",
        "a legal loop exists outside the 1-minute window"
    );
    assert!(
        result["minPlanSeconds"].is_null(),
        "a beam-truncated diagnostic cannot prove the minimum, got {}",
        result["minPlanSeconds"]
    );
}

/// `maxExpandedStates = 5`: the candidate pass still finishes (2 expansions),
/// but the diagnostic pass exhausts the expanded-state Budget — it records the
/// 14 000 s self-loop before being cut off, and the post-processing loop stops
/// before the shorter loop is measured.  As with the beam case the minimum is
/// not provable and must be `null` instead of the 16 892 s value the fix
/// removes.
#[test]
fn min_plan_seconds_is_null_when_the_diagnostic_budget_is_exhausted() {
    let result = run(
        &graph_with_long_anchor_self_loop(),
        &one_minute_request("diag-budget-exhausted"),
        json!({"maxExpandedStates": 5}),
    );

    assert_eq!(result["status"], "no_candidates");
    assert!(
        matches!(result["reason"].as_str(), Some("TIME_WINDOW" | "NO_LOOP")),
        "a truncated diagnostic may only report an honest no-candidate reason, got {}",
        result["reason"]
    );
    assert!(
        result["minPlanSeconds"].is_null(),
        "an expanded-state-truncated diagnostic cannot prove the minimum, got {}",
        result["minPlanSeconds"]
    );
}

/// Without truncation the diagnostic is exact: the same graph and request with
/// the default limits enumerate both loops, so the shorter `ab → bc → ca`
/// loop (plan 2 252 s, inside the product cap) is reported.  This is the
/// control that separates "truncated ⇒ null" from "complete ⇒ exact".
#[test]
fn diagnostic_without_truncation_reports_the_true_minimum() {
    let result = run(
        &graph_with_long_anchor_self_loop(),
        &one_minute_request("diag-complete"),
        json!({}),
    );

    assert_eq!(result["status"], "no_candidates");
    assert_eq!(result["reason"], "TIME_WINDOW");
    assert_eq!(
        result["minPlanSeconds"].as_u64(),
        Some(2252),
        "with a complete enumeration the short loop (plan 2 252 s) is the minimum"
    );
    assert!(
        result["minPlanSeconds"].as_u64().unwrap() <= MAX_PRODUCT_MINUTES * 60,
        "a value inside the product cap must drive the 'widen the window' guidance"
    );
}

/// Boundary at `maxMinutes = 240`: the diagnostic pass is skipped because the
/// candidate enumeration already uses the product cap.  With the default beam
/// the enumeration is complete and the exact minimum is reported; with
/// `beamWidth = 1` it truncates, so the result must be `SEARCH_LIMIT` +
/// `truncated` with `minPlanSeconds = null` instead of a bogus TIME_WINDOW
/// "unreachable" claim.
#[test]
fn max_minutes_boundary_240_reports_exact_or_truncated_never_bogus() {
    let g = graph_with_long_anchor_self_loop();
    let mut r = one_minute_request("diag-max-240");
    r["minMinutes"] = json!(1);
    r["maxMinutes"] = json!(240);

    let exact = run(&g, &r, json!({}));
    assert_eq!(exact["status"], "ok");
    assert_eq!(
        exact["minPlanSeconds"].as_u64(),
        Some(2252),
        "at the product cap the diagnostic is unnecessary and the candidate \
         enumeration is exact"
    );

    let truncated = run(&g, &r, json!({"beamWidth": 1}));
    assert_eq!(
        truncated["status"], "truncated",
        "a beam-truncated search must keep reporting SEARCH_LIMIT"
    );
    assert_eq!(truncated["reason"], "SEARCH_LIMIT");
    assert!(
        truncated["minPlanSeconds"].is_null(),
        "a truncated enumeration must not report an unprovable minimum, got {}",
        truncated["minPlanSeconds"]
    );
}

/// Requirement 2 constraint: `NO_HANDOFF` is only reachable when
/// `format_maps_url` exceeds 2 048 characters.  `select_waypoints` returns at
/// most three waypoints and coordinates are printed with six decimals, so the
/// generated URL stays far below the limit — even for pathological extreme
/// coordinates.  `NO_HANDOFF` is therefore unreachable in practice and no
/// reason is fabricated for it.
#[test]
fn maps_url_length_cannot_trigger_no_handoff_with_three_waypoints() {
    use shutoko_routing_core::handoff::{format_maps_url, MAX_MAPS_URL_LENGTH};
    use shutoko_routing_core::LatLng;

    let endpoint = |lat: f64, lon: f64| LatLng { lat, lon };
    let origin = endpoint(-89.999999, -179.999999);
    let waypoints = vec![
        endpoint(-89.999999, -179.999999),
        endpoint(89.999999, 179.999999),
        endpoint(35.689672, 139.764424),
    ];
    let url = format_maps_url(&origin, &waypoints).expect("three waypoints must fit the limit");
    assert!(url.len() <= MAX_MAPS_URL_LENGTH);
    assert!(
        url.len() * 4 < MAX_MAPS_URL_LENGTH,
        "with <= 3 waypoints the URL must stay far below 2 048 chars, got {}",
        url.len()
    );
}

#[test]
fn micro_loop_exclusion_contract() {
    use shutoko_routing_core::{
        search, Edge, EdgeKind, Graph, Node, Price, SearchLimits, SearchRequest, VerificationStatus,
    };

    // Graph with a 1,000m small loop (e.g. internal JCT connector)
    let g = Graph {
        schema_version: 3,
        release_id: "test-micro-loop".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node {
                id: "n:e".into(),
                lat: 35.680,
                lon: 139.760,
            },
            Node {
                id: "n:a".into(),
                lat: 35.681,
                lon: 139.761,
            },
            Node {
                id: "n:b".into(),
                lat: 35.682,
                lon: 139.762,
            },
            Node {
                id: "n:x".into(),
                lat: 35.683,
                lon: 139.763,
            },
        ],
        edges: vec![
            Edge {
                id: "e:entry".into(),
                from: "n:e".into(),
                to: "n:a".into(),
                kind: EdgeKind::Entry,
                duration_seconds: 10,
                distance_meters: 100,
                name: Some("Entry".into()),
            },
            // Micro loop: a -> b -> a (total 1000m)
            Edge {
                id: "e:loop1".into(),
                from: "n:a".into(),
                to: "n:b".into(),
                kind: EdgeKind::Shutoko,
                duration_seconds: 50,
                distance_meters: 500,
                name: Some("Connector".into()),
            },
            Edge {
                id: "e:loop2".into(),
                from: "n:b".into(),
                to: "n:a".into(),
                kind: EdgeKind::Shutoko,
                duration_seconds: 50,
                distance_meters: 500,
                name: Some("Connector".into()),
            },
            Edge {
                id: "e:exit".into(),
                from: "n:b".into(),
                to: "n:x".into(),
                kind: EdgeKind::Exit,
                duration_seconds: 10,
                distance_meters: 100,
                name: Some("Exit".into()),
            },
        ],
        billing_pairs: vec![shutoko_routing_core::BillingPair {
            id: "bp-1".into(),
            entry_id: "e:entry".into(),
            exit_id: "e:exit".into(),
            anchor_node_id: "n:a".into(),
            entry_to_anchor_edge_ids: vec!["e:entry".into()],
            anchor_to_exit_edge_ids: vec!["e:loop1".into(), "e:exit".into()],
            status: VerificationStatus::Verified,
            vehicle_profile: "passenger-car-etc".into(),
            prices: vec![Price {
                amount_yen: 300,
                effective_from: "2026-01-01T00:00:00Z".into(),
                effective_to: None,
            }],
            entry_name: None,
            exit_name: None,
            entry_ramp_id: None,
            exit_ramp_id: None,
            billing_distance_meters: None,
        }],
        forbidden_transitions: vec![],
        ramps: vec![],
        od_tariffs: vec![],
    };

    let req = SearchRequest {
        request_id: "req-1".into(),
        release_id: "test-micro-loop".into(),
        origin_node_id: Some("n:e".into()),
        origin: None,
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 1,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };

    // Case 1: with min_loop_meters = 5000, 1000m micro-loop is rejected!
    let limits_strict = SearchLimits {
        min_loop_meters: 5000,
        ..SearchLimits::default()
    };
    let res_strict = search(&g, &req, &limits_strict).unwrap();
    assert_eq!(res_strict.status, "no_candidates");
    assert_eq!(res_strict.reason.as_deref(), Some("NO_LOOP"));

    // Case 2: with min_loop_meters = 0, loop is permitted
    let limits_permissive = SearchLimits {
        min_loop_meters: 0,
        ..SearchLimits::default()
    };
    let res_permissive = search(&g, &req, &limits_permissive).unwrap();
    assert_eq!(res_permissive.status, "ok");
    assert_eq!(res_permissive.candidates.len(), 1);
}

#[test]
fn search_request_explicit_ramp_filters() {
    use shutoko_routing_core::{
        search, Edge, EdgeKind, Graph, Node, Price, Ramp, RampKind, SearchLimits, SearchRequest,
        VerificationStatus,
    };

    let g = Graph {
        schema_version: 3,
        release_id: "test-ramp-filter".into(),
        vehicle_profile: "passenger-car-etc".into(),
        nodes: vec![
            Node {
                id: "n:e1".into(),
                lat: 35.680,
                lon: 139.760,
            },
            Node {
                id: "n:e2".into(),
                lat: 35.681,
                lon: 139.760,
            },
            Node {
                id: "n:a".into(),
                lat: 35.682,
                lon: 139.761,
            },
            Node {
                id: "n:b".into(),
                lat: 35.683,
                lon: 139.762,
            },
            Node {
                id: "n:x1".into(),
                lat: 35.684,
                lon: 139.763,
            },
        ],
        edges: vec![
            Edge {
                id: "e:entry1".into(),
                from: "n:e1".into(),
                to: "n:a".into(),
                kind: EdgeKind::Entry,
                duration_seconds: 10,
                distance_meters: 100,
                name: Some("Ramp 1 Entry".into()),
            },
            Edge {
                id: "e:entry2".into(),
                from: "n:e2".into(),
                to: "n:a".into(),
                kind: EdgeKind::Entry,
                duration_seconds: 10,
                distance_meters: 100,
                name: Some("Ramp 2 Entry".into()),
            },
            Edge {
                id: "e:loop1".into(),
                from: "n:a".into(),
                to: "n:b".into(),
                kind: EdgeKind::Shutoko,
                duration_seconds: 50,
                distance_meters: 500,
                name: Some("C1".into()),
            },
            Edge {
                id: "e:loop2".into(),
                from: "n:b".into(),
                to: "n:a".into(),
                kind: EdgeKind::Shutoko,
                duration_seconds: 50,
                distance_meters: 500,
                name: Some("C1".into()),
            },
            Edge {
                id: "e:exit1".into(),
                from: "n:b".into(),
                to: "n:x1".into(),
                kind: EdgeKind::Exit,
                duration_seconds: 10,
                distance_meters: 100,
                name: Some("Ramp 1 Exit".into()),
            },
        ],
        billing_pairs: vec![
            shutoko_routing_core::BillingPair {
                id: "bp-1".into(),
                entry_id: "e:entry1".into(),
                exit_id: "e:exit1".into(),
                anchor_node_id: "n:a".into(),
                entry_to_anchor_edge_ids: vec!["e:entry1".into()],
                anchor_to_exit_edge_ids: vec!["e:loop1".into(), "e:exit1".into()],
                status: VerificationStatus::Verified,
                vehicle_profile: "passenger-car-etc".into(),
                prices: vec![Price {
                    amount_yen: 300,
                    effective_from: "2026-01-01T00:00:00Z".into(),
                    effective_to: None,
                }],
                entry_name: None,
                exit_name: None,
                entry_ramp_id: Some("ramp:c1:kandabashi-entry".into()),
                exit_ramp_id: Some("ramp:c1:kandabashi-exit".into()),
                billing_distance_meters: Some(1500),
            },
            shutoko_routing_core::BillingPair {
                id: "bp-2".into(),
                entry_id: "e:entry2".into(),
                exit_id: "e:exit1".into(),
                anchor_node_id: "n:a".into(),
                entry_to_anchor_edge_ids: vec!["e:entry2".into()],
                anchor_to_exit_edge_ids: vec!["e:loop1".into(), "e:exit1".into()],
                status: VerificationStatus::Verified,
                vehicle_profile: "passenger-car-etc".into(),
                prices: vec![Price {
                    amount_yen: 300,
                    effective_from: "2026-01-01T00:00:00Z".into(),
                    effective_to: None,
                }],
                entry_name: None,
                exit_name: None,
                entry_ramp_id: Some("ramp:c1:ginza-entry".into()),
                exit_ramp_id: Some("ramp:c1:kandabashi-exit".into()),
                billing_distance_meters: Some(2500),
            },
        ],
        forbidden_transitions: vec![],
        ramps: vec![
            Ramp {
                id: "ramp:c1:kandabashi-entry".into(),
                facility_id: "fac:kandabashi".into(),
                name: "神田橋".into(),
                route: "C1".into(),
                direction: "inner".into(),
                kind: RampKind::GeneralEntry,
                edge_id: "e:entry1".into(),
                node_id: "n:e1".into(),
                mainline_node_id: "n:a".into(),
                restrictions: vec![],
            },
            Ramp {
                id: "ramp:c1:ginza-entry".into(),
                facility_id: "fac:ginza".into(),
                name: "銀座".into(),
                route: "C1".into(),
                direction: "inner".into(),
                kind: RampKind::GeneralEntry,
                edge_id: "e:entry2".into(),
                node_id: "n:e2".into(),
                mainline_node_id: "n:a".into(),
                restrictions: vec![],
            },
            Ramp {
                id: "ramp:c1:kandabashi-exit".into(),
                facility_id: "fac:kandabashi".into(),
                name: "神田橋".into(),
                route: "C1".into(),
                direction: "inner".into(),
                kind: RampKind::GeneralExit,
                edge_id: "e:exit1".into(),
                node_id: "n:x1".into(),
                mainline_node_id: "n:b".into(),
                restrictions: vec![],
            },
        ],
        od_tariffs: vec![],
    };

    let limits = SearchLimits {
        min_loop_meters: 0,
        ..SearchLimits::default()
    };

    // 1. Query specifying entry_ramp_id = "ramp:c1:ginza-entry"
    let req_ginza = SearchRequest {
        request_id: "req-ginza".into(),
        release_id: "test-ramp-filter".into(),
        origin_node_id: Some("n:e2".into()),
        origin: None,
        entry_ramp_id: Some("ramp:c1:ginza-entry".into()),
        exit_ramp_id: None,
        min_minutes: 1,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let res_ginza = search(&g, &req_ginza, &limits).unwrap();
    assert_eq!(res_ginza.status, "ok");
    assert_eq!(res_ginza.candidates.len(), 1);
    assert_eq!(
        res_ginza.candidates[0].entry().ramp_id.as_deref(),
        Some("ramp:c1:ginza-entry")
    );

    // 2. Query specifying mismatched entry ramp
    let req_mismatch = SearchRequest {
        request_id: "req-mismatch".into(),
        release_id: "test-ramp-filter".into(),
        origin_node_id: Some("n:e1".into()),
        origin: None,
        entry_ramp_id: Some("ramp:c1:ginza-entry".into()), // origin n:e1 is kandabashi, requested ginza
        exit_ramp_id: None,
        min_minutes: 1,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let res_mismatch = search(&g, &req_mismatch, &limits).unwrap();
    assert_eq!(res_mismatch.status, "no_candidates");
}

#[test]
fn official_etc_toll_calculation_contract() {
    use shutoko_routing_core::calculate_etc_toll_yen;

    // Below minimum distance threshold (4,300m) -> 300 yen
    assert_eq!(calculate_etc_toll_yen(0), 300);
    assert_eq!(calculate_etc_toll_yen(1_000), 300);
    assert_eq!(calculate_etc_toll_yen(4_300), 300);

    // Intermediate distances
    // 5 km: (150 + 29.52 * 5) * 1.10 = 297.6 * 1.10 = 327.36 -> rounded to 330
    assert_eq!(calculate_etc_toll_yen(5_000), 330);

    // 10 km: (150 + 29.52 * 10) * 1.10 = 445.2 * 1.10 = 489.72 -> rounded to 490
    assert_eq!(calculate_etc_toll_yen(10_000), 490);

    // 20 km: (150 + 29.52 * 20) * 1.10 = 740.4 * 1.10 = 814.44 -> rounded to 810
    assert_eq!(calculate_etc_toll_yen(20_000), 810);

    // Cap at 1,950 yen
    assert_eq!(calculate_etc_toll_yen(55_000), 1950);
    assert_eq!(calculate_etc_toll_yen(100_000), 1950);
    assert_eq!(calculate_etc_toll_yen(500_000), 1950);

    // Multiple of 10 check for arbitrary distances
    for d in (0..=100_000).step_by(1_337) {
        let toll = calculate_etc_toll_yen(d);
        assert!((300..=1950).contains(&toll));
        assert_eq!(toll % 10, 0);
    }
}

#[test]
fn boundary_jct_and_half_ic_model_contract() {
    use shutoko_routing_core::RampKind;

    let kinds = [
        (RampKind::GeneralEntry, "\"general_entry\""),
        (RampKind::GeneralExit, "\"general_exit\""),
        (RampKind::BoundaryIn, "\"boundary_in\""),
        (RampKind::BoundaryOut, "\"boundary_out\""),
    ];

    for (k, expected_json) in kinds {
        let serialized = serde_json::to_string(&k).unwrap();
        assert_eq!(serialized, expected_json);
        let deserialized: RampKind = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, k);
    }
}

// ---------------------------------------------------------------------------
// Issue #57: coordinate entry-tier search (nearest entry, dynamic OD, Budget)
// ---------------------------------------------------------------------------
mod coordinate_entry_tiers {
    use shutoko_routing_core::{
        search, BillingPair, Candidate, Edge, EdgeKind, Graph, LatLng, LegacyCandidate, Node,
        OdTariff, Price, Ramp, RampKind, SearchLimits, SearchRequest, TopologyOnlyCandidate,
        VerificationStatus,
    };

    fn legacy(candidate: &Candidate) -> &LegacyCandidate {
        candidate
            .as_legacy()
            .expect("verified tier expects LegacyCandidate")
    }

    fn topology_only(candidate: &Candidate) -> &TopologyOnlyCandidate {
        candidate
            .as_topology_only()
            .expect("dynamic tier expects TopologyOnlyCandidate")
    }

    const RELEASE: &str = "tier-v1";
    const PROFILE: &str = "passenger-car-etc";
    const PRICING_AT: &str = "2026-09-10T00:00:00Z";
    const ORIGIN_LAT: f64 = 35.7000;
    const ORIGIN_LON: f64 = 139.7000;

    /// Minimal graph builder for the coordinate-tier contract.
    ///
    /// A three-node mainline cycle L1→L2→L3→L1 (1800 s / 30 km) is the shared
    /// loop; tests add GeneralEntry/GeneralExit ramps whose surface nodes are
    /// placed at controlled distances from the fixed origin.
    struct World {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        ramps: Vec<Ramp>,
        pairs: Vec<BillingPair>,
        tariffs: Vec<OdTariff>,
    }

    impl World {
        fn base(with_mainline_cycle: bool) -> Self {
            let mut world = World {
                nodes: Vec::new(),
                edges: Vec::new(),
                ramps: Vec::new(),
                pairs: Vec::new(),
                tariffs: Vec::new(),
            };
            if with_mainline_cycle {
                world.node("L1", ORIGIN_LAT, ORIGIN_LON);
                world.node("L2", 35.7100, 139.7000);
                world.node("L3", 35.7050, 139.7100);
                world.shutoko("c12", "L1", "L2", 600, 10_000);
                world.shutoko("c23", "L2", "L3", 600, 10_000);
                world.shutoko("c31", "L3", "L1", 600, 10_000);
            }
            world
        }

        fn new() -> Self {
            Self::base(true)
        }

        fn node(&mut self, id: &str, lat: f64, lon: f64) {
            if self.nodes.iter().any(|n| n.id == id) {
                return;
            }
            self.nodes.push(Node {
                id: id.into(),
                lat,
                lon,
            });
        }

        fn shutoko(&mut self, id: &str, from: &str, to: &str, seconds: u64, meters: u64) {
            self.edges.push(Edge {
                id: id.into(),
                from: from.into(),
                to: to.into(),
                kind: EdgeKind::Shutoko,
                duration_seconds: seconds,
                distance_meters: meters,
                name: Some("C1".into()),
            });
        }

        fn entry(
            &mut self,
            ramp_id: &str,
            facility: &str,
            surface: &str,
            lat: f64,
            lon: f64,
            mainline: &str,
        ) -> String {
            let edge_id = format!("e:{ramp_id}");
            self.node(surface, lat, lon);
            self.edges.push(Edge {
                id: edge_id.clone(),
                from: surface.into(),
                to: mainline.into(),
                kind: EdgeKind::Entry,
                duration_seconds: 30,
                distance_meters: 200,
                name: None,
            });
            self.ramps.push(Ramp {
                id: ramp_id.into(),
                facility_id: facility.into(),
                name: format!("{ramp_id}:name"),
                route: "C1".into(),
                direction: "inner".into(),
                kind: RampKind::GeneralEntry,
                edge_id: edge_id.clone(),
                node_id: surface.into(),
                mainline_node_id: mainline.into(),
                restrictions: vec![],
            });
            edge_id
        }

        fn exit(
            &mut self,
            ramp_id: &str,
            facility: &str,
            surface: &str,
            lat: f64,
            lon: f64,
            mainline: &str,
        ) -> String {
            let edge_id = format!("e:{ramp_id}");
            self.node(surface, lat, lon);
            self.edges.push(Edge {
                id: edge_id.clone(),
                from: mainline.into(),
                to: surface.into(),
                kind: EdgeKind::Exit,
                duration_seconds: 30,
                distance_meters: 200,
                name: None,
            });
            self.ramps.push(Ramp {
                id: ramp_id.into(),
                facility_id: facility.into(),
                name: format!("{ramp_id}:name"),
                route: "C1".into(),
                direction: "inner".into(),
                kind: RampKind::GeneralExit,
                edge_id: edge_id.clone(),
                node_id: surface.into(),
                mainline_node_id: mainline.into(),
                restrictions: vec![],
            });
            edge_id
        }

        #[allow(clippy::too_many_arguments)]
        fn verified_pair(
            &mut self,
            pair_id: &str,
            entry_edge: &str,
            exit_edge: &str,
            anchor: &str,
            entry_ramp: &str,
            exit_ramp: &str,
            entry_name: &str,
            exit_name: &str,
        ) {
            self.pairs.push(BillingPair {
                id: pair_id.into(),
                entry_id: entry_edge.into(),
                exit_id: exit_edge.into(),
                anchor_node_id: anchor.into(),
                entry_to_anchor_edge_ids: vec![entry_edge.into()],
                anchor_to_exit_edge_ids: vec![exit_edge.into()],
                status: VerificationStatus::Verified,
                vehicle_profile: PROFILE.into(),
                prices: vec![Price {
                    amount_yen: 300,
                    effective_from: "2022-03-31T15:00:00Z".into(),
                    effective_to: None,
                }],
                entry_name: Some(entry_name.into()),
                exit_name: Some(exit_name.into()),
                entry_ramp_id: Some(entry_ramp.into()),
                exit_ramp_id: Some(exit_ramp.into()),
                billing_distance_meters: Some(1_500),
            });
        }

        fn tariff(&mut self, entry_ramp: &str, exit_ramp: &str, meters: u64, yen: u64) {
            self.tariffs.push(OdTariff {
                entry_ramp_id: entry_ramp.into(),
                exit_ramp_id: exit_ramp.into(),
                billing_distance_meters: meters,
                amount_yen: Some(yen),
                effective_from: Some("2022-03-31T15:00:00Z".into()),
                effective_to: None,
            });
        }

        fn graph(&self) -> Graph {
            Graph {
                schema_version: 2,
                release_id: RELEASE.into(),
                vehicle_profile: PROFILE.into(),
                nodes: self.nodes.clone(),
                edges: self.edges.clone(),
                billing_pairs: self.pairs.clone(),
                forbidden_transitions: vec![],
                ramps: self.ramps.clone(),
                od_tariffs: self.tariffs.clone(),
            }
        }
    }

    fn request(min_minutes: u64, max_minutes: u64) -> SearchRequest {
        SearchRequest {
            request_id: format!("tier-{min_minutes}-{max_minutes}"),
            release_id: RELEASE.into(),
            origin_node_id: None,
            origin: Some(LatLng {
                lat: ORIGIN_LAT,
                lon: ORIGIN_LON,
            }),
            entry_ramp_id: None,
            exit_ramp_id: None,
            min_minutes,
            max_minutes,
            vehicle_profile: PROFILE.into(),
            pricing_at: PRICING_AT.into(),
        }
    }

    fn entry_ramps(result: &shutoko_routing_core::SearchResult) -> Vec<Option<String>> {
        result
            .candidates
            .iter()
            .map(|c| c.entry().ramp_id.clone())
            .collect()
    }

    fn exit_ramps(result: &shutoko_routing_core::SearchResult) -> Vec<Option<String>> {
        result
            .candidates
            .iter()
            .map(|c| c.exit().ramp_id.clone())
            .collect()
    }

    /// Two GeneralEntry ramps share one snapped node. The lexicographically
    /// smaller ramp ID must win the tie, and the farther entry must not be
    /// reached while the nearest tier already yields a candidate.
    #[test]
    fn tiers_order_by_distance_then_stable_ramp_id() {
        let mut world = World::new();
        let near_exit = world.exit(
            "ramp:t:a-exit",
            "fac:t:a",
            "tA",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );
        let _ = near_exit;
        world.entry(
            "ramp:t:a-entry-1",
            "fac:t:a",
            "sA",
            35.7010,
            ORIGIN_LON,
            "L1",
        );
        world.entry(
            "ramp:t:a-entry-2",
            "fac:t:a",
            "sA",
            35.7010,
            ORIGIN_LON,
            "L1",
        );
        let far_exit = world.exit(
            "ramp:t:b-exit",
            "fac:t:b",
            "tB",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );
        let _ = far_exit;
        world.entry("ramp:t:b-entry", "fac:t:b", "sB", 35.7030, ORIGIN_LON, "L1");

        let result = search(&world.graph(), &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert!(!result.candidates.is_empty());
        assert_eq!(
            result.candidates[0].entry().ramp_id.as_deref(),
            Some("ramp:t:a-entry-1"),
            "the lex-smaller ramp at the nearest snap node must win"
        );
        assert_eq!(result.candidates[0].snapped_origin().node_id, "sA");
        assert!(
            !entry_ramps(&result)
                .iter()
                .any(|id| id.as_deref() == Some("ramp:t:b-entry")),
            "farther tier must not be selected while the nearest tier has a candidate"
        );
    }

    /// A Verified entry tier evaluates only its own pair exits (priced path),
    /// never the unknown-toll dynamic exit attached to the same facility.
    #[test]
    fn verified_entry_uses_only_its_pair_exit() {
        let mut world = World::new();
        let entry_edge = world.entry("ramp:t:v-entry", "fac:t:v", "sV", 35.7010, ORIGIN_LON, "L1");
        let pair_exit_edge = world.exit(
            "ramp:t:v-exit",
            "fac:t:v",
            "tV",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L1",
        );
        // Same facility, but a different (non-verified) mainline node: it would
        // be dynamically usable if the verified tier mixed cohorts.
        world.exit(
            "ramp:t:v-side-exit",
            "fac:t:v",
            "tVS",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L3",
        );
        world.verified_pair(
            "bp:t:verified",
            &entry_edge,
            &pair_exit_edge,
            "L1",
            "ramp:t:v-entry",
            "ramp:t:v-exit",
            "V入口",
            "V出口",
        );

        let result = search(&world.graph(), &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert_eq!(result.candidates.len(), 1);
        let candidate = legacy(&result.candidates[0]);
        assert_eq!(candidate.exit.ramp_id.as_deref(), Some("ramp:t:v-exit"));
        assert_eq!(candidate.toll.amount_yen, Some(300));
        assert_eq!(candidate.toll.billing_pair_id, "bp:t:verified");
        assert_eq!(result.ranking_mode, "time_per_yen");
        assert_eq!(candidate.entry.name.as_deref(), Some("V入口"));
        assert_eq!(candidate.exit.name.as_deref(), Some("V出口"));
        assert!(
            !exit_ramps(&result)
                .iter()
                .any(|id| id.as_deref() == Some("ramp:t:v-side-exit")),
            "a verified tier must not emit dynamic unknown-toll candidates"
        );
    }

    /// With no Verified pair, at most two same-facility exits by stable ramp ID
    /// are evaluated; the third is never used.
    #[test]
    fn same_facility_exits_are_bounded_and_stable() {
        let mut world = World::new();
        world.entry("ramp:t:c-entry", "fac:t:c", "sC", 35.7010, ORIGIN_LON, "L1");
        world.exit(
            "ramp:t:c-exit-1",
            "fac:t:c",
            "tC1",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );
        world.exit(
            "ramp:t:c-exit-2",
            "fac:t:c",
            "tC2",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L3",
        );
        world.exit(
            "ramp:t:c-exit-3",
            "fac:t:c",
            "tC3",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );

        let result = search(&world.graph(), &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert!(!result.candidates.is_empty());
        assert!(result.candidates.len() <= 2);
        assert_eq!(
            result.candidates[0].exit().ramp_id.as_deref(),
            Some("ramp:t:c-exit-1"),
            "stable ID order must pick the lex-smaller exit first"
        );
        for id in exit_ramps(&result) {
            let id = id.expect("dynamic candidates carry an exit ramp id");
            assert!(
                id == "ramp:t:c-exit-1" || id == "ramp:t:c-exit-2",
                "third same-facility exit must not be evaluated: {id}"
            );
        }
    }

    /// A same-facility exit that shares the entry's mainline node is degenerate
    /// and must be rejected in favour of the valid exit.
    #[test]
    fn degenerate_same_mainline_exit_is_rejected() {
        let mut world = World::new();
        world.entry("ramp:t:g-entry", "fac:t:g", "sG", 35.7010, ORIGIN_LON, "L1");
        world.exit(
            "ramp:t:g-deg-exit",
            "fac:t:g",
            "tGdeg",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L1",
        );
        world.exit(
            "ramp:t:g-ok-exit",
            "fac:t:g",
            "tGok",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );

        let result = search(&world.graph(), &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert!(!result.candidates.is_empty());
        for id in exit_ramps(&result) {
            assert_eq!(
                id.as_deref(),
                Some("ramp:t:g-ok-exit"),
                "the same-mainline degenerate exit must never be used"
            );
        }
    }

    /// An entry whose facility has no exit falls back to the distinct exits
    /// referenced by existing Verified BillingPairs.
    #[test]
    fn verified_pair_exit_is_used_as_fallback() {
        let mut world = World::new();
        world.entry("ramp:t:d-entry", "fac:t:d", "sD", 35.7010, ORIGIN_LON, "L1");
        let other_entry_edge = world.entry(
            "ramp:t:d-other-entry",
            "fac:t:d-other",
            "sDother",
            35.7300,
            ORIGIN_LON,
            "L2",
        );
        let fallback_exit_edge = world.exit(
            "ramp:t:d-fallback-exit",
            "fac:t:d-other",
            "tDfallback",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );
        world.verified_pair(
            "bp:t:fallback",
            &other_entry_edge,
            &fallback_exit_edge,
            "L2",
            "ramp:t:d-other-entry",
            "ramp:t:d-fallback-exit",
            "Other入口",
            "Other出口",
        );

        let result = search(&world.graph(), &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert_eq!(
            result.candidates[0].entry().ramp_id.as_deref(),
            Some("ramp:t:d-entry"),
            "the nearest dynamic tier must win"
        );
        assert_eq!(
            result.candidates[0].exit().ramp_id.as_deref(),
            Some("ramp:t:d-fallback-exit"),
            "fallback exit comes from the Verified pair ledger"
        );
        assert_eq!(topology_only(&result.candidates[0]).toll.amount_yen, None);
        assert_eq!(result.ranking_mode, "shutoko_time");
    }

    /// A nearest tier that is fully evaluated with no candidate (a dead-end
    /// mainline with no cycle) falls through to a farther tier.
    #[test]
    fn fully_evaluated_no_candidate_tier_falls_through() {
        let mut world = World::new();
        world.node("D1", 35.7005, 139.7000);
        world.node("D2", 35.7010, 139.7000);
        world.shutoko("d12", "D1", "D2", 120, 1_000);
        world.entry(
            "ramp:t:dead-entry",
            "fac:t:dead",
            "sDead",
            35.7010,
            ORIGIN_LON,
            "D1",
        );
        world.exit(
            "ramp:t:dead-exit",
            "fac:t:dead",
            "tDead",
            ORIGIN_LAT,
            ORIGIN_LON,
            "D2",
        );
        world.entry(
            "ramp:t:live-entry",
            "fac:t:live",
            "sLive",
            35.7030,
            ORIGIN_LON,
            "L1",
        );
        world.exit(
            "ramp:t:live-exit",
            "fac:t:live",
            "tLive",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );

        let result = search(&world.graph(), &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert_eq!(
            result.candidates[0].entry().ramp_id.as_deref(),
            Some("ramp:t:live-entry"),
            "the fully evaluated no-candidate nearest tier must fall through"
        );
    }

    /// A fully evaluated no-candidate tier (a dead-end mainline) falls through
    /// to a farther tier. When the shared Budget is then exhausted while
    /// evaluating that farther tier, the search fails closed with SEARCH_LIMIT
    /// instead of returning an empty `NO_LOOP`/`TIME_WINDOW` or silently
    /// selecting the farther entry. This fixes the earlier version of the test
    /// that truncated on the very first tier and never exercised the exact
    /// fall-through boundary (review V5-04).
    #[test]
    fn budget_exhaustion_on_later_tier_after_fully_evaluated_tier() {
        let mut world = World::base(false);
        // ── Tier 0 (nearest, ~111 m): dead-end D1 → D2 with a same-facility
        //    exit. It is fully evaluated (forward +1, reverse +1, no cycle) and
        //    yields no legal loop, so it must fall through. ──
        world.node("D1", 35.7005, 139.7000);
        world.node("D2", 35.7010, 139.7000);
        world.shutoko("d12", "D1", "D2", 120, 1_000);
        world.entry(
            "ramp:t:near-entry",
            "fac:t:near",
            "sNear",
            35.7010,
            ORIGIN_LON,
            "D1",
        );
        world.exit(
            "ramp:t:near-exit",
            "fac:t:near",
            "tNear",
            ORIGIN_LAT,
            ORIGIN_LON,
            "D2",
        );
        // ── Tier 1 (farther, ~1.1 km): a 20-edge dead-end chain whose forward
        //    tree cannot fit in the remaining Budget, so its charge truncates
        //    the search on the second tier. ──
        world.entry(
            "ramp:t:far-entry",
            "fac:t:far",
            "sFar",
            35.7100,
            ORIGIN_LON,
            "B0",
        );
        world.node("B0", 35.7100, 139.7000);
        for index in 0..20usize {
            let next = index + 1;
            world.node(
                &format!("B{next}"),
                35.7100 + (next as f64) * 0.000_01,
                139.7000,
            );
            world.shutoko(
                &format!("b{index}"),
                &format!("B{index}"),
                &format!("B{next}"),
                120,
                1_000,
            );
        }
        world.exit(
            "ramp:t:far-exit",
            "fac:t:far",
            "tFar",
            ORIGIN_LAT,
            ORIGIN_LON,
            "B20",
        );

        // Tier 0 consumes exactly 4 expanded states (2 forward + 2 reverse);
        // tier 1's forward tree needs 21 and cannot fit in the remaining 2.
        let limits = SearchLimits {
            max_expanded_states: 6,
            ..SearchLimits::default()
        };
        let result = search(&world.graph(), &request(30, 60), &limits).unwrap();
        assert_eq!(result.status, "truncated", "reason={:?}", result.reason);
        assert_eq!(result.reason.as_deref(), Some("SEARCH_LIMIT"));
        assert!(result.candidates.is_empty());
        assert_eq!(
            result.expanded_states, 6,
            "the shared Budget is charged to its ceiling exactly once"
        );
        assert!(
            result.min_plan_seconds.is_none(),
            "a truncated tier cannot prove a minimum plan"
        );
        assert!(
            !entry_ramps(&result)
                .iter()
                .any(|id| id.as_deref() == Some("ramp:t:far-entry")),
            "a truncated farther tier must not silently select its entry"
        );
    }

    /// The nearest access tier owns the diagnostic: when it is fully evaluated
    /// and exposes a legal loop that the window rejects, the search reports
    /// `TIME_WINDOW` with the proven `minPlanSeconds` instead of falling through
    /// to a farther entry (review V5-01/V5-03 product requirement).
    #[test]
    fn nearest_tier_time_window_owns_the_diagnostic() {
        let mut world = World::base(false);
        // A 3 300 s loop (plan ~90 min) is legal but cannot fit a 30–60 minute
        // window, so the nearest tier is fully evaluated with no candidate.
        world.node("A1", 35.7005, ORIGIN_LON);
        world.node("A2", 35.7100, ORIGIN_LON);
        world.node("A3", 35.7050, 139.7100);
        world.shutoko("a12", "A1", "A2", 1_100, 10_000);
        world.shutoko("a23", "A2", "A3", 1_100, 10_000);
        world.shutoko("a31", "A3", "A1", 1_100, 10_000);
        world.entry(
            "ramp:t:near-entry",
            "fac:t:near",
            "sNear",
            35.7010,
            ORIGIN_LON,
            "A1",
        );
        world.exit(
            "ramp:t:near-exit",
            "fac:t:near",
            "tNear",
            ORIGIN_LAT,
            ORIGIN_LON,
            "A2",
        );
        // A farther entry that would expose the same loop must NOT be selected.
        world.entry(
            "ramp:t:far-entry",
            "fac:t:far",
            "sFar",
            35.7300,
            ORIGIN_LON,
            "A1",
        );
        world.exit(
            "ramp:t:far-exit",
            "fac:t:far",
            "tFar",
            ORIGIN_LAT,
            ORIGIN_LON,
            "A2",
        );

        let result = search(&world.graph(), &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(result.status, "no_candidates", "reason={:?}", result.reason);
        assert_eq!(result.reason.as_deref(), Some("TIME_WINDOW"));
        assert!(result.candidates.is_empty());
        assert!(
            result.min_plan_seconds.is_some(),
            "the fully evaluated nearest tier proves a minimum plan"
        );
        assert!(
            !entry_ramps(&result)
                .iter()
                .any(|id| id.as_deref() == Some("ramp:t:far-entry")),
            "a fully evaluated nearest tier must not fall through to a farther entry"
        );
    }

    /// Diagnostic truncation on the coordinate tier path (review V5-02): a
    /// candidate whose enumeration completed must still report
    /// `minPlanSeconds = null` when the wider product-cap diagnostic pass was
    /// cut short. Without the fix only the Verified-tier `ok` return checked
    /// `diagnostic_budget`, so an unprovable value leaked to the UI.
    #[test]
    fn diagnostic_truncation_nulls_min_plan_even_with_candidate() {
        let mut world = World::new();
        // A self-loop at the anchor that fits the 240-minute diagnostic pass but
        // not a 60-minute candidate window, plus a short in-window loop.
        world.shutoko("l-loop", "L1", "L1", 4_000, 10_000);
        let entry_edge = world.entry("ramp:t:v-entry", "fac:t:v", "sV", 35.7010, ORIGIN_LON, "L1");
        let exit_edge = world.exit(
            "ramp:t:v-exit",
            "fac:t:v",
            "tV",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L1",
        );
        world.verified_pair(
            "bp:t:v",
            &entry_edge,
            &exit_edge,
            "L1",
            "ramp:t:v-entry",
            "ramp:t:v-exit",
            "V入口",
            "V出口",
        );

        // The candidate pass completes with a candidate (5 expansions), but the
        // wider diagnostic pass needs a 6th and truncates.
        let limits = SearchLimits {
            max_expanded_states: 5,
            ..SearchLimits::default()
        };
        let result = search(&world.graph(), &request(1, 60), &limits).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert_eq!(result.candidates.len(), 1);
        assert!(
            result.min_plan_seconds.is_none(),
            "a diagnostic-truncated enumeration cannot prove the minimum plan (V5-02)"
        );
    }

    /// `max_pairs` bounds Verified pairs only; more dynamic entry tiers than
    /// `max_pairs` must not by itself truncate the search.
    #[test]
    fn dynamic_entry_count_is_independent_of_max_pairs() {
        let mut world = World::new();
        for index in 0..3 {
            let entry = format!("ramp:t:p-dyn-{index}");
            let exit = format!("ramp:t:p-dyn-{index}-exit");
            let branch = format!("P{index}");
            let branch_end = format!("P{index}b");
            let lat = 35.7004 + (index as f64) * 0.0002;
            world.node(&branch, lat, ORIGIN_LON);
            world.node(&branch_end, lat + 0.0005, ORIGIN_LON);
            world.shutoko(&format!("p{index}"), &branch, &branch_end, 120, 1_000);
            world.entry(
                &entry,
                &format!("fac:t:p-dyn-{index}"),
                &format!("s{index}"),
                lat,
                ORIGIN_LON,
                &branch,
            );
            world.exit(
                &exit,
                &format!("fac:t:p-dyn-{index}"),
                &format!("t{index}"),
                ORIGIN_LAT,
                ORIGIN_LON,
                &branch_end,
            );
        }
        let entry_edge = world.entry("ramp:t:p-entry", "fac:t:p", "sP", 35.7300, ORIGIN_LON, "L1");
        let exit_edge = world.exit(
            "ramp:t:p-exit",
            "fac:t:p",
            "tP",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L1",
        );
        world.verified_pair(
            "bp:t:max-pairs",
            &entry_edge,
            &exit_edge,
            "L1",
            "ramp:t:p-entry",
            "ramp:t:p-exit",
            "P入口",
            "P出口",
        );

        let limits = SearchLimits {
            max_pairs: 1,
            ..SearchLimits::default()
        };
        let result = search(&world.graph(), &request(30, 60), &limits).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert_eq!(
            result.candidates[0].entry().ramp_id.as_deref(),
            Some("ramp:t:p-entry")
        );
        assert_eq!(legacy(&result.candidates[0]).toll.amount_yen, Some(300));
    }

    fn cohort_world(with_tariff: bool) -> World {
        let mut world = World::new();
        let entry_edge = world.entry("ramp:t:h-entry", "fac:t:h", "sH", 35.7010, ORIGIN_LON, "L1");
        let _ = entry_edge;
        world.exit(
            "ramp:t:h-exit-1",
            "fac:t:h",
            "tH1",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );
        world.exit(
            "ramp:t:h-exit-2",
            "fac:t:h",
            "tH2",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L3",
        );
        if with_tariff {
            world.tariff("ramp:t:h-entry", "ramp:t:h-exit-1", 4_300, 300);
        }
        world
    }

    #[test]
    fn topology_only_candidates_never_enter_product_cohort() {
        let priced = search(
            &cohort_world(true).graph(),
            &request(30, 60),
            &SearchLimits::default(),
        )
        .unwrap();
        assert_eq!(priced.status, "ok", "reason={:?}", priced.reason);
        assert!(!priced.candidates.is_empty());
        assert_eq!(priced.ranking_mode, "shutoko_time");
        for candidate in &priced.candidates {
            let candidate = topology_only(candidate);
            assert_eq!(candidate.toll.amount_yen, Some(300));
            assert_eq!(
                candidate.tariff_status,
                shutoko_routing_core::TariffStatus::Priced
            );
            assert_eq!(candidate.exit.ramp_id.as_deref(), Some("ramp:t:h-exit-1"));
            assert!(candidate
                .reasons
                .iter()
                .all(|reason| reason == "TOPOLOGY_ONLY"));
        }

        let unpriced = search(
            &cohort_world(false).graph(),
            &request(30, 60),
            &SearchLimits::default(),
        )
        .unwrap();
        assert_eq!(unpriced.status, "ok", "reason={:?}", unpriced.reason);
        assert!(!unpriced.candidates.is_empty());
        assert_eq!(unpriced.ranking_mode, "shutoko_time");
        for candidate in &unpriced.candidates {
            let candidate = topology_only(candidate);
            assert_eq!(candidate.toll.amount_yen, None);
            assert_eq!(
                candidate.tariff_status,
                shutoko_routing_core::TariffStatus::Unpriced
            );
        }
    }

    /// `max_candidates` is applied inside the winning tier.
    #[test]
    fn tier_local_max_candidates_is_respected() {
        let mut world = World::new();
        world.entry("ramp:t:m-entry", "fac:t:m", "sM", 35.7010, ORIGIN_LON, "L1");
        world.exit(
            "ramp:t:m-exit-1",
            "fac:t:m",
            "tM1",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L2",
        );
        world.exit(
            "ramp:t:m-exit-2",
            "fac:t:m",
            "tM2",
            ORIGIN_LAT,
            ORIGIN_LON,
            "L3",
        );
        let limits = SearchLimits {
            max_candidates: 1,
            ..SearchLimits::default()
        };
        let result = search(&world.graph(), &request(30, 60), &limits).unwrap();
        assert_eq!(result.status, "ok", "reason={:?}", result.reason);
        assert_eq!(result.candidates.len(), 1);
    }

    /// Repeated searches of the same coordinate request serialize identically.
    #[test]
    fn repeated_coordinate_tier_json_is_deterministic() {
        let graph = cohort_world(true).graph();
        let first = search(&graph, &request(30, 60), &SearchLimits::default()).unwrap();
        let second = search(&graph, &request(30, 60), &SearchLimits::default()).unwrap();
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap(),
            "coordinate tier search must be byte-deterministic"
        );
    }
}
