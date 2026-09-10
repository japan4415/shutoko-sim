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
        8,
        "exactly 8 billing pairs expected in fixture"
    );

    let edge_map: std::collections::HashMap<&str, &shutoko_routing_core::Edge> =
        g.edges.iter().map(|e| (e.id.as_str(), e)).collect();

    for pair in &g.billing_pairs {
        assert_eq!(
            pair.status,
            shutoko_routing_core::VerificationStatus::Verified,
            "billing pair {} must be verified",
            pair.id
        );
        assert_eq!(
            pair.prices.len(),
            2,
            "billing pair {} prices must have 2 records",
            pair.id
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

        // Verify billing pair is a simple path (no node revisited on direct entry-to-exit path)
        let mut seen_nodes = BTreeSet::new();
        let first_entry = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()];
        seen_nodes.insert(first_entry.from.as_str());

        for eid in &pair.entry_to_anchor_edge_ids {
            let e = edge_map[eid.as_str()];
            assert!(
                seen_nodes.insert(e.to.as_str()),
                "pair {}: node {} revisited in entry_to_anchor",
                pair.id,
                e.to
            );
        }
        for eid in &pair.anchor_to_exit_edge_ids {
            let e = edge_map[eid.as_str()];
            assert!(
                seen_nodes.insert(e.to.as_str()),
                "pair {}: node {} revisited in anchor_to_exit",
                pair.id,
                e.to
            );
        }
    }

    // Verify manifest unverifiedSections has no rejected elements
    let manifest_str = include_str!("../../../fixtures/generated/manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(manifest_str).expect("manifest.json must deserialize");
    let unverified = manifest["unverifiedSections"]
        .as_array()
        .expect("manifest.unverifiedSections must be array");
    for item in unverified {
        let s = item
            .as_str()
            .expect("unverifiedSections item must be string");
        assert!(
            !s.contains("rejected:"),
            "manifest.unverifiedSections must not contain rejected elements, got: {}",
            s
        );
    }
}

#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
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
    let start = std::time::Instant::now();
    let result = search(&g, &request, &limits).expect("search must succeed on real graph");
    eprintln!(
        "Kandabashi search took {:?}, expanded: {}, candidates: {}",
        start.elapsed(),
        result.expanded_states,
        result.candidates.len()
    );

    assert_eq!(result.status, "ok", "search status must be ok");
    assert!(
        result.expanded_states < 50_000,
        "expanded states must be well below 100,000, got: {}",
        result.expanded_states
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
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
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
    assert_eq!(res_before_boundary.status, "ok");
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
    assert_eq!(res_at_boundary.status, "ok");
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
    assert_eq!(res_post.status, "ok");
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
    assert_eq!(res_prior.status, "ok");
    assert_eq!(res_prior.ranking_mode, "shutoko_time");
    let c = &res_prior.candidates[0];
    assert_eq!(c.toll.amount_yen, None);
    assert_eq!(c.toll.effective_from, None);
    assert_eq!(c.toll.effective_to, None);
}

#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
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
    assert_eq!(st, "ok", "JSON search status must be ok, got: {}", st);
    assert_eq!(val["rankingMode"].as_str().unwrap(), "time_per_yen");
    let candidates = val["candidates"].as_array().unwrap();
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0]["toll"]["amountYen"].as_u64(), Some(300));
}

/// 代表ペア定数: 探索コアの既定上限では全ペアの候補が得られないため、
/// 一般道網が連結している課金ペア（5ペア）:
/// 既定 SearchLimits::default()（max_expanded_states: 100,000）で
/// 入口エッジ from ノードを起点とする探索により自ペア Candidate（300円・10km以上・status == "ok"）が得られる。
const CONNECTED_SEARCH_PAIR_IDS: [&str; 5] = [
    "bp:c1-outer:kandabashi-takaracho",
    "bp:c1-inner:takaracho-kandabashi",
    "bp:c1-inner:kasumigaseki-shibakoen",
    "bp:c1-inner:shibakoen-shiodome",
    "bp:c1-outer:shibakoen-iikura",
];

/// 一般道網が OSM 取得境界により切断されている課金ペア（3ペア、analyze-002 F7）:
/// Shibakoen 出口（外回り n:254360532、13ノード孤立成分）や
/// Daikancho 出口/入口（61ノード/16ノード孤立成分）など、
/// OSM 取得範囲（fetch-osm.sh の bbox / リンク走査境界）に起因して一般道側が物理的に切断されているため、
/// 候補なし（status == "no_candidates"）となる。
/// 既定 limits で truncated にならず、expanded_states も上限を大きく下回ることを固定する。
const DISCONNECTED_SEARCH_PAIR_IDS: [&str; 3] = [
    "bp:c1-outer:ginza-shibakoen",
    "bp:c1-outer:kasumigaseki-daikancho",
    "bp:c1-inner:daikancho-kasumigaseki",
];

#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn test_all_billing_pairs_search_and_connectivity_contract() {
    let g = real_graph();
    let limits = SearchLimits::default();
    let edge_map: std::collections::HashMap<&str, &shutoko_routing_core::Edge> =
        g.edges.iter().map(|e| (e.id.as_str(), e)).collect();

    let start_total = std::time::Instant::now();

    // 1. 一般道が連結している 5 ペアの探索契約
    for id in CONNECTED_SEARCH_PAIR_IDS {
        let pair = g
            .billing_pairs
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("billing pair {} must exist in fixture", id));
        let entry_edge = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()];
        let origin_node_id = entry_edge.from.clone();

        let max_minutes = if id == "bp:c1-inner:shibakoen-shiodome" {
            // Shibakoen-Shiodome の実走行計画時間は約28.8分（base=1431s, buffer=300s）。
            // maxMinutes=60 の場合、一般道で約14分（848s）離れた霞が関入口（kasumigaseki-shibakoen, 首都高1205s/300円）が
            // time_per_yen 比率（1205/300 > 1136/300）により上位にランクインし、同一の内回り C1 ループであるため
            // 80% Jaccard 類似度除外により芝公園入口側の候補が除外される。
            // 計画時間30分枠では遠隔の霞が関（plan=2464s ≈ 41分）が時間窓外となり、自ペア候補が採択される。
            30
        } else {
            60
        };

        let req_start = std::time::Instant::now();
        let request = SearchRequest {
            request_id: format!("req-loop-{}", pair.id),
            release_id: g.release_id.clone(),
            origin_node_id: origin_node_id.clone(),
            min_minutes: 15,
            max_minutes,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };

        let result = search(&g, &request, &limits)
            .unwrap_or_else(|e| panic!("search must succeed for pair {}: {}", pair.id, e));

        eprintln!(
            "SEARCH for pair {} (origin={}): status={}, expanded={}, candidates_count={}, elapsed={:?}",
            pair.id,
            origin_node_id,
            result.status,
            result.expanded_states,
            result.candidates.len(),
            req_start.elapsed()
        );

        assert_eq!(
            result.status, "ok",
            "search for connected pair {} must have status 'ok', got {}",
            pair.id, result.status
        );
        assert!(
            result.expanded_states < 50_000,
            "pair {}: expanded states must be well below 100,000, got: {}",
            pair.id,
            result.expanded_states
        );

        let candidate = result
            .candidates
            .iter()
            .find(|c| c.toll.billing_pair_id == pair.id)
            .unwrap_or_else(|| {
                panic!(
                    "candidate with billing_pair_id {} must be found from origin {}",
                    pair.id, origin_node_id
                )
            });

        assert_eq!(
            candidate.toll.amount_yen,
            Some(300),
            "toll amount for pair {} must be 300 yen",
            pair.id
        );
        assert!(
            candidate.r#loop.distance_meters > 10000,
            "pair {}: C1 loop distance must be > 10km, got {}m",
            pair.id,
            candidate.r#loop.distance_meters
        );
    }

    // 2. OSM データ境界により一般道が切断されている 3 ペアの契約
    // 既定 limits で truncated にならず（expanded_states が上限を大きく下回る）、
    // 自ペアの Candidate は生成されないことを固定。
    // 起点周辺に他の連結入口が存在する場合は他ペア候補により status == "ok" となり得るが、
    // 孤立成分内（Ginza 外回り等）では status == "no_candidates" となる。
    for id in DISCONNECTED_SEARCH_PAIR_IDS {
        let pair = g
            .billing_pairs
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("billing pair {} must exist in fixture", id));
        let entry_edge = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()];
        let origin_node_id = entry_edge.from.clone();

        let req_start = std::time::Instant::now();
        let request = SearchRequest {
            request_id: format!("req-loop-{}", pair.id),
            release_id: g.release_id.clone(),
            origin_node_id: origin_node_id.clone(),
            min_minutes: 15,
            max_minutes: 60,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };

        let result = search(&g, &request, &limits)
            .unwrap_or_else(|e| panic!("search must succeed for pair {}: {}", pair.id, e));

        eprintln!(
            "DISCONNECTED SEARCH for pair {} (origin={}): status={}, expanded={}, candidates_count={}, elapsed={:?}",
            pair.id,
            origin_node_id,
            result.status,
            result.expanded_states,
            result.candidates.len(),
            req_start.elapsed()
        );

        assert_ne!(
            result.status, "truncated",
            "disconnected pair {} must not be truncated, got: {}",
            pair.id, result.status
        );
        assert!(
            result.expanded_states < 50_000,
            "pair {}: expanded states must be well below 100,000, got: {}",
            pair.id,
            result.expanded_states
        );
        assert!(
            !result
                .candidates
                .iter()
                .any(|c| c.toll.billing_pair_id == pair.id),
            "pair {} must not produce candidate due to isolated component in OSM data",
            pair.id
        );
    }

    // 全 8 ペアの fixture 上のメタデータ整合性（verified・料金レコード・経路ワイヤリング）を確認
    for pair in &g.billing_pairs {
        assert_eq!(
            pair.status,
            shutoko_routing_core::VerificationStatus::Verified,
            "pair {} must be verified",
            pair.id
        );
        assert_eq!(
            pair.prices.len(),
            2,
            "pair {} must have 2 price records",
            pair.id
        );
        assert!(
            pair.prices.iter().all(|p| p.amount_yen == 300),
            "pair {} prices must be 300 yen",
            pair.id
        );
        assert!(!pair.entry_to_anchor_edge_ids.is_empty());
        assert!(!pair.anchor_to_exit_edge_ids.is_empty());
    }

    let total_elapsed = start_total.elapsed();
    eprintln!(
        "all 8 billing pairs search contract total elapsed: {:?}",
        total_elapsed
    );
    assert!(
        total_elapsed < std::time::Duration::from_secs(60),
        "total search time for all 8 pairs must be under 60 seconds, took {:?}",
        total_elapsed
    );
}
