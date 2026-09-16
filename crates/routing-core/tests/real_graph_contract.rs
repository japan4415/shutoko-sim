use shutoko_routing_core::{search, search_json, Graph, LatLng, SearchLimits, SearchRequest};
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
    assert_eq!(g.schema_version, 2);
    assert_eq!(g.release_id, "all-real-v1");
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

    assert_eq!(
        g.billing_pairs
            .iter()
            .filter(|p| p.status == shutoko_routing_core::VerificationStatus::Verified)
            .count(),
        2
    );
    assert_eq!(
        g.billing_pairs
            .iter()
            .filter(|p| p.status == shutoko_routing_core::VerificationStatus::Unverified)
            .count(),
        6
    );

    for pair in &g.billing_pairs {
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
        if pair.status == shutoko_routing_core::VerificationStatus::Verified {
            assert!(
                pair.entry_ramp_id.is_some() && pair.exit_ramp_id.is_some(),
                "verified billing pair {} must have non-null ramp IDs",
                pair.id
            );
        }

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

    for id in [
        "bp:c1-inner:daikancho-kasumigaseki",
        "bp:c1-inner:shibakoen-shiodome",
        "bp:c1-outer:ginza-shibakoen",
    ] {
        assert_eq!(
            g.billing_pairs
                .iter()
                .find(|pair| pair.id == id)
                .unwrap_or_else(|| panic!("missing billing pair {id}"))
                .status,
            shutoko_routing_core::VerificationStatus::Unverified,
            "billing pair {id} must remain unverified until both endpoints uniquely resolve"
        );
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
        release_id: "all-real-v1".into(),
        origin_node_id: Some("n:1070862943".into()),
        origin: None, // Kandabashi surface street node
        entry_ramp_id: None,
        exit_ramp_id: None,
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
    assert_eq!(result.release_id, "all-real-v1");
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
        release_id: "all-real-v1".into(),
        origin_node_id: Some("n:1070862943".into()),
        origin: None,
        entry_ramp_id: None,
        exit_ramp_id: None,
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
        "releaseId": "all-real-v1",
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

#[test]
#[ignore = "full-network real-graph search; run with --release -- --ignored (CI does)"]
fn explicit_full_network_ramps_route_deterministically_within_budget() {
    let g = real_graph();
    let limits = SearchLimits::default();
    let pairs = [
        (
            "C1",
            "ramp:c1-outer:kandabashi-entry",
            "ramp:c1-outer:takaracho-exit",
        ),
        (
            "C2",
            "ramp:c2-outer:gotanda-entry",
            "ramp:c2-inner:gotanda-exit",
        ),
        (
            "radial-3",
            "ramp:3-inbound:shibuya-entry",
            "ramp:3-outbound:yoga-exit",
        ),
        (
            "kanagawa-K1",
            "ramp:k1-inbound:daishi-entry",
            "ramp:k1-inbound:minato-mirai-exit",
        ),
        (
            "saitama-S1",
            "ramp:s1-inbound:araijuku-entry",
            "ramp:s1-outbound:shikahamabashi-exit",
        ),
    ];
    for (area, entry, exit) in pairs {
        let request = SearchRequest {
            request_id: format!("explicit-{area}"),
            release_id: g.release_id.clone(),
            origin_node_id: None,
            origin: None,
            entry_ramp_id: Some(entry.into()),
            exit_ramp_id: Some(exit.into()),
            min_minutes: 1,
            max_minutes: 240,
            vehicle_profile: g.vehicle_profile.clone(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let started = std::time::Instant::now();
        let first = search(&g, &request, &limits)
            .unwrap_or_else(|error| panic!("{area} explicit search failed: {error}"));
        let elapsed = started.elapsed();
        let second = search(&g, &request, &limits)
            .unwrap_or_else(|error| panic!("{area} repeat search failed: {error}"));
        eprintln!(
            "{area}: status={}, reason={:?}, expanded={}, candidates={}, loop={}m, elapsed={elapsed:?}",
            first.status,
            first.reason,
            first.expanded_states,
            first.candidates.len(),
            first
                .candidates
                .first()
                .map_or(0, |c| c.r#loop.distance_meters)
        );
        assert_eq!(first.status, "ok", "{area}: reason={:?}", first.reason);
        assert!(first.expanded_states < limits.max_expanded_states);
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "{area} explicit search exceeded 10s: {elapsed:?}"
        );
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap(),
            "{area} explicit search must be deterministic"
        );
        let candidate = &first.candidates[0];
        assert_eq!(candidate.entry.ramp_id.as_deref(), Some(entry));
        assert_eq!(candidate.exit.ramp_id.as_deref(), Some(exit));
        assert!(candidate.r#loop.distance_meters >= limits.min_loop_meters);
        assert!(candidate.toll.amount_yen.is_none() || candidate.toll.toll_source.is_some());
    }
}

#[test]
#[ignore = "full-network real-graph search; run with --release -- --ignored (CI does)"]
fn explicit_ramps_reject_non_public_kinds_and_report_unreachable_od() {
    let g = real_graph();
    let limits = SearchLimits::default();
    let request = |entry: &str, exit: &str| SearchRequest {
        request_id: format!("explicit-negative-{entry}-{exit}"),
        release_id: g.release_id.clone(),
        origin_node_id: None,
        origin: None,
        entry_ramp_id: Some(entry.into()),
        exit_ramp_id: Some(exit.into()),
        min_minutes: 1,
        max_minutes: 240,
        vehicle_profile: g.vehicle_profile.clone(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };

    for excluded in [
        "ramp:c1-outer:kyobashi-entry",    // unsupported
        "boundary:3-inbound:tomei-jct-in", // boundary JCT
        "ramp:c1-inner:gofukubashi-entry", // closed
    ] {
        let error = search(
            &g,
            &request(excluded, "ramp:c1-outer:takaracho-exit"),
            &limits,
        )
        .expect_err("non-public inventory record must not become a routing endpoint");
        assert_eq!(error.code, "INVALID_INPUT");
        assert!(error.message.contains("unknown or unsupported entry"));
    }

    let wrong_kind = search(
        &g,
        &request(
            "ramp:c1-outer:takaracho-exit",
            "ramp:c1-outer:kandabashi-entry",
        ),
        &limits,
    )
    .expect_err("exit-as-entry and entry-as-exit must be rejected");
    assert_eq!(wrong_kind.code, "INVALID_INPUT");
    assert!(wrong_kind.message.contains("not a routable general entry"));

    let unreachable = search(
        &g,
        &request(
            "ramp:k3-outbound:bandobashi-entry",
            "ramp:k3-inbound:shin-yamashita-exit",
        ),
        &limits,
    )
    .expect("unreachable OD is a normal no-candidate result");
    assert_eq!(unreachable.status, "no_candidates");
    assert_eq!(unreachable.reason.as_deref(), Some("NO_LOOP"));
    assert_ne!(unreachable.status, "truncated");
}

/// データ契約上 verified の全 2 ペアの探索契約定数。
/// 各ペアの C1 一周計画時間が時間窓に収まる max_minutes とその根拠を明示。
/// 一般道排除（issue #25）により、旧来「一般道が切断されていた」3 ペアも
/// origin_node_id = Entry エッジの from-node（アクセス時間 0）として直接探索可能になった。
struct ConnectedPairContract {
    pair_id: &'static str,
    max_minutes: u64,
    rationale: &'static str,
}

const CONNECTED_SEARCH_PAIRS: [ConnectedPairContract; 2] = [
    ConnectedPairContract {
        pair_id: "bp:c1-outer:kandabashi-takaracho",
        max_minutes: 60,
        rationale: "神田橋〜宝町（外回り）。C1 一周の実走行計画時間は約30分（base=1503s, plan=1803s）。max_minutes=60 の標準窓で自ペア候補が採択される。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-outer:kasumigaseki-daikancho",
        max_minutes: 60,
        rationale: "霞が関〜大官町（外回り）。一般道排除後、Entry from-node を起点とするためアクセス時間 0。C1 一周の標準窓 max_minutes=60 で自ペア候補が採択される。",
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

    // 1. verified 全 2 ペアの探索契約。
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
            origin_node_id: Some(origin_node_id.clone()),
            origin: None,
            entry_ramp_id: None,
            exit_ramp_id: None,
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

    // 2. verified/unverified の区分を維持し、verified のみ探索対象にする。
    assert_eq!(
        g.billing_pairs
            .iter()
            .filter(|pair| pair.status == shutoko_routing_core::VerificationStatus::Verified)
            .count(),
        2
    );
    assert_eq!(
        g.billing_pairs
            .iter()
            .filter(|pair| pair.status == shutoko_routing_core::VerificationStatus::Unverified)
            .count(),
        6
    );
    for pair in g
        .billing_pairs
        .iter()
        .filter(|pair| pair.status == shutoko_routing_core::VerificationStatus::Verified)
    {
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
        "all 2 verified billing pairs search contract total elapsed: {:?}",
        total_elapsed
    );
    assert!(
        total_elapsed < std::time::Duration::from_secs(60),
        "total search time for all 2 verified pairs must be under 60 seconds, took {:?}",
        total_elapsed
    );
}

#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn real_graph_coordinate_input_snap_and_candidate_enrichment() {
    let g = real_graph();
    let limits = SearchLimits::default();
    // 神田橋入口の一般道側始点 n:1070862943 の実座標をそのまま使う。
    let request = SearchRequest {
        request_id: "req-c1-coord".into(),
        release_id: "all-real-v1".into(),
        origin_node_id: None,
        origin: Some(shutoko_routing_core::LatLng {
            lat: 35.6896727,
            lon: 139.7644248,
        }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 15,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let result = search(&g, &request, &limits).expect("coordinate search must succeed");
    assert_eq!(result.status, "ok");
    // The Round 2 endpoint audit leaves two uniquely reverse-mapped verified
    // pairs. At Kandabashi coordinates the Kandabashi pair must be usable.
    let candidate = result
        .candidates
        .iter()
        .find(|c| c.toll.billing_pair_id == "bp:c1-outer:kandabashi-takaracho")
        .expect("kandabashi-takaracho must be available from Kandabashi coordinates");
    assert_eq!(candidate.entry.name.as_deref(), Some("神田橋入口"));
    assert_eq!(candidate.exit.name.as_deref(), Some("宝町出口"));
    assert_eq!(candidate.entry_id, candidate.entry.edge_id);
    assert_eq!(candidate.exit_id, candidate.exit.edge_id);
    assert_eq!(
        candidate.geometry.coordinates.len(),
        candidate.edge_ids.len() + 1,
        "geometry points must equal edge count + 1"
    );
    assert!(
        candidate.handoff.waypoints.len() <= 3,
        "waypoints must be at most 3, got {}",
        candidate.handoff.waypoints.len()
    );
    assert!(candidate
        .handoff
        .maps_url
        .starts_with("https://www.google.com/maps/dir/?api=1&"));
    assert!(candidate.handoff.maps_url.len() <= 2048);
    assert!(candidate.handoff.verification_set_version.is_none());
    assert!(candidate
        .warnings
        .iter()
        .any(|w| w == "HANDOFF_WAYPOINTS_UNVERIFIED"));
    assert!(!candidate.road_names.is_empty());
}

#[test]
fn real_graph_no_entry_edges_coordinate_is_no_connection() {
    // スナップ半径 200m 制限は廃止済み。新仕様での NO_CONNECTION 条件は
    // 「グラフに Entry エッジが1件もない（snap grid が空）」である。
    // Entry エッジを全て除いた改変グラフで座標入力を行い、
    // k_nearest が空 → NO_CONNECTION となることを確認する。
    use shutoko_routing_core::EdgeKind;
    let mut g = real_graph();
    // Entry エッジを除去 → snap grid が空になる。
    g.edges.retain(|e| e.kind != EdgeKind::Entry);
    // BillingPairs and entry Ramps reference Entry edges; clear them to keep graph valid.
    g.billing_pairs.clear();
    g.ramps
        .retain(|r| g.edges.iter().any(|e| e.id == r.edge_id));
    let limits = SearchLimits::default();
    let request = SearchRequest {
        request_id: "req-no-entry".into(),
        release_id: "all-real-v1".into(),
        origin_node_id: None,
        origin: Some(shutoko_routing_core::LatLng {
            lat: 35.62,
            lon: 139.79,
        }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 15,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let result = search(&g, &request, &limits)
        .expect("empty-entry graph must be Ok (search does not error on empty snap grid)");
    assert_eq!(result.status, "no_candidates");
    assert_eq!(result.reason.as_deref(), Some("NO_CONNECTION"));
    assert!(result.candidates.is_empty());
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
            origin_node_id: Some(origin.clone()),
            origin: None,
            entry_ramp_id: None,
            exit_ramp_id: None,
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

// ---------------------------------------------------------------------------
// Task A+B: 現実座標起点のテスト（30 km キャップ / unlimited entries）
// ---------------------------------------------------------------------------

/// 大阪駅 (34.7025, 135.4959) は C1 最寄り入口まで約 400 km あり、
/// デフォルト 30 km キャップを超えるため NO_CONNECTION を返す。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn osaka_station_returns_no_connection_due_to_distance_cap() {
    let g = real_graph();
    let limits = SearchLimits::default(); // max_access_distance_meters = 30 000 m
    let request = SearchRequest {
        request_id: "req-osaka-no-conn".into(),
        release_id: "all-real-v1".into(),
        origin_node_id: None,
        origin: Some(LatLng {
            lat: 34.7025,
            lon: 135.4959,
        }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 30,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let result = search(&g, &request, &limits).expect("search must not error on far origin");
    eprintln!(
        "Osaka station: status={}, reason={:?}",
        result.status, result.reason
    );
    assert_eq!(
        result.status, "no_candidates",
        "Osaka station must return no_candidates"
    );
    assert_eq!(
        result.reason.as_deref(),
        Some("NO_CONNECTION"),
        "Osaka station is ~400 km from C1: must be NO_CONNECTION with 30 km cap"
    );
}

/// 東京駅 (35.6812, 139.7671) から検索すると、unlimited entries（デフォルト）で
/// 候補が返る。最寄り入口（宝町）まで約 746 m。
/// 旧デフォルト 5 件だと候補 0 だったが、unlimited で候補 ≥ 1 が確認できる。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn tokyo_station_returns_candidates_with_unlimited_entries() {
    let g = real_graph();
    let limits = SearchLimits::default(); // max_access_entries=0 (unlimited), 30 km cap
    let request = SearchRequest {
        request_id: "req-tokyo-station".into(),
        release_id: "all-real-v1".into(),
        origin_node_id: None,
        origin: Some(LatLng {
            lat: 35.6812,
            lon: 139.7671,
        }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 15,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let result = search(&g, &request, &limits).expect("search must not error");
    eprintln!(
        "Tokyo station (unlimited): status={}, reason={:?}, candidates={}",
        result.status,
        result.reason,
        result.candidates.len()
    );
    assert_eq!(
        result.status, "ok",
        "東京駅 unlimited entries: 候補が得られること。reason={:?}",
        result.reason
    );
    assert!(
        !result.candidates.is_empty(),
        "東京駅 unlimited entries: 少なくとも 1 件の候補が必要"
    );
    // 最寄り入口は宝町入口（~746 m）。アクセス距離が 2 km 未満であることを確認する。
    let nearest_access_dist = result
        .candidates
        .iter()
        .map(|c| c.snapped_origin.distance_meters)
        .fold(f64::MAX, f64::min);
    assert!(
        nearest_access_dist < 2000.0,
        "東京駅 nearest access must be < 2 km, got {:.0} m",
        nearest_access_dist
    );
}

/// 新宿駅 (35.6896, 139.7006) と渋谷駅 (35.6580, 139.7016) から
/// unlimited entries（デフォルト）で候補が返ること。
/// 新宿: 最寄り約 4.6 km（霞が関）/ 渋谷: 最寄り約 3.9 km（芝公園）。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn shinjuku_and_shibuya_stations_return_candidates() {
    let g = real_graph();
    let limits = SearchLimits::default();

    for (station, lat, lon) in [
        ("新宿駅", 35.6896_f64, 139.7006_f64),
        ("渋谷駅", 35.6580_f64, 139.7016_f64),
    ] {
        let request = SearchRequest {
            request_id: format!("req-{station}"),
            release_id: "all-real-v1".into(),
            origin_node_id: None,
            origin: Some(LatLng { lat, lon }),
            entry_ramp_id: None,
            exit_ramp_id: None,
            min_minutes: 30,
            max_minutes: 60,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = search(&g, &request, &limits)
            .unwrap_or_else(|e| panic!("{station} search must not error: {e}"));
        eprintln!(
            "{station}: status={}, reason={:?}, candidates={}",
            result.status,
            result.reason,
            result.candidates.len()
        );
        assert_eq!(
            result.status, "ok",
            "{station} unlimited entries: 候補が得られること。reason={:?}",
            result.reason
        );
        assert!(
            !result.candidates.is_empty(),
            "{station}: 少なくとも 1 件の候補が必要"
        );
    }
}

// ---------------------------------------------------------------------------
// engine-001: 都内境界の診断契約（nearestAccess / minPlanSeconds）
// ---------------------------------------------------------------------------

/// Rust の既定 cap（30 km）は変更しない。web 側が prepare limits で 46 km を明示する
/// 前提の境界を実データで固定する。
const WIDE_ACCESS_CAP_METERS: f64 = 46_000.0;
/// 製品上限は 240 分（`validate_request` と同じ）。
const FOUR_HOURS_SECONDS: u64 = 240 * 60;

fn wide_access_limits() -> SearchLimits {
    SearchLimits {
        max_access_distance_meters: WIDE_ACCESS_CAP_METERS,
        ..SearchLimits::default()
    }
}

fn coordinate_request(request_id: &str, lat: f64, lon: f64, max_minutes: u64) -> SearchRequest {
    SearchRequest {
        request_id: request_id.into(),
        release_id: "all-real-v1".into(),
        origin_node_id: None,
        origin: Some(LatLng { lat, lon }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 15,
        max_minutes,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    }
}

/// 日野市役所 (35.6711, 139.3952) / 立川駅 (35.6979, 139.4139) /
/// 八王子駅 (35.6556, 139.3388) / 神田橋入口 (35.6896727, 139.7644248) の
/// 全線実データ境界。K7・4号・3号の verified-bound 入口が加わったため、3地点は
/// いずれも既定30km cap内となる。日野・立川では候補が返り、八王子では往復込み
/// 最短計画が240分を僅かに超えるため TIME_WINDOW 診断になることを確認する。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn tokyo_wide_coordinate_diagnostics_contract() {
    let g = real_graph();
    let wide = wide_access_limits();

    // ── 1. 日野市役所: K7横浜青葉入口まで約18.5km、既定cap内 ──
    let hino_default_cap = search(
        &g,
        &coordinate_request("req-hino-default-cap", 35.6711, 139.3952, 240),
        &SearchLimits::default(),
    )
    .expect("hino default-cap search must not error");
    assert_eq!(hino_default_cap.status, "ok");
    let default_nearest = hino_default_cap
        .nearest_access
        .as_ref()
        .expect("coordinate search must report nearestAccess");
    assert!(
        (18_000.0..19_000.0).contains(&default_nearest.distance_meters),
        "hino nearest entry must be ~18.5 km, got {:.0} m",
        default_nearest.distance_meters
    );
    assert!(
        hino_default_cap.min_plan_seconds.is_some(),
        "successful search must report minPlanSeconds"
    );

    // ── 2. 日野市役所（cap 46 km）: 240 分窓に収まる候補が成立する ──
    let hino_wide = search(
        &g,
        &coordinate_request("req-hino-wide-cap", 35.6711, 139.3952, 240),
        &wide,
    )
    .expect("hino wide-cap search must not error");
    eprintln!(
        "hino wide-cap: status={}, reason={:?}, candidates={}, nearest={:.0}m, minPlan={:?}s",
        hino_wide.status,
        hino_wide.reason,
        hino_wide.candidates.len(),
        hino_wide
            .nearest_access
            .as_ref()
            .map_or(f64::NAN, |n| n.distance_meters),
        hino_wide.min_plan_seconds
    );
    assert_eq!(
        hino_wide.status, "ok",
        "hino with 46 km cap must produce candidates; reason={:?}",
        hino_wide.reason
    );
    assert!(!hino_wide.candidates.is_empty());
    let hino_nearest = hino_wide
        .nearest_access
        .as_ref()
        .expect("coordinate input must report nearestAccess");
    assert!(
        (18_000.0..19_000.0).contains(&hino_nearest.distance_meters),
        "hino nearest entry must be ~18.5 km, got {:.0} m",
        hino_nearest.distance_meters
    );
    let hino_min_plan = hino_wide
        .min_plan_seconds
        .expect("hino must report minPlanSeconds when candidates exist");
    assert!(
        hino_min_plan <= FOUR_HOURS_SECONDS,
        "hino minPlanSeconds ({hino_min_plan}) must fit the 240 min window"
    );

    // ── 3. 立川駅（cap 46 km）: 240 分窓に収まる候補が成立する ──
    let tachikawa_wide = search(
        &g,
        &coordinate_request("req-tachikawa-wide-cap", 35.6979, 139.4139, 240),
        &wide,
    )
    .expect("tachikawa wide-cap search must not error");
    eprintln!(
        "tachikawa wide-cap: status={}, reason={:?}, candidates={}, nearest={:.0}m, minPlan={:?}s",
        tachikawa_wide.status,
        tachikawa_wide.reason,
        tachikawa_wide.candidates.len(),
        tachikawa_wide
            .nearest_access
            .as_ref()
            .map_or(f64::NAN, |n| n.distance_meters),
        tachikawa_wide.min_plan_seconds
    );
    assert_eq!(
        tachikawa_wide.status, "ok",
        "tachikawa with 46 km cap must produce candidates; reason={:?}",
        tachikawa_wide.reason
    );
    assert!(!tachikawa_wide.candidates.is_empty());
    let tachikawa_nearest = tachikawa_wide
        .nearest_access
        .as_ref()
        .expect("coordinate input must report nearestAccess");
    assert!(
        (18_000.0..19_000.0).contains(&tachikawa_nearest.distance_meters),
        "tachikawa nearest entry must be ~18.6 km, got {:.0} m",
        tachikawa_nearest.distance_meters
    );
    let tachikawa_min_plan = tachikawa_wide
        .min_plan_seconds
        .expect("tachikawa must report minPlanSeconds when candidates exist");
    assert!(
        tachikawa_min_plan <= FOUR_HOURS_SECONDS,
        "tachikawa minPlanSeconds ({tachikawa_min_plan}) must fit the 240 min window"
    );

    // ── 4. 八王子駅: 入口はcap内だが往復込み最短計画が240分を僅かに超える ──
    let hachioji_wide = search(
        &g,
        &coordinate_request("req-hachioji-wide-cap", 35.6556, 139.3388, 240),
        &wide,
    )
    .expect("hachioji wide-cap search must not error");
    eprintln!(
        "hachioji wide-cap: status={}, reason={:?}, candidates={}, nearest={:.0}m, minPlan={:?}s",
        hachioji_wide.status,
        hachioji_wide.reason,
        hachioji_wide.candidates.len(),
        hachioji_wide
            .nearest_access
            .as_ref()
            .map_or(f64::NAN, |n| n.distance_meters),
        hachioji_wide.min_plan_seconds
    );
    assert_eq!(hachioji_wide.status, "no_candidates");
    assert_eq!(hachioji_wide.reason.as_deref(), Some("TIME_WINDOW"));
    assert!(hachioji_wide.candidates.is_empty());
    let hachioji_nearest = hachioji_wide
        .nearest_access
        .as_ref()
        .expect("hachioji must report nearestAccess for coordinate input");
    assert!(
        (21_000.0..22_000.0).contains(&hachioji_nearest.distance_meters),
        "hachioji nearest entry must be ~21.4 km, got {:.0} m",
        hachioji_nearest.distance_meters
    );
    let hachioji_min_plan = hachioji_wide
        .min_plan_seconds
        .expect("hachioji TIME_WINDOW result must report minPlanSeconds");
    assert!(
        hachioji_min_plan > FOUR_HOURS_SECONDS && hachioji_min_plan < 16_000,
        "hachioji minPlanSeconds ({hachioji_min_plan}) must narrowly exceed 240 minutes"
    );

    // ── 5. 神田橋入口: 座標入力で距離 0、originNodeId 入力で nearestAccess は null ──
    let kandabashi_coord = search(
        &g,
        &coordinate_request("req-kandabashi-coord", 35.6896727, 139.7644248, 60),
        &wide,
    )
    .expect("kandabashi coordinate search must not error");
    assert_eq!(kandabashi_coord.status, "ok");
    let kandabashi_nearest = kandabashi_coord
        .nearest_access
        .as_ref()
        .expect("kandabashi must report nearestAccess for coordinate input");
    assert!(
        kandabashi_nearest.distance_meters < 1.0,
        "kandabashi entry coordinates must snap with ~0 m, got {:.1} m",
        kandabashi_nearest.distance_meters
    );

    let kandabashi_node = search(
        &g,
        &SearchRequest {
            request_id: "req-kandabashi-node".into(),
            release_id: "all-real-v1".into(),
            origin_node_id: Some("n:1070862943".into()),
            origin: None,
            entry_ramp_id: None,
            exit_ramp_id: None,
            min_minutes: 15,
            max_minutes: 60,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        },
        &wide,
    )
    .expect("kandabashi originNodeId search must not error");
    assert_eq!(kandabashi_node.status, "ok");
    assert!(
        kandabashi_node.nearest_access.is_none(),
        "originNodeId input must not report nearestAccess"
    );
    assert!(
        kandabashi_node.min_plan_seconds.is_some(),
        "originNodeId input must still report minPlanSeconds"
    );
}

// ---------------------------------------------------------------------------
// correct-001: narrow windows must still report the true minimum plan time.
// Before this fix a 60-minute request pruned every legal loop (all C1 loops
// take > 60 min once access/return are added), so `minPlanSeconds` was `null`
// and the UI could not tell "widen the window" from "no legal loop".
// ---------------------------------------------------------------------------

/// 立川駅 (35.6979, 139.4139) の 60 分窓: 候補にはならないが、診断は製品上限
/// 240 分までの合法周回を見て最短計画秒を返す。八王子駅では 240 分を超える値が
/// 返り、UI が「時間を広げても届かない」と正しく断定できる。
/// 併せて同一入力の決定論と 10 秒以内の応答を確認する。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn tokyo_wide_narrow_window_reports_min_plan_and_is_deterministic() {
    let g = real_graph();
    let wide = wide_access_limits();

    // ── 立川駅 60 分窓: legal loop は存在するが窓に収まらない（TIME_WINDOW）──
    let started = std::time::Instant::now();
    let tachikawa = search(
        &g,
        &coordinate_request("req-tachikawa-narrow", 35.6979, 139.4139, 60),
        &wide,
    )
    .expect("tachikawa narrow search must not error");
    let elapsed = started.elapsed();

    assert_eq!(tachikawa.status, "no_candidates");
    assert_eq!(tachikawa.reason.as_deref(), Some("TIME_WINDOW"));
    let min_plan = tachikawa
        .min_plan_seconds
        .expect("narrow window must still report the true minimum plan time");
    assert!(
        min_plan > 60 * 60,
        "tachikawa minimum plan ({min_plan} s) must exceed the 60 min window"
    );
    assert!(
        min_plan <= FOUR_HOURS_SECONDS,
        "tachikawa minimum plan ({min_plan} s) must fit the 240 min product cap"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "narrow-window diagnostic must answer within 10 s, took {elapsed:?}"
    );

    // ── 決定論: 同一入力の再実行で診断値が一致する ──
    let repeat = search(
        &g,
        &coordinate_request("req-tachikawa-narrow", 35.6979, 139.4139, 60),
        &wide,
    )
    .expect("repeat search must not error");
    assert_eq!(repeat.min_plan_seconds, tachikawa.min_plan_seconds);
    assert_eq!(repeat.reason, tachikawa.reason);
    assert_eq!(repeat.expanded_states, tachikawa.expanded_states);
    assert_eq!(repeat.candidates.len(), tachikawa.candidates.len());

    // ── 八王子駅 60 分窓: 最短計画は 240 分を超える（unreachable の根拠）──
    let hachioji = search(
        &g,
        &coordinate_request("req-hachioji-narrow", 35.6556, 139.3388, 60),
        &wide,
    )
    .expect("hachioji narrow search must not error");
    assert_eq!(hachioji.status, "no_candidates");
    assert_eq!(hachioji.reason.as_deref(), Some("TIME_WINDOW"));
    let hachioji_min_plan = hachioji
        .min_plan_seconds
        .expect("hachioji narrow window must report the minimum plan time");
    assert!(
        hachioji_min_plan > FOUR_HOURS_SECONDS,
        "hachioji minimum plan ({hachioji_min_plan} s) must exceed the product cap"
    );
}
