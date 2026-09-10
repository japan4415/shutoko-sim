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
        result.expanded_states < 100_000,
        "expanded states must be below 100,000, got: {}",
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

/// 連結 5 ペアの探索契約定数
/// 各ペアの C1 一周計画時間が時間窓に収まる max_minutes とその根拠を明示。
struct ConnectedPairContract {
    pair_id: &'static str,
    max_minutes: u64,
    rationale: &'static str,
}

const CONNECTED_SEARCH_PAIRS: [ConnectedPairContract; 5] = [
    ConnectedPairContract {
        pair_id: "bp:c1-outer:kandabashi-takaracho",
        max_minutes: 60,
        rationale: "神田橋〜宝町（外回り）。C1 一周の実走行計画時間は約30分（base=1503s, plan=1803s）。max_minutes=60 の標準窓で自ペア候補が採択される。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-inner:takaracho-kandabashi",
        max_minutes: 60,
        rationale: "宝町〜神田橋（内回り）。C1 一周の実走行計画時間は約28.4分（base=1405s, plan=1705s）。max_minutes=60 の標準窓で自ペア候補が採択される。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-inner:kasumigaseki-shibakoen",
        max_minutes: 60,
        rationale: "霞が関〜芝公園（内回り）。C1 一周の実走行計画時間は約34.3分（base=1717s, plan=2060s）。max_minutes=60 の標準窓で自ペア候補が採択される。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-inner:shibakoen-shiodome",
        max_minutes: 30,
        rationale: "芝公園〜汐留（内回り）。実走行計画時間は約28.8分（base=1431s, plan=1731s）。max_minutes=60 では霞が関入口（kasumigaseki-shibakoen, 首都高1205s/300円）が time_per_yen 比率（1205/300 > 1136/300）により上位にランクインし、同一の内回り C1 ループであるため 80% Jaccard 類似度除外により芝公園入口側の候補が除外される。計画時間30分枠では遠隔の霞が関（plan=2464s ≈ 41分）が時間窓外となり、自ペア候補が採択される。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-outer:shibakoen-iikura",
        max_minutes: 60,
        rationale: "芝公園〜飯倉（外回り）。C1 一周の実走行計画時間は約26.6分（base=1298s, plan=1598s）。max_minutes=60 の標準窓で自ペア候補が採択される。",
    },
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
    for contract in CONNECTED_SEARCH_PAIRS {
        let pair = g
            .billing_pairs
            .iter()
            .find(|p| p.id == contract.pair_id)
            .unwrap_or_else(|| panic!("billing pair {} must exist in fixture", contract.pair_id));
        let entry_edge = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()];
        let origin_node_id = entry_edge.from.clone();

        let req_start = std::time::Instant::now();
        let request = SearchRequest {
            request_id: format!("req-loop-{}", pair.id),
            release_id: g.release_id.clone(),
            origin_node_id: origin_node_id.clone(),
            min_minutes: 15,
            max_minutes: contract.max_minutes,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };

        let result = search(&g, &request, &limits)
            .unwrap_or_else(|e| panic!("search must succeed for pair {}: {}", pair.id, e));

        eprintln!(
            "SEARCH for pair {} (origin={}, max_minutes={}): status={}, expanded={}, candidates_count={}, elapsed={:?}",
            pair.id,
            origin_node_id,
            contract.max_minutes,
            result.status,
            result.expanded_states,
            result.candidates.len(),
            req_start.elapsed()
        );

        assert_eq!(
            result.status, "ok",
            "search for connected pair {} must have status 'ok', got {} (rationale: {})",
            pair.id, result.status, contract.rationale
        );
        assert!(
            result.expanded_states < 100_000,
            "pair {}: expanded states must be below 100,000, got: {}",
            pair.id,
            result.expanded_states
        );

        let candidate = result
            .candidates
            .iter()
            .find(|c| c.toll.billing_pair_id == pair.id)
            .unwrap_or_else(|| {
                panic!(
                    "candidate with billing_pair_id {} must be found from origin {} with max_minutes={} ({})",
                    pair.id, origin_node_id, contract.max_minutes, contract.rationale
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

    // 2. OSM データ境界により一般道が切断されている 3 ペアの契約（実態に合わせる）
    // 既定 limits で truncated にならず、孤立成分と一般道連結性の実態を固定する。
    //
    // (a) bp:c1-outer:ginza-shibakoen:
    //     Ginza 外回り出口（n:254360532、13ノード孤立成分）など OSM 取得境界により
    //     一般道側が物理的に切断されており他入口へも到達不能なため status == "no_candidates"。
    {
        let pair_id = "bp:c1-outer:ginza-shibakoen";
        let pair = g.billing_pairs.iter().find(|p| p.id == pair_id).unwrap();
        let origin_node_id = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()]
            .from
            .clone();
        let request = SearchRequest {
            request_id: format!("req-loop-{}", pair_id),
            release_id: g.release_id.clone(),
            origin_node_id: origin_node_id.clone(),
            min_minutes: 15,
            max_minutes: 60,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = search(&g, &request, &limits).expect("search must succeed");
        assert_eq!(
            result.status, "no_candidates",
            "ginza-shibakoen must have status no_candidates due to isolated component"
        );
        assert!(result.candidates.is_empty());
        assert!(result.expanded_states < 100_000);
    }

    // (b) bp:c1-inner:daikancho-kasumigaseki:
    //     Daikancho 入口/出口（16ノード/61ノード孤立成分）が OSM 取得境界により孤立しているため
    //     一般道側が切断されており status == "no_candidates"。
    {
        let pair_id = "bp:c1-inner:daikancho-kasumigaseki";
        let pair = g.billing_pairs.iter().find(|p| p.id == pair_id).unwrap();
        let origin_node_id = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()]
            .from
            .clone();
        let request = SearchRequest {
            request_id: format!("req-loop-{}", pair_id),
            release_id: g.release_id.clone(),
            origin_node_id: origin_node_id.clone(),
            min_minutes: 15,
            max_minutes: 60,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = search(&g, &request, &limits).expect("search must succeed");
        assert_eq!(
            result.status, "no_candidates",
            "daikancho-kasumigaseki must have status no_candidates due to isolated component"
        );
        assert!(result.candidates.is_empty());
        assert!(result.expanded_states < 100_000);
    }

    // (c) bp:c1-outer:kasumigaseki-daikancho:
    //     Kasumigaseki 起点は一般道網に連結しているため他ペア候補（神田橋・芝公園等経由）により
    //     status == "ok" となるが、Daikancho 出口側が切断されているため自ペア候補は含まれない。
    {
        let pair_id = "bp:c1-outer:kasumigaseki-daikancho";
        let pair = g.billing_pairs.iter().find(|p| p.id == pair_id).unwrap();
        let origin_node_id = edge_map[pair.entry_to_anchor_edge_ids[0].as_str()]
            .from
            .clone();
        let request = SearchRequest {
            request_id: format!("req-loop-{}", pair_id),
            release_id: g.release_id.clone(),
            origin_node_id: origin_node_id.clone(),
            min_minutes: 15,
            max_minutes: 60,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = search(&g, &request, &limits).expect("search must succeed");
        assert_eq!(
            result.status, "ok",
            "kasumigaseki-daikancho must have status ok due to reachable other pairs"
        );
        assert!(
            !result
                .candidates
                .iter()
                .any(|c| c.toll.billing_pair_id == pair_id),
            "kasumigaseki-daikancho must not contain own-pair candidate due to isolated exit"
        );
        assert!(!result.candidates.is_empty());
        assert!(result.expanded_states < 100_000);
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

#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn test_eight_pairs_determinism_and_performance_table() {
    let g = real_graph();
    let graph_json = include_str!("../../../fixtures/generated/graph.json");
    let limits = SearchLimits::default();
    let edge_map: std::collections::HashMap<&str, &shutoko_routing_core::Edge> =
        g.edges.iter().map(|e| (e.id.as_str(), e)).collect();

    let mut pairs: Vec<_> = g.billing_pairs.iter().collect();
    pairs.sort_by(|a, b| a.id.cmp(&b.id));

    eprintln!("\n=== 8 PAIRS PERFORMANCE AND DETERMINISM (release) ===");
    eprintln!(
        "pair_id | origin | status | candidates | own_pair | expanded | time_ms | determinism_3x"
    );

    for p in pairs {
        let origin = edge_map[p.entry_to_anchor_edge_ids[0].as_str()]
            .from
            .clone();
        let max_minutes = if p.id == "bp:c1-inner:shibakoen-shiodome" {
            30
        } else {
            60
        };
        let req = SearchRequest {
            request_id: format!("det-req-{}", p.id),
            release_id: g.release_id.clone(),
            origin_node_id: origin.clone(),
            min_minutes: 15,
            max_minutes,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let req_json = serde_json::to_string(&req).unwrap();

        // 3 回 search_json を実行し、バイト完全一致（決定論）を確認
        let res1 = search_json(graph_json, &req_json, "{}").unwrap();
        let res2 = search_json(graph_json, &req_json, "{}").unwrap();
        let res3 = search_json(graph_json, &req_json, "{}").unwrap();
        assert_eq!(
            res1, res2,
            "search_json determinism check 1 vs 2 failed for {}",
            p.id
        );
        assert_eq!(
            res2, res3,
            "search_json determinism check 2 vs 3 failed for {}",
            p.id
        );

        let t0 = std::time::Instant::now();
        let res = search(&g, &req, &limits).unwrap();
        let elapsed = t0.elapsed();

        let own = res
            .candidates
            .iter()
            .any(|c| c.toll.billing_pair_id == p.id);
        eprintln!(
            "{} | {} | {} | {} | {} | {} | {:.2}ms | 3x_byte_identical_PASS",
            p.id,
            origin,
            res.status,
            res.candidates.len(),
            own,
            res.expanded_states,
            elapsed.as_secs_f64() * 1000.0
        );
    }
}
