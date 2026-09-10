use shutoko_routing_core::{search, search_json, Graph, SearchLimits, SearchRequest};
use std::collections::BTreeSet;

fn real_graph_str() -> &'static str {
    include_str!("../../../fixtures/generated/graph.json")
}

fn real_graph() -> Graph {
    serde_json::from_str(real_graph_str())
        .expect("Graph deserialization must succeed with deny_unknown_fields")
}

#[test]
fn real_graph_deserialization_and_schema_validation() {
    let g = real_graph();
    assert_eq!(g.schema_version, 1);
    assert_eq!(g.release_id, "c1-real-v1");
    assert_eq!(g.vehicle_profile, "passenger-car-etc");
    assert!(!g.nodes.is_empty(), "nodes must not be empty");
    assert!(!g.edges.is_empty(), "edges must not be empty");
    assert_eq!(
        g.billing_pairs.len(),
        1,
        "exactly 1 billing pair expected in fixture"
    );

    let pair = &g.billing_pairs[0];
    assert_eq!(pair.id, "bp:c1-outer:kandabashi-takaracho");
    assert_eq!(pair.entry_id, "e:w92243921:0:f");
    assert_eq!(pair.exit_id, "e:w297864314:11:f");
    assert_eq!(pair.anchor_node_id, "n:499831338");
    assert_eq!(
        pair.prices.len(),
        2,
        "billingPairs[0].prices must have 2 records"
    );
    assert_eq!(pair.prices[0].amount_yen, 300);
    assert_eq!(pair.prices[0].effective_from, "2022-03-31T15:00:00Z");
    assert_eq!(
        pair.prices[0].effective_to.as_deref(),
        Some("2026-09-30T15:00:00Z")
    );
    assert_eq!(pair.prices[1].amount_yen, 300);
    assert_eq!(pair.prices[1].effective_from, "2026-09-30T15:00:00Z");
    assert_eq!(pair.prices[1].effective_to, None);

    // Verify billing pair is a simple path (no node revisited on the direct entry-to-exit path)
    let edge_map: std::collections::HashMap<&str, &shutoko_routing_core::Edge> =
        g.edges.iter().map(|e| (e.id.as_str(), e)).collect();

    let mut seen_nodes = BTreeSet::new();
    let first_entry = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()];
    seen_nodes.insert(first_entry.from.as_str());

    for eid in &pair.entry_to_anchor_edge_ids {
        let e = edge_map[eid.as_str()];
        assert!(
            seen_nodes.insert(e.to.as_str()),
            "node {} revisited in entry_to_anchor",
            e.to
        );
    }
    for eid in &pair.anchor_to_exit_edge_ids {
        let e = edge_map[eid.as_str()];
        assert!(
            seen_nodes.insert(e.to.as_str()),
            "node {} revisited in anchor_to_exit",
            e.to
        );
    }
}

#[test]
fn real_graph_routing_core_search_returns_candidates() {
    let g = real_graph();

    let request = SearchRequest {
        request_id: "req-c1-kandabashi-1".into(),
        release_id: "c1-real-v1".into(),
        origin_node_id: "n:1070862943".into(), // Kandabashi surface street node
        min_minutes: 15,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };

    let limits = SearchLimits::default();
    let result = search(&g, &request, &limits).expect("search must succeed on real graph");

    assert!(
        result.status == "ok" || result.status == "truncated",
        "search status must be ok or truncated, got: {}",
        result.status
    );
    assert_eq!(result.request_id, "req-c1-kandabashi-1");
    assert_eq!(result.release_id, "c1-real-v1");
    assert!(
        !result.candidates.is_empty(),
        "expected at least 1 candidate route from real graph"
    );

    let c = &result.candidates[0];
    assert!(
        !c.edge_ids.is_empty(),
        "candidate edgeIds must be non-empty"
    );

    // Verify non-empty loop around C1 mainline
    assert!(
        !c.r#loop.edge_ids.is_empty(),
        "loop edgeIds must be non-empty"
    );
    assert!(
        c.r#loop.duration_seconds > 0,
        "loop duration must be positive"
    );
    assert!(c.r#loop.distance_meters > 10000, "C1 loop should be > 10km");

    // Verify durations and distance
    assert!(c.duration.base_seconds > 0);
    assert!(c.duration.shutoko_seconds > 0);
    assert!(c.duration.return_seconds > 0);
    assert!(c.distance_meters > 0);
    assert_eq!(result.ranking_mode, "time_per_yen");
    assert!(c.shutoko_distance_meters > 0);

    // Verify toll record for pre-revision date (amount_yen: 300, valid interval, time_per_yen ranking)
    assert_eq!(c.toll.billing_pair_id, "bp:c1-outer:kandabashi-takaracho");
    assert_eq!(c.toll.charged_section_count, 1);
    assert_eq!(c.toll.amount_yen, Some(300));
    assert_eq!(
        c.toll.effective_from.as_deref(),
        Some("2022-03-31T15:00:00Z")
    );
    assert_eq!(c.toll.effective_to.as_deref(), Some("2026-09-30T15:00:00Z"));
}

#[test]
fn real_graph_pricing_intervals_and_ranking_transitions() {
    let g = real_graph();
    let limits = SearchLimits::default();

    let make_request = |pricing_at: &str| SearchRequest {
        request_id: format!("req-{}", pricing_at),
        release_id: "c1-real-v1".into(),
        origin_node_id: "n:1070862943".into(),
        min_minutes: 15,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: pricing_at.into(),
    };

    // 1. Boundary: 1 second before 2026-10-01 revision (2026-09-30T14:59:59Z) -> pre-revision record
    let res_before_boundary = search(&g, &make_request("2026-09-30T14:59:59Z"), &limits)
        .expect("search must succeed at boundary-1s");
    assert!(res_before_boundary.status == "ok" || res_before_boundary.status == "truncated");
    assert_eq!(res_before_boundary.ranking_mode, "time_per_yen");
    let c = &res_before_boundary.candidates[0];
    assert_eq!(c.toll.amount_yen, Some(300));
    assert_eq!(
        c.toll.effective_from.as_deref(),
        Some("2022-03-31T15:00:00Z")
    );
    assert_eq!(c.toll.effective_to.as_deref(), Some("2026-09-30T15:00:00Z"));

    // 2. Exactly at revision boundary (2026-09-30T15:00:00Z) -> post-revision record
    let res_at_boundary = search(&g, &make_request("2026-09-30T15:00:00Z"), &limits)
        .expect("search must succeed at boundary");
    assert!(res_at_boundary.status == "ok" || res_at_boundary.status == "truncated");
    assert_eq!(res_at_boundary.ranking_mode, "time_per_yen");
    let c = &res_at_boundary.candidates[0];
    assert_eq!(c.toll.amount_yen, Some(300));
    assert_eq!(
        c.toll.effective_from.as_deref(),
        Some("2026-09-30T15:00:00Z")
    );
    assert_eq!(c.toll.effective_to, None);

    // 3. Post-revision (e.g. 2026-10-01T00:00:00Z) -> post-revision record
    let res_post = search(&g, &make_request("2026-10-01T00:00:00Z"), &limits)
        .expect("search must succeed post-revision");
    assert!(res_post.status == "ok" || res_post.status == "truncated");
    assert_eq!(res_post.ranking_mode, "time_per_yen");
    let c = &res_post.candidates[0];
    assert_eq!(c.toll.amount_yen, Some(300));
    assert_eq!(
        c.toll.effective_from.as_deref(),
        Some("2026-09-30T15:00:00Z")
    );
    assert_eq!(c.toll.effective_to, None);

    // 4. Prior to 2022-03-31T15:00:00Z (e.g. 2022-01-01T00:00:00Z) -> unknown toll, fall back to shutoko_time
    let res_prior = search(&g, &make_request("2022-01-01T00:00:00Z"), &limits)
        .expect("search must succeed prior to tariff start");
    assert!(res_prior.status == "ok" || res_prior.status == "truncated");
    assert_eq!(res_prior.ranking_mode, "shutoko_time");
    let c = &res_prior.candidates[0];
    assert_eq!(c.toll.amount_yen, None);
    assert_eq!(c.toll.effective_from, None);
    assert_eq!(c.toll.effective_to, None);
}

#[test]
fn real_graph_search_json_wasm_contract_parity() {
    let graph_json = real_graph_str();
    let request_json = serde_json::json!({
        "requestId": "req-c1-json",
        "releaseId": "c1-real-v1",
        "originNodeId": "n:1070862943",
        "minMinutes": 15,
        "maxMinutes": 60,
        "vehicleProfile": "passenger-car-etc",
        "pricingAt": "2026-09-10T00:00:00Z"
    })
    .to_string();

    let limits_json = "{}";
    let res_str = search_json(graph_json, &request_json, limits_json)
        .expect("search_json must succeed with real graph");

    let val: serde_json::Value = serde_json::from_str(&res_str).unwrap();
    let st = val["status"].as_str().unwrap();
    assert!(
        st == "ok" || st == "truncated",
        "JSON search status must be ok or truncated, got: {}",
        st
    );
    assert_eq!(val["rankingMode"].as_str().unwrap(), "time_per_yen");
    let candidates = val["candidates"].as_array().unwrap();
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0]["toll"]["amountYen"].as_u64(), Some(300));
}
