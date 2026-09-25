use serde_json::{json, Value};
use shutoko_routing_core::{
    prepare_json, search, search_json, Candidate, Graph, LatLng, LegacyCandidate, SearchLimits,
    SearchRequest, TopologyOnlyCandidate,
};
use std::collections::{BTreeSet, HashMap};

fn legacy(candidate: &Candidate) -> &LegacyCandidate {
    candidate
        .as_legacy()
        .expect("real graph contract expects LegacyCandidate")
}

fn topology_only(candidate: &Candidate) -> &TopologyOnlyCandidate {
    candidate
        .as_topology_only()
        .expect("dynamic OD contract expects TopologyOnlyCandidate")
}

fn real_graph_str() -> &'static str {
    include_str!("../../../fixtures/generated/graph.json")
}

fn real_graph() -> Graph {
    let mut wire: Value =
        serde_json::from_str(real_graph_str()).expect("schema 4 graph JSON must deserialize");
    wire["schemaVersion"] = json!(2);
    wire.as_object_mut().unwrap().remove("routeMemberships");
    wire.as_object_mut().unwrap().remove("odTariffsV3");
    // all-real-v4 が記録する版メタデータ（schema 2 の reader には無い）。
    wire.as_object_mut().unwrap().remove("billingPairsVersion");
    wire.as_object_mut().unwrap().remove("tariffModelVersion");
    wire["billingPairs"]
        .as_array_mut()
        .unwrap()
        .retain(|pair| pair["pairKind"] == json!("legacyRing"));
    let ramp_data = wire["ramps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ramp| {
            (
                ramp["edgeId"].as_str().unwrap().to_owned(),
                (
                    ramp["id"].as_str().unwrap().to_owned(),
                    ramp["name"].as_str().unwrap_or_default().to_owned(),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    for pair in wire["billingPairs"].as_array_mut().unwrap() {
        let status = if pair["pairEligibility"]["status"] == json!("verified_one_section_ahead") {
            "verified"
        } else {
            "unverified"
        };
        let entry_id = pair["entryId"].as_str().unwrap().to_owned();
        let exit_id = pair["exitId"].as_str().unwrap().to_owned();
        let entry_ramp = ramp_data.get(&entry_id);
        let exit_ramp = ramp_data.get(&exit_id);
        let entry_name = pair
            .get("entryName")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| entry_ramp.map(|(_, name)| name.clone()));
        let exit_name = pair
            .get("exitName")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| exit_ramp.map(|(_, name)| name.clone()));
        *pair = json!({
            "id": pair["id"],
            "entryId": entry_id,
            "exitId": exit_id,
            "anchorNodeId": pair["anchor"]["nodeId"],
            "entryToAnchorEdgeIds": pair["entryToAnchorEdgeIds"],
            "anchorToExitEdgeIds": pair["anchorToExitEdgeIds"],
            "status": status,
            "vehicleProfile": pair["vehicleProfile"],
            "prices": pair["tariff"]["prices"],
            "entryName": entry_name,
            "exitName": exit_name,
            "entryRampId": entry_ramp.map(|(id, _)| id.as_str()),
            "exitRampId": exit_ramp.map(|(id, _)| id.as_str()),
            "billingDistanceMeters": pair["tariff"]["billingDistanceMeters"],
        });
    }
    serde_json::from_value(wire).expect("legacy adapter graph deserialization must succeed")
}

#[test]
fn real_graph_deserialization_and_schema_validation() {
    let wire: Value = serde_json::from_str(real_graph_str()).unwrap();
    assert_eq!(wire["schemaVersion"], 4);
    assert_eq!(wire["releaseId"], "all-real-v4");
    assert_eq!(wire["billingPairs"].as_array().unwrap().len(), 10);
    assert!(wire["routeMemberships"].as_array().is_some());
    let prepared = prepare_json(real_graph_str(), "{}").expect("schema 4 graph must prepare");
    assert_eq!(prepared.graph().schema_version, 4);
    // all-real-v4 は全 route relation を cover するため、双方向 46 件に 7 件の
    // forward membership が加わる（pair-candidates.json の relationManifest と一致）。
    assert_eq!(prepared.route_memberships().len(), 53);
    assert_eq!(
        wire["billingPairs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|pair| pair["pairEligibility"]["status"] == json!("verified_one_section_ahead"))
            .count(),
        9,
        "9 verified pairs (7 legacyRing + 2 radialReturn); only bp:c1-outer:shibakoen-iikura stays unverified (conditional public way)"
    );
    let g = real_graph();
    assert_eq!(g.schema_version, 2);
    assert_eq!(g.release_id, "all-real-v4");
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
        7
    );
    assert_eq!(
        g.billing_pairs
            .iter()
            .filter(|p| p.status == shutoko_routing_core::VerificationStatus::Unverified)
            .count(),
        1
    );

    for pair in &g.billing_pairs {
        let expected_amount = if pair.id == "bp:c1-outer:kasumigaseki-daikancho" {
            570
        } else {
            300
        };
        assert_eq!(
            pair.prices
                .iter()
                .map(|price| price.amount_yen)
                .collect::<Vec<_>>(),
            vec![expected_amount, 300],
            "billing pair {} prices must match the reviewed tariff records",
            pair.id
        );
        assert_eq!(pair.prices[0].effective_from, "2022-03-31T15:00:00Z");
        assert_eq!(
            pair.prices[0].effective_to.as_deref(),
            Some("2026-09-30T15:00:00Z")
        );
        assert_eq!(pair.prices[1].amount_yen, 300);
        assert_eq!(pair.prices[1].effective_from, "2026-09-30T15:00:00Z");
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

    let id = "bp:c1-outer:shibakoen-iikura";
    assert_eq!(
        g.billing_pairs
            .iter()
            .find(|pair| pair.id == id)
            .unwrap_or_else(|| panic!("missing billing pair {id}"))
            .status,
        shutoko_routing_core::VerificationStatus::Unverified,
        "billing pair {id} must remain unverified until both endpoints uniquely resolve"
    );

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
    let diagnostic_only = unverified
        .iter()
        .filter_map(Value::as_str)
        .filter(|section| section.starts_with("diagnostic-only:"))
        .collect::<Vec<_>>();
    assert!(diagnostic_only.is_empty());
}

#[test]
fn radial_seed_is_promoted_after_exact_binding_resolution() {
    let seed: Value =
        serde_json::from_str(include_str!("../../../data/billing-pairs-seed.json")).unwrap();
    assert_eq!(seed["schemaVersion"], 2);
    assert_eq!(seed["billingPairs"].as_array().unwrap().len(), 10);

    let radial_pairs = seed["billingPairs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|pair| pair["pairKind"] == "radialReturn")
        .collect::<Vec<_>>();
    assert_eq!(
        radial_pairs
            .iter()
            .map(|pair| pair["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "bp:2-inbound:meguro:c1-inner:tengenji",
            "bp:2-inbound:meguro:c1-outer:tengenji"
        ]
    );

    for pair in &radial_pairs {
        assert_eq!(pair["routePlanVersion"], 1);
        assert_eq!(pair["entryEndpoint"]["supportState"], "verified_bound");
        assert_eq!(pair["exitEndpoint"]["supportState"], "verified_bound");
        assert!(pair["exitEndpoint"]
            .get("bindingCandidates")
            .and_then(Value::as_array)
            .is_none_or(|candidates| candidates.is_empty()));
        assert_eq!(
            pair["exitEndpoint"]["directedSegments"][0]["edgeIds"]
                .as_array()
                .unwrap()
                .len(),
            16
        );
        assert_eq!(
            pair["exitEndpoint"]["directedSegments"][0]["edgeIdsSha256"],
            "bb9114f49d64b952b58b5a2ef53679a6007bea48a51671ade34c56b0325fa7cd"
        );
        assert_eq!(
            pair["pairEligibility"]["status"],
            "verified_one_section_ahead"
        );
        assert_eq!(pair["pairEligibility"]["oneSectionAheadVerified"], true);
        assert_eq!(pair["tariff"]["status"], "priced");
        assert_eq!(pair["tariff"]["amountYen"], 790);
        assert_eq!(pair["tariff"]["billingDistanceMeters"], 19400);
    }

    assert_eq!(
        radial_pairs[0]["routePlan"]["anchor"]["mergeNodeId"],
        "n:574460576"
    );
    assert_eq!(
        radial_pairs[0]["routePlan"]["anchor"]["branchNodeId"],
        "n:574460605"
    );
    assert_eq!(
        radial_pairs[1]["routePlan"]["anchor"]["mergeNodeId"],
        "n:31297008"
    );
    assert_eq!(
        radial_pairs[1]["routePlan"]["anchor"]["branchNodeId"],
        "n:31297000"
    );
    assert_eq!(
        radial_pairs[0]["routePlan"]["anchor"]["excludedShortConnector"]["edgeCount"],
        23
    );
    assert_eq!(
        radial_pairs[0]["routePlan"]["anchor"]["excludedShortConnector"]["distanceMeters"],
        493
    );
    assert_eq!(
        radial_pairs[1]["routePlan"]["anchor"]["excludedShortConnector"]["edgeCount"],
        20
    );
    assert_eq!(
        radial_pairs[1]["routePlan"]["anchor"]["excludedShortConnector"]["distanceMeters"],
        461
    );

    let graph: Value = serde_json::from_str(real_graph_str()).unwrap();
    assert_eq!(graph["billingPairs"].as_array().unwrap().len(), 10);
    assert_eq!(
        graph["billingPairs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|pair| pair["pairKind"] == "radialReturn")
            .count(),
        2
    );
}

#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn real_graph_routing_core_search_returns_candidates() {
    let g = real_graph();

    let request = SearchRequest {
        request_id: "req-c1-kandabashi-1".into(),
        release_id: "all-real-v4".into(),
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
    assert_eq!(result.release_id, "all-real-v4");
    assert!(
        !result.candidates.is_empty(),
        "expected at least 1 candidate route from real graph"
    );

    let c = legacy(&result.candidates[0]);
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
        release_id: "all-real-v4".into(),
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
    let c = legacy(&res_before_boundary.candidates[0]);
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
    let c = legacy(&res_at_boundary.candidates[0]);
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
    let c = legacy(&res_post.candidates[0]);
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
    let c = legacy(&res_prior.candidates[0]);
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
        "releaseId": "all-real-v4",
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
                .map_or(0, |c| topology_only(c).r#loop.distance_meters)
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
        let candidate = topology_only(&first.candidates[0]);
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
    amount_yen: u64,
    rationale: &'static str,
}

const CONNECTED_SEARCH_PAIRS: [ConnectedPairContract; 7] = [
    ConnectedPairContract {
        pair_id: "bp:c1-outer:kandabashi-takaracho",
        max_minutes: 60,
        amount_yen: 300,
        rationale: "神田橋〜宝町（外回り）。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-outer:kasumigaseki-daikancho",
        max_minutes: 60,
        amount_yen: 570,
        rationale: "霞が関〜大官町（外回り）。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-outer:ginza-shibakoen",
        max_minutes: 60,
        amount_yen: 300,
        rationale: "銀座〜芝公園（外回り）。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-inner:kasumigaseki-shibakoen",
        max_minutes: 60,
        amount_yen: 300,
        rationale: "霞が関〜芝公園（内回り）。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-inner:daikancho-kasumigaseki",
        max_minutes: 60,
        amount_yen: 300,
        rationale: "代官町〜霞が関（内回り）。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-inner:shibakoen-shiodome",
        max_minutes: 60,
        amount_yen: 300,
        rationale: "芝公園〜汐留（内回り）。",
    },
    ConnectedPairContract {
        pair_id: "bp:c1-inner:takaracho-kandabashi",
        max_minutes: 60,
        amount_yen: 300,
        rationale: "宝町〜神田橋（内回り）。",
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
            .find(|c| legacy(c).toll.billing_pair_id == pair.id)
            .map(legacy)
            .unwrap_or_else(|| {
                panic!(
                    "candidate with billing_pair_id {} must be found from origin {} with max_minutes={} ({})",
                    pair.id, origin_node_id, contract.max_minutes, contract.rationale
                )
            });

        assert_eq!(
            candidate.toll.amount_yen,
            Some(contract.amount_yen),
            "toll amount for pair {} must be {} yen",
            pair.id,
            contract.amount_yen
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
        7
    );
    assert_eq!(
        g.billing_pairs
            .iter()
            .filter(|pair| pair.status == shutoko_routing_core::VerificationStatus::Unverified)
            .count(),
        1
    );
    for pair in g
        .billing_pairs
        .iter()
        .filter(|pair| pair.status == shutoko_routing_core::VerificationStatus::Verified)
    {
        let expected_amount = if pair.id == "bp:c1-outer:kasumigaseki-daikancho" {
            570
        } else {
            300
        };
        assert_eq!(
            pair.prices
                .iter()
                .map(|price| price.amount_yen)
                .collect::<Vec<_>>(),
            vec![expected_amount, 300],
            "pair {} prices must match the reviewed tariff records",
            pair.id
        );
        assert!(!pair.entry_to_anchor_edge_ids.is_empty());
        assert!(!pair.anchor_to_exit_edge_ids.is_empty());
    }

    let total_elapsed = start_total.elapsed();
    eprintln!(
        "all verified billing pairs search contract total elapsed: {:?}",
        total_elapsed
    );
    assert!(
        total_elapsed < std::time::Duration::from_secs(60),
        "total search time for verified pairs must be under 60 seconds, took {:?}",
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
        release_id: "all-real-v4".into(),
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
        .find(|c| legacy(c).toll.billing_pair_id == "bp:c1-outer:kandabashi-takaracho")
        .map(legacy)
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
    g.ramps.clear();
    let limits = SearchLimits::default();
    let request = SearchRequest {
        request_id: "req-no-entry".into(),
        release_id: "all-real-v4".into(),
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

struct EightPairContract {
    pair_id: &'static str,
    origin_node_id: &'static str,
    anchor_node_id: &'static str,
    exit_edge_id: &'static str,
    max_minutes: u64,
    own_pair: bool,
}

const EIGHT_PAIR_CONTRACTS: [EightPairContract; 8] = [
    EightPairContract {
        pair_id: "bp:c1-inner:daikancho-kasumigaseki",
        origin_node_id: "n:1866081909",
        anchor_node_id: "n:297945194",
        exit_edge_id: "e:w1232166619:0:f",
        max_minutes: 60,
        own_pair: true,
    },
    EightPairContract {
        pair_id: "bp:c1-inner:kasumigaseki-shibakoen",
        origin_node_id: "n:573233927",
        anchor_node_id: "n:264877748",
        exit_edge_id: "e:w203873821:2:f",
        max_minutes: 60,
        own_pair: true,
    },
    EightPairContract {
        pair_id: "bp:c1-inner:shibakoen-shiodome",
        origin_node_id: "n:254367256",
        anchor_node_id: "n:31295430",
        exit_edge_id: "e:w45068171:1:f",
        max_minutes: 30,
        own_pair: true,
    },
    EightPairContract {
        pair_id: "bp:c1-inner:takaracho-kandabashi",
        origin_node_id: "n:1105125663",
        anchor_node_id: "n:1891818143",
        exit_edge_id: "e:w390441534:2:f",
        max_minutes: 60,
        own_pair: true,
    },
    EightPairContract {
        pair_id: "bp:c1-outer:ginza-shibakoen",
        origin_node_id: "n:835996316",
        anchor_node_id: "n:31254160",
        exit_edge_id: "e:w944671542:0:f",
        max_minutes: 60,
        own_pair: true,
    },
    EightPairContract {
        pair_id: "bp:c1-outer:kandabashi-takaracho",
        origin_node_id: "n:1070862943",
        anchor_node_id: "n:499831338",
        exit_edge_id: "e:w297864314:11:f",
        max_minutes: 60,
        own_pair: true,
    },
    EightPairContract {
        pair_id: "bp:c1-outer:kasumigaseki-daikancho",
        origin_node_id: "n:577255402",
        anchor_node_id: "n:577255571",
        exit_edge_id: "e:w276920911:6:f",
        max_minutes: 60,
        own_pair: true,
    },
    EightPairContract {
        pair_id: "bp:c1-outer:shibakoen-iikura",
        origin_node_id: "n:940044988",
        anchor_node_id: "n:31296971",
        exit_edge_id: "e:w203832842:4:f",
        max_minutes: 60,
        own_pair: false,
    },
];

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
        let contract = EIGHT_PAIR_CONTRACTS
            .iter()
            .find(|contract| contract.pair_id == p.id)
            .unwrap_or_else(|| panic!("missing eight-pair contract for {}", p.id));
        let origin = edge_map[p.entry_to_anchor_edge_ids[0].as_str()]
            .from
            .clone();
        assert_eq!(origin, contract.origin_node_id, "pair {}", p.id);
        assert_eq!(p.anchor_node_id, contract.anchor_node_id, "pair {}", p.id);
        assert_eq!(
            p.anchor_to_exit_edge_ids.last().map(String::as_str),
            Some(contract.exit_edge_id),
            "pair {}",
            p.id
        );
        assert_eq!(
            p.status == shutoko_routing_core::VerificationStatus::Verified,
            contract.own_pair
        );
        let max_minutes = contract.max_minutes;
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

        let expected_status = if contract.own_pair {
            "ok"
        } else {
            "no_candidates"
        };
        assert_eq!(res.status, expected_status, "pair {}", p.id);
        assert_eq!(
            res.candidates.len(),
            if contract.own_pair { 1 } else { 0 },
            "pair {} candidate count",
            p.id
        );
        let own = res
            .candidates
            .iter()
            .any(|c| legacy(c).toll.billing_pair_id == p.id);
        assert_eq!(own, contract.own_pair, "pair {} own-pair", p.id);
        if contract.own_pair {
            let candidate = res
                .candidates
                .iter()
                .find_map(|candidate| {
                    legacy(candidate)
                        .toll
                        .billing_pair_id
                        .eq(&p.id)
                        .then(|| legacy(candidate))
                })
                .unwrap_or_else(|| panic!("pair {} candidate missing", p.id));
            assert_eq!(candidate.toll.charged_section_count, 1, "pair {}", p.id);
            assert_eq!(
                candidate.r#loop.anchor_node_id, contract.anchor_node_id,
                "pair {}",
                p.id
            );
            assert_eq!(
                candidate.exit.edge_id, contract.exit_edge_id,
                "pair {}",
                p.id
            );
            assert_eq!(
                candidate.edge_ids.last().map(String::as_str),
                Some(contract.exit_edge_id),
                "pair {} First Exit",
                p.id
            );
        }
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
        release_id: "all-real-v4".into(),
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

/// 東京駅 (35.6812, 139.7671) の最近接入口 tier 実測契約 (Issue #57)。
///
/// 最寄りは宝町入口 (746 m) で、同一施設の対向出口
/// ramp:c1-outer:takaracho-exit を持つため、15〜60 分は動的 OD
/// (料金額は未算出 = `shutoko_time`) の宝町候補が成立する。旧来の
/// 「より遠い Verified 入口 (神田橋) を選ぶ」挙動は最近接入口優先へ
/// 置き換わったため、期待値を動的 OD へ更新する。
///
/// 窓を 26 分以下へ狭めると最近接 tier (宝町入口) は完全評価済みで合法周回を
/// 持つが、その最短計画 1,610 秒が上限を超える。最近接入口優先の診断として
/// `TIME_WINDOW` と証明済み `minPlanSeconds` を返し、遠方入口へは縮退しない。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn tokyo_station_returns_candidates_with_unlimited_entries() {
    let g = real_graph();
    let limits = SearchLimits::default(); // max_access_entries=0 (unlimited), 30 km cap
    let request = |request_id: &str, min_minutes: u64, max_minutes: u64| SearchRequest {
        request_id: request_id.into(),
        release_id: "all-real-v4".into(),
        origin_node_id: None,
        origin: Some(LatLng {
            lat: 35.6812,
            lon: 139.7671,
        }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes,
        max_minutes,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let result = search(&g, &request("req-tokyo-station", 15, 60), &limits)
        .expect("15-60 minute search must not error");
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
    assert_eq!(
        result.candidates.len(),
        1,
        "東京駅 15-60 分は最近接の宝町 tier が返す 1 候補だけを返す"
    );
    let candidate = legacy(&result.candidates[0]);
    assert_eq!(
        candidate.toll.billing_pair_id,
        "bp:c1-inner:takaracho-kandabashi"
    );
    assert_eq!(
        candidate.entry.ramp_id.as_deref(),
        Some("ramp:c1-inner:takaracho-entry")
    );
    assert_eq!(
        candidate.exit.ramp_id.as_deref(),
        Some("ramp:c1-inner:kandabashi-exit")
    );
    assert_eq!(candidate.entry.name.as_deref(), Some("宝町入口"));
    assert_eq!(candidate.exit.name.as_deref(), Some("神田橋出口"));
    assert_eq!(candidate.duration.plan_seconds, 1_707);
    assert_eq!(candidate.toll.amount_yen, Some(300));
    assert_eq!(result.ranking_mode, "time_per_yen");
    assert_eq!(result.min_plan_seconds, Some(1_707));

    // 最短計画 1,610 秒 (≒26.83 分) の成立境界を固定する。27 分上限では同じ
    // 宝町候補が成立し、26 分上限では最近接 tier (宝町入口) が完全評価されたうえで
    // その唯一の合法周回が上限を超える。最近接 tier の診断が確定するため、
    // 遠方入口を探さず `TIME_WINDOW` と証明済み `minPlanSeconds` を返す。
    let at_plan_boundary = search(&g, &request("req-tokyo-max-27", 15, 27), &limits)
        .expect("15-27 minute search must not error");
    assert_eq!(at_plan_boundary.status, "no_candidates");
    assert_eq!(at_plan_boundary.reason.as_deref(), Some("TIME_WINDOW"));
    assert_eq!(at_plan_boundary.min_plan_seconds, Some(1_707));

    let below_plan_boundary = search(&g, &request("req-tokyo-max-26", 15, 26), &limits)
        .expect("15-26 minute search must not error");
    assert_eq!(below_plan_boundary.status, "no_candidates");
    assert_eq!(below_plan_boundary.reason.as_deref(), Some("TIME_WINDOW"));
    assert!(below_plan_boundary.candidates.is_empty());
    assert_eq!(below_plan_boundary.min_plan_seconds, Some(1_707));

    // 30〜60 分窓では同じ宝町 tier が長い周回 (20,205 m) を選ぶ。
    let wide_window = search(&g, &request("req-tokyo-30-60", 30, 60), &limits)
        .expect("30-60 minute search must not error");
    assert_eq!(wide_window.status, "no_candidates");
    assert_eq!(wide_window.reason.as_deref(), Some("TIME_WINDOW"));
    assert!(wide_window.candidates.is_empty());
    assert_eq!(wide_window.min_plan_seconds, Some(1_707));

    // 決定論: 同一入力の再実行で JSON が完全一致する。
    let repeat = search(&g, &request("req-tokyo-station", 15, 60), &limits)
        .expect("repeat search must not error");
    assert_eq!(
        serde_json::to_string(&result).unwrap(),
        serde_json::to_string(&repeat).unwrap(),
        "同一入力の再実行はバイト完全一致すること"
    );

    // 最寄り入口は宝町入口（~746 m）。アクセス距離が 2 km 未満であることを確認する。
    let nearest_access_dist = result
        .candidates
        .iter()
        .map(|c| c.snapped_origin().distance_meters)
        .fold(f64::MAX, f64::min);
    assert!(
        nearest_access_dist < 2000.0,
        "東京駅 nearest access must be < 2 km, got {:.0} m",
        nearest_access_dist
    );
}

/// 新宿駅・渋谷駅の最近接入口 tier 実測契約 (Issue #57)。
///
/// 新宿駅 (35.6896, 139.7006) の最寄りは 4号外苑入口 (約 1.56 km) だが、
/// 4号外苑は構造的に出口へ到達できない dead-end tier (合法周回なし) である。
/// そこで fall-through し、同じく合法周回を持たない近接 tier を挟んで、
/// 最初に窓内候補を生む 4号ランプ入口 tier (約 1.74 km) が選ばれる。
/// 共有 Budget は展開状態を forward-reachable 集合へ制限することで
/// 近接 tier を評価し切っても枯渇しない (expanded < maxExpandedStates)。
///
/// 渋谷駅 (35.6580, 139.7016) の最寄りは 3号渋谷入口 (約 612 m) で、
/// 同一施設の対向出口を持つため動的 OD 候補が成立する。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn shinjuku_and_shibuya_stations_return_candidates() {
    let g = real_graph();
    let limits = SearchLimits::default();

    let request = |station: &str, lat: f64, lon: f64| SearchRequest {
        request_id: format!("req-{station}"),
        release_id: "all-real-v4".into(),
        origin_node_id: None,
        origin: Some(LatLng { lat, lon }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 30,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };

    // ── 新宿駅: dead-end な近接 tier を fall-through し、最初に窓内候補を
    //    生む 4号ランプ入口 tier が選ばれる。遠方 Verified 入口へは縮退しない。 ──
    let shinjuku = search(&g, &request("新宿駅", 35.6896, 139.7006), &limits)
        .unwrap_or_else(|e| panic!("新宿駅 search must not error: {e}"));
    eprintln!(
        "新宿駅: status={}, reason={:?}, candidates={}, expanded={}",
        shinjuku.status,
        shinjuku.reason,
        shinjuku.candidates.len(),
        shinjuku.expanded_states
    );
    assert_eq!(
        shinjuku.status, "ok",
        "新宿駅 unlimited entries: 近接 dead-end tier を飛ばして候補が得られること。reason={:?}",
        shinjuku.reason
    );
    assert_eq!(shinjuku.candidates.len(), 1);
    let shinjuku_candidate = topology_only(&shinjuku.candidates[0]);
    assert_eq!(
        shinjuku_candidate.entry.ramp_id.as_deref(),
        Some("ramp:4-outbound:gaien-entry")
    );
    assert_eq!(
        shinjuku_candidate.exit.ramp_id.as_deref(),
        Some("ramp:4-inbound:gaien-exit")
    );
    assert_eq!(shinjuku_candidate.duration.plan_seconds, 3_366);
    assert_eq!(shinjuku_candidate.r#loop.distance_meters, 13_797);
    assert_eq!(shinjuku_candidate.toll.amount_yen, None);
    assert_eq!(shinjuku.ranking_mode, "shutoko_time");
    assert_eq!(shinjuku.min_plan_seconds, Some(3_366));
    assert!(
        shinjuku.expanded_states < limits.max_expanded_states,
        "新宿駅: forward-reachable 制限により Budget を使い切らずに候補へ到達すること (expanded={})",
        shinjuku.expanded_states
    );
    let shinjuku_nearest = shinjuku
        .nearest_access
        .as_ref()
        .expect("coordinate input must report nearestAccess");
    assert!(
        (1_500.0..1_600.0).contains(&shinjuku_nearest.distance_meters),
        "新宿駅 nearest entry must be 4号外苑 (~1.56 km), got {:.0} m",
        shinjuku_nearest.distance_meters
    );

    let shinjuku_repeat = search(&g, &request("新宿駅", 35.6896, 139.7006), &limits)
        .unwrap_or_else(|e| panic!("新宿駅 repeat search must not error: {e}"));
    assert_eq!(
        serde_json::to_string(&shinjuku).unwrap(),
        serde_json::to_string(&shinjuku_repeat).unwrap(),
        "新宿駅: 同一入力の再実行はバイト完全一致すること"
    );

    // ── 渋谷駅: 最近接の 3号渋谷入口 tier が動的 OD 候補を返す。 ──
    let shibuya = search(&g, &request("渋谷駅", 35.6580, 139.7016), &limits)
        .unwrap_or_else(|e| panic!("渋谷駅 search must not error: {e}"));
    eprintln!(
        "渋谷駅: status={}, reason={:?}, candidates={}, expanded={}",
        shibuya.status,
        shibuya.reason,
        shibuya.candidates.len(),
        shibuya.expanded_states
    );
    assert_eq!(
        shibuya.status, "ok",
        "渋谷駅 unlimited entries: 候補が得られること。reason={:?}",
        shibuya.reason
    );
    assert_eq!(shibuya.candidates.len(), 1);
    let candidate = topology_only(&shibuya.candidates[0]);
    assert_eq!(
        candidate.entry.ramp_id.as_deref(),
        Some("ramp:3-outbound:shibuya-entry")
    );
    assert_eq!(
        candidate.exit.ramp_id.as_deref(),
        Some("ramp:3-outbound:shibuya-exit")
    );
    assert_eq!(candidate.duration.plan_seconds, 3_080);
    assert_eq!(candidate.toll.amount_yen, None);
    assert_eq!(shibuya.ranking_mode, "shutoko_time");
    assert_eq!(shibuya.min_plan_seconds, Some(3_080));
    assert!(
        shibuya.expanded_states <= limits.max_expanded_states,
        "渋谷駅: expanded states は上限内であること"
    );

    let repeat = search(&g, &request("渋谷駅", 35.6580, 139.7016), &limits)
        .unwrap_or_else(|e| panic!("渋谷駅 repeat search must not error: {e}"));
    assert_eq!(
        serde_json::to_string(&shibuya).unwrap(),
        serde_json::to_string(&repeat).unwrap(),
        "渋谷駅: 同一入力の再実行はバイト完全一致すること"
    );
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
        release_id: "all-real-v4".into(),
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
/// 全線実データ境界。Issue #57 の最近接入口優先により、いずれの地点も
/// 最寄りの routable 入口 tier (日野・八王子: K7横浜青葉、立川: 4号高井戸)
/// から動的 OD 候補が成立する。立川は deprecated assignment のため未計算、
/// 日野・八王子も料金未算出なのでいずれも `shutoko_time` になる。
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
    // 最寄り4号高井戸入口は deprecated assignment のため、dynamic OD として未計算とする。
    let tachikawa_candidate = topology_only(&tachikawa_wide.candidates[0]);
    assert_eq!(
        tachikawa_candidate.entry.ramp_id.as_deref(),
        Some("ramp:4-inbound:takaido-entry")
    );
    assert_eq!(
        tachikawa_candidate.exit.ramp_id.as_deref(),
        Some("ramp:4-outbound:takaido-exit")
    );
    assert_eq!(tachikawa_candidate.toll.amount_yen, None);
    assert_eq!(
        tachikawa_candidate.tariff_status,
        shutoko_routing_core::TariffStatus::Unpriced
    );
    assert_eq!(
        tachikawa_candidate.eligibility_status,
        shutoko_routing_core::PairEligibilityStatus::TopologyOnly
    );
    assert_eq!(tachikawa_candidate.duration.plan_seconds, 11_790);
    assert_eq!(tachikawa_min_plan, 10_727);
    assert_eq!(tachikawa_wide.ranking_mode, "shutoko_time");

    // ── 4. 八王子駅: 最近接の K7横浜青葉入口 tier が動的 OD 候補を返す ──
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
    assert_eq!(
        hachioji_wide.status, "ok",
        "hachioji with 46 km cap must produce candidates; reason={:?}",
        hachioji_wide.reason
    );
    assert!(!hachioji_wide.candidates.is_empty());
    let hachioji_nearest = hachioji_wide
        .nearest_access
        .as_ref()
        .expect("hachioji must report nearestAccess for coordinate input");
    assert!(
        (21_000.0..22_000.0).contains(&hachioji_nearest.distance_meters),
        "hachioji nearest entry must be ~21.4 km, got {:.0} m",
        hachioji_nearest.distance_meters
    );
    let hachioji_candidate = topology_only(&hachioji_wide.candidates[0]);
    assert_eq!(
        hachioji_candidate.entry.ramp_id.as_deref(),
        Some("ramp:k7-inbound:yokohama-aoba-entry")
    );
    assert_eq!(
        hachioji_candidate.exit.ramp_id.as_deref(),
        Some("ramp:k7-outbound:yokohama-aoba-exit")
    );
    assert_eq!(hachioji_candidate.toll.amount_yen, None);
    assert_eq!(hachioji_candidate.duration.plan_seconds, 13_377);
    assert_eq!(hachioji_wide.ranking_mode, "shutoko_time");
    let hachioji_min_plan = hachioji_wide
        .min_plan_seconds
        .expect("hachioji must report minPlanSeconds when candidates exist");
    assert_eq!(hachioji_min_plan, 13_377);
    assert!(
        hachioji_min_plan <= FOUR_HOURS_SECONDS,
        "hachioji minPlanSeconds ({hachioji_min_plan}) must fit the 240 min window"
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
            release_id: "all-real-v4".into(),
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
// Issue #57: narrow-window coordinate searches report the nearest tier's
// diagnostic instead of falling through.
//
// Under nearest-entry-tier semantics the nearest tier (Tachikawa: 4号高井戸
// 18.6 km / Hachioji: K7横浜青葉 21.4 km) is fully evaluated and exposes legal
// loops, but every one of them exceeds the 60-minute window. Because the nearest
// access tier owns the diagnostic, the search stops there and reports
// `TIME_WINDOW` with the proven `minPlanSeconds` — the same product behavior the
// coordinate search had before Issue #57 and the value the UI recovery path
// (widen the upper bound / lower the minimum) is built on. A farther entry is
// never silently selected just to satisfy the window.
// ---------------------------------------------------------------------------

/// 立川駅・八王子駅の 60 分窓 (Issue #57 最近接入口 tier 実測契約)。
///
/// 最近接 tier は完全評価され、合法周回が 60 分窓に収まらないため、
/// `TIME_WINDOW` と証明済み `minPlanSeconds` を返す。併せて同一入力の決定論と
/// 10 秒以内の応答を確認する。
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn tokyo_wide_narrow_window_reports_nearest_tier_time_window() {
    let g = real_graph();
    let wide = wide_access_limits();

    // ── 立川駅 60 分窓: 最近接 tier は完全評価済み・時間外 → TIME_WINDOW ──
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
    assert!(tachikawa.candidates.is_empty());
    assert_eq!(
        tachikawa.min_plan_seconds,
        Some(10_727),
        "完全評価済みの最近接 tier の最短計画を証明できること"
    );
    assert!(
        tachikawa.expanded_states < wide.max_expanded_states,
        "診断確定は Budget を使い切らずに完了すること (expanded={})",
        tachikawa.expanded_states
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "narrow-window diagnostic must answer within 10 s, took {elapsed:?}"
    );

    // ── 決定論: 同一入力の再実行で結果が一致する ──
    let repeat = search(
        &g,
        &coordinate_request("req-tachikawa-narrow", 35.6979, 139.4139, 60),
        &wide,
    )
    .expect("repeat search must not error");
    assert_eq!(repeat.status, tachikawa.status);
    assert_eq!(repeat.reason, tachikawa.reason);
    assert_eq!(repeat.expanded_states, tachikawa.expanded_states);
    assert_eq!(repeat.min_plan_seconds, tachikawa.min_plan_seconds);
    assert_eq!(repeat.candidates.len(), tachikawa.candidates.len());
    assert_eq!(
        serde_json::to_string(&repeat).unwrap(),
        serde_json::to_string(&tachikawa).unwrap()
    );

    // ── 八王子駅 60 分窓: 同じく最近接 tier の TIME_WINDOW 診断 ──
    let hachioji = search(
        &g,
        &coordinate_request("req-hachioji-narrow", 35.6556, 139.3388, 60),
        &wide,
    )
    .expect("hachioji narrow search must not error");
    assert_eq!(hachioji.status, "no_candidates");
    assert_eq!(hachioji.reason.as_deref(), Some("TIME_WINDOW"));
    assert!(hachioji.candidates.is_empty());
    assert_eq!(hachioji.min_plan_seconds, Some(13_377));
    assert!(hachioji.expanded_states < wide.max_expanded_states);
}

#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn meguro_station_all_real_v4_end_to_end_contract() {
    let graph = real_graph();
    assert_eq!(graph.billing_pairs.len(), 8);
    assert!(graph
        .billing_pairs
        .iter()
        .all(|pair| pair.id.starts_with("bp:c1-")));

    let request = SearchRequest {
        request_id: "req-meguro-all-real-v4-contract".into(),
        release_id: "all-real-v4".into(),
        origin_node_id: None,
        origin: Some(LatLng {
            lat: 35.635681,
            lon: 139.718489,
        }),
        entry_ramp_id: None,
        exit_ramp_id: None,
        min_minutes: 15,
        max_minutes: 60,
        vehicle_profile: "passenger-car-etc".into(),
        pricing_at: "2026-09-10T00:00:00Z".into(),
    };
    let result = search(&graph, &request, &SearchLimits::default())
        .expect("Meguro station search must succeed on all-real-v4");

    assert_eq!(result.status, "ok");
    assert_eq!(result.ranking_mode, "shutoko_time");
    assert_eq!(
        result
            .nearest_access
            .as_ref()
            .map(|access| access.node_id.as_str()),
        Some("n:2177935837")
    );
    assert!(!result.candidates.is_empty());
    for candidate in &result.candidates {
        assert!(candidate.as_radial().is_none());
        let candidate = topology_only(candidate);
        assert_eq!(
            candidate.entry.ramp_id.as_deref(),
            Some("ramp:2-inbound:meguro-entry")
        );
        assert_eq!(
            candidate.exit.ramp_id.as_deref(),
            Some("ramp:2-outbound:meguro-exit")
        );
        assert_eq!(
            candidate.eligibility_status,
            shutoko_routing_core::PairEligibilityStatus::TopologyOnly
        );
        assert_eq!(
            candidate.loop_validation_status,
            shutoko_routing_core::LoopValidationStatus::TopologyOnly
        );
        assert_eq!(candidate.reasons, vec!["TOPOLOGY_ONLY"]);
        let wire = serde_json::to_value(candidate).unwrap();
        assert!(wire["toll"].get("chargedSectionCount").is_none());
        assert!(wire["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .all(|reason| reason != "ONE_SECTION_TOLL"));
    }

    let generated_graph: Value = serde_json::from_str(real_graph_str()).unwrap();
    assert_eq!(
        generated_graph["billingPairs"].as_array().unwrap().len(),
        10
    );
    assert_eq!(
        generated_graph["billingPairs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|pair| pair["pairKind"] == "radialReturn")
            .count(),
        2
    );
    let manifest: Value =
        serde_json::from_str(include_str!("../../../fixtures/generated/manifest.json")).unwrap();
    let diagnostic_only = manifest["unverifiedSections"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|section| section.starts_with("diagnostic-only:"))
        .collect::<Vec<_>>();
    assert!(diagnostic_only.is_empty());
}

// ---------------------------------------------------------------------------
// Issue #57 acceptance: Meguro coordinates must select the nearest entry.
//
// Before the fix, automatic coordinate search only scanned static Verified
// billing pairs, so both Meguro coordinates fell back to distant C1 entries.
// The nearest-entry-tier path now pairs the nearest GeneralEntry
// (ramp:2-inbound:meguro-entry) with its same-facility exit
// (ramp:2-outbound:meguro-exit) as a dynamic OD: no tariff is defined, so
// `toll.amountYen` stays null and ranking uses `shutoko_time`.
// ---------------------------------------------------------------------------

/// Issue #57 acceptance: both Meguro coordinates, every window, must return
/// `ok` candidates whose entry is `ramp:2-inbound:meguro-entry`.
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn meguro_coordinates_select_nearest_entry_across_windows() {
    let g = real_graph();
    let limits = SearchLimits::default();

    // (lat, lon, min, max, expected planSeconds, expected minPlanSeconds, loop metres)
    type MeguroCase = ((f64, f64), (u64, u64, u64, u64, u64));
    let cases: [MeguroCase; 6] = [
        ((35.635681, 139.718489), (15, 60, 2_884, 2_884, 13_797)),
        ((35.635681, 139.718489), (30, 120, 2_884, 2_884, 13_797)),
        ((35.635681, 139.718489), (52, 120, 4_074, 2_884, 25_205)),
        ((35.63239, 139.71524), (15, 60, 3_059, 3_059, 13_797)),
        ((35.63239, 139.71524), (30, 120, 3_059, 3_059, 13_797)),
        ((35.63239, 139.71524), (52, 120, 4_250, 3_059, 25_205)),
    ];

    for ((lat, lon), (min, max, plan, min_plan, loop_meters)) in cases {
        let request = SearchRequest {
            request_id: format!("req-meguro-{lat}-{lon}-{min}-{max}"),
            release_id: "all-real-v4".into(),
            origin_node_id: None,
            origin: Some(LatLng { lat, lon }),
            entry_ramp_id: None,
            exit_ramp_id: None,
            min_minutes: min,
            max_minutes: max,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = search(&g, &request, &limits).unwrap_or_else(|e| {
            panic!("meguro ({lat}, {lon}) {min}-{max} search must not error: {e}")
        });
        eprintln!(
            "meguro ({lat}, {lon}) {min}-{max}: status={}, reason={:?}, candidates={}, expanded={}, minPlan={:?}",
            result.status,
            result.reason,
            result.candidates.len(),
            result.expanded_states,
            result.min_plan_seconds
        );

        assert_eq!(
            result.status, "ok",
            "meguro ({lat}, {lon}) {min}-{max}: reason={:?}",
            result.reason
        );
        assert!(
            !result.candidates.is_empty(),
            "meguro ({lat}, {lon}) {min}-{max}: at least one candidate required"
        );
        assert!(
            result.expanded_states <= limits.max_expanded_states,
            "meguro ({lat}, {lon}) {min}-{max}: expanded states {} exceed the configured limit {}",
            result.expanded_states,
            limits.max_expanded_states
        );

        for candidate in &result.candidates {
            let candidate = topology_only(candidate);
            assert_eq!(
                candidate.entry.ramp_id.as_deref(),
                Some("ramp:2-inbound:meguro-entry"),
                "meguro ({lat}, {lon}) {min}-{max}: wrong entry tier"
            );
            assert_eq!(
                candidate.exit.ramp_id.as_deref(),
                Some("ramp:2-outbound:meguro-exit"),
                "meguro ({lat}, {lon}) {min}-{max}: wrong same-facility exit"
            );
            assert_eq!(
                candidate.toll.amount_yen, None,
                "meguro ({lat}, {lon}) {min}-{max}: dynamic OD has no tariff"
            );
            assert_eq!(
                candidate.eligibility_status,
                shutoko_routing_core::PairEligibilityStatus::TopologyOnly
            );
            assert_eq!(
                candidate.loop_validation_status,
                shutoko_routing_core::LoopValidationStatus::TopologyOnly
            );
            assert_eq!(candidate.reasons, vec!["TOPOLOGY_ONLY"]);
            let wire = serde_json::to_value(candidate).unwrap();
            assert!(wire["toll"].get("chargedSectionCount").is_none());
            assert!(wire["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .all(|reason| reason != "ONE_SECTION_TOLL"));
            assert!(
                candidate.toll.toll_source.is_none(),
                "meguro ({lat}, {lon}) {min}-{max}: unknown toll must not claim a source"
            );
        }
        assert_eq!(
            result.ranking_mode, "shutoko_time",
            "meguro ({lat}, {lon}) {min}-{max}: unknown toll ranks by shutoko time"
        );

        let candidate = topology_only(&result.candidates[0]);
        assert_eq!(
            candidate.duration.plan_seconds, plan,
            "meguro ({lat}, {lon}) {min}-{max}: pinned plan seconds"
        );
        assert_eq!(
            candidate.r#loop.distance_meters, loop_meters,
            "meguro ({lat}, {lon}) {min}-{max}: pinned loop metres"
        );
        assert_eq!(
            result.min_plan_seconds,
            Some(min_plan),
            "meguro ({lat}, {lon}) {min}-{max}: pinned minimum plan seconds"
        );

        // Repeated calls must be byte-for-byte deterministic.
        let repeat = search(&g, &request, &limits)
            .unwrap_or_else(|e| panic!("meguro repeat search must not error: {e}"));
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&repeat).unwrap(),
            "meguro ({lat}, {lon}) {min}-{max}: repeated calls must be deterministic"
        );
    }
}

// ---------------------------------------------------------------------------
// Issue #57 acceptance: explicit ramp mode must still select the Meguro entry
// pair directly (review V5-06).
//
// The coordinate path above proves the nearest-entry tier resolves to
// `ramp:2-inbound:meguro-entry`. This test pins the explicit
// `entryRampId`/`exitRampId` path independently so the refactor that introduced
// `evaluate_dynamic_od` cannot silently regress explicit ramp routing.
// ---------------------------------------------------------------------------

/// Explicit 目黒入口 → 目黒出口 across every Issue #57 window.
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn meguro_explicit_ramp_pair_matches_coordinate_route() {
    let g = real_graph();
    let limits = SearchLimits::default();

    // (min, max, expected planSeconds, expected minPlanSeconds, loop metres)
    let cases: [(u64, u64, u64, u64, u64); 3] = [
        (15, 60, 2_884, 2_884, 13_797),
        (30, 120, 2_884, 2_884, 13_797),
        (52, 120, 4_074, 2_884, 25_205),
    ];

    for (min, max, plan, min_plan, loop_meters) in cases {
        let request = SearchRequest {
            request_id: format!("req-meguro-explicit-{min}-{max}"),
            release_id: "all-real-v4".into(),
            origin_node_id: None,
            origin: Some(LatLng {
                lat: 35.635681,
                lon: 139.718489,
            }),
            entry_ramp_id: Some("ramp:2-inbound:meguro-entry".into()),
            exit_ramp_id: Some("ramp:2-outbound:meguro-exit".into()),
            min_minutes: min,
            max_minutes: max,
            vehicle_profile: "passenger-car-etc".into(),
            pricing_at: "2026-09-10T00:00:00Z".into(),
        };
        let result = search(&g, &request, &limits)
            .unwrap_or_else(|e| panic!("explicit meguro {min}-{max} search must not error: {e}"));
        assert_eq!(
            result.status, "ok",
            "explicit meguro {min}-{max}: reason={:?}",
            result.reason
        );
        assert_eq!(result.candidates.len(), 1);
        let candidate = topology_only(&result.candidates[0]);
        assert_eq!(
            candidate.entry.ramp_id.as_deref(),
            Some("ramp:2-inbound:meguro-entry")
        );
        assert_eq!(
            candidate.exit.ramp_id.as_deref(),
            Some("ramp:2-outbound:meguro-exit")
        );
        assert_eq!(
            candidate.duration.plan_seconds, plan,
            "explicit meguro {min}-{max}: same plan as the coordinate path"
        );
        assert_eq!(
            candidate.r#loop.distance_meters, loop_meters,
            "explicit meguro {min}-{max}: same loop as the coordinate path"
        );
        assert_eq!(
            candidate.toll.amount_yen, None,
            "explicit meguro {min}-{max}: no tariff for this OD"
        );
        assert_eq!(result.ranking_mode, "shutoko_time");
        assert_eq!(result.min_plan_seconds, Some(min_plan));
        assert!(result.expanded_states <= limits.max_expanded_states);

        let repeat = search(&g, &request, &limits)
            .unwrap_or_else(|e| panic!("explicit meguro repeat must not error: {e}"));
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&repeat).unwrap(),
            "explicit meguro {min}-{max}: repeated calls must be deterministic"
        );
    }
}

/// 4 地点（東京駅・目黒駅・銀座・六本木）の探索結果を実 engine で固定する。
///
/// 期待値は fixtures/representative-locations.json に置き、Web の実 WASM 統合テスト
/// （web/test/integration-wasm.test.ts）と同じ項目を engine 側でも照合する。
/// - 最寄りの入口（access node と距離、候補の入口ランプ）
/// - 候補の件数・並び・pairId / pairKind
/// - 。”（`tariffStatus` と 2026-10 改定をまたぐ 2 時点の金額・規則 ID・証拠 ID・適用期間）
/// - 推薦バッジが先頭 1 件だけであること
/// - 同じ入力の再実行がバイト完全一致（並びも決定的）であること
#[test]
#[ignore = "real-graph search is slow in debug; run with --release -- --ignored (CI does)"]
fn representative_locations_release_v4_contract() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/representative-locations.json"
    ))
    .expect("representative-locations.json must be valid JSON");
    let graph_json = real_graph_str();
    let wire: Value = serde_json::from_str(graph_json).unwrap();
    assert_eq!(
        wire["releaseId"], fixture["releaseId"],
        "release id must match the fixture"
    );
    let limits_json = format!("{{\"maxAccessDistanceMeters\":{WIDE_ACCESS_CAP_METERS}}}");
    let windows = fixture["pricingWindows"].as_array().unwrap();
    assert_eq!(
        windows.len(),
        2,
        "the 2026-10 revision must be pinned on both sides"
    );

    for location in fixture["locations"].as_array().unwrap() {
        let id = location["id"].as_str().unwrap();
        let label = location["label"].as_str().unwrap();
        for window in windows {
            let window_id = window["id"].as_str().unwrap();
            let pricing_at = window["pricingAt"].as_str().unwrap();
            let where_ = format!("{label} ({id}) {window_id}");
            let request_json = serde_json::json!({
                "requestId": format!("representative-{id}-{window_id}"),
                "releaseId": fixture["releaseId"],
                "origin": location["origin"],
                "minMinutes": location["minMinutes"],
                "maxMinutes": location["maxMinutes"],
                "vehicleProfile": fixture["vehicleProfile"],
                "pricingAt": pricing_at,
            })
            .to_string();
            let result_json = search_json(graph_json, &request_json, &limits_json)
                .unwrap_or_else(|e| panic!("{where_}: search must succeed: {e}"));
            let repeat_json = search_json(graph_json, &request_json, &limits_json)
                .unwrap_or_else(|e| panic!("{where_}: repeat search must succeed: {e}"));
            assert_eq!(
                result_json, repeat_json,
                "{where_}: search must be deterministic"
            );
            let result: Value = serde_json::from_str(&result_json).unwrap();

            assert_eq!(result["status"], location["expectedStatus"], "{where_}");
            assert_eq!(result["reason"], location["expectedReason"], "{where_}");
            assert_eq!(
                result["rankingMode"], location["expectedRankingMode"],
                "{where_}"
            );
            let candidates = result["candidates"].as_array().unwrap();
            assert_eq!(
                candidates.len() as u64,
                location["expectedCandidateCount"].as_u64().unwrap(),
                "{where_}: candidate count"
            );
            assert!(
                candidates.len() <= 3,
                "{where_}: displayed candidates are capped at 3"
            );

            // 最寄りの入口: access node と距離（1 m 許容）。
            let nearest = &result["nearestAccess"];
            assert_eq!(
                nearest["nodeId"], location["expectedNearestAccess"]["nodeId"],
                "{where_}: nearest access node"
            );
            let distance = nearest["distanceMeters"].as_f64().unwrap();
            let expected_distance = location["expectedNearestAccess"]["distanceMeters"]
                .as_f64()
                .unwrap();
            assert!(
                (distance - expected_distance).abs() <= 1.0,
                "{where_}: nearest access distance {distance} vs {expected_distance}"
            );

            let expected_candidates = location["expectedCandidates"].as_array().unwrap();
            let mut badges = 0usize;
            for (index, expected) in expected_candidates.iter().enumerate() {
                let candidate = &candidates[index];
                let pair_id = expected["pairId"].as_str().unwrap();
                let where_pair = format!("{where_} {pair_id}");
                assert_eq!(candidate["toll"]["billingPairId"], pair_id, "{where_pair}");
                assert_eq!(
                    candidate["pairKind"], expected["pairKind"],
                    "{where_pair}: pair kind"
                );
                assert_eq!(
                    candidate["entry"]["rampId"], expected["entryRampId"],
                    "{where_pair}: 候補は最寄りの入口 tier からしか返らない"
                );
                assert_eq!(
                    candidate["exit"]["rampId"], expected["exitRampId"],
                    "{where_pair}"
                );
                assert_eq!(
                    candidate["entry"]["name"], expected["entryName"],
                    "{where_pair}"
                );
                assert_eq!(
                    candidate["exit"]["name"], expected["exitName"],
                    "{where_pair}"
                );
                assert_eq!(
                    candidate["duration"]["shutokoSeconds"], expected["shutokoSeconds"],
                    "{where_pair}: shutoko seconds"
                );
                assert_eq!(
                    candidate["duration"]["planSeconds"], expected["planSeconds"],
                    "{where_pair}: plan seconds"
                );

                // 推薦バッジは fixture が指定した 1 件だけ。
                let recommended = expected["recommended"].as_bool().unwrap();
                let has_badge = candidate["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reason| reason == "BEST_TIME_PER_YEN" || reason == "BEST_SHUTOKO_TIME");
                assert_eq!(has_badge, recommended, "{where_pair}: recommended badge");
                if has_badge {
                    badges += 1;
                }
                // 商品対象外の候補は推薦しない。
                assert!(
                    !has_badge || is_product_eligible(candidate),
                    "{where_pair}: an ineligible candidate must not be recommended"
                );

                // 2026-10 改定をまたぐ料金。規則 ID と証拠 ID は期間ごとに別レコード。
                let pricing = expected["pricing"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["window"] == window["id"])
                    .unwrap_or_else(|| panic!("{where_pair}: pricing window is missing"));
                assert_eq!(
                    candidate["tariffStatus"], pricing["tariffStatus"],
                    "{where_pair}: tariff status"
                );
                assert_eq!(
                    candidate["toll"]["amountYen"], pricing["amountYen"],
                    "{where_pair}"
                );
                assert_eq!(
                    candidate["toll"]["billingDistanceMeters"], pricing["billingDistanceMeters"],
                    "{where_pair}: billing distance"
                );
                assert_eq!(
                    candidate["toll"]["effectiveFrom"], pricing["effectiveFrom"],
                    "{where_pair}: effective from"
                );
                assert_eq!(
                    candidate["toll"]["effectiveTo"], pricing["effectiveTo"],
                    "{where_pair}: effective to"
                );
                if pricing["tariffStatus"] == "priced" {
                    assert_eq!(
                        candidate["toll"]["fareLabel"], fixture["fareLabel"],
                        "{where_pair}"
                    );
                    assert_eq!(
                        candidate["toll"]["assignmentId"], expected["assignmentId"],
                        "{where_pair}"
                    );
                    assert_eq!(
                        candidate["toll"]["ruleId"], pricing["ruleId"],
                        "{where_pair}"
                    );
                    assert_eq!(
                        candidate["toll"]["evidenceId"], pricing["evidenceId"],
                        "{where_pair}"
                    );
                    assert_eq!(
                        candidate["toll"]["distanceEvidenceId"], pricing["distanceEvidenceId"],
                        "{where_pair}"
                    );
                    assert_eq!(
                        candidate["toll"]["tollSource"], fixture["tollSource"],
                        "{where_pair}"
                    );
                } else {
                    // 確定しない候補は金額も証拠も持たない。
                    for field in [
                        "amountYen",
                        "assignmentId",
                        "ruleId",
                        "evidenceId",
                        "tollSource",
                    ] {
                        assert!(
                            candidate["toll"][field].is_null(),
                            "{where_pair}: {field} must stay null for an unpriced candidate"
                        );
                    }
                }
            }

            // 推薦バッジは表示範囲に高々 1 件。
            assert!(
                badges <= 1,
                "{where_}: at most one candidate is recommended"
            );

            // time_per_yen の主規則（円あたり首都高走行時間）は降順。並びは決定的。
            if result["rankingMode"] == "time_per_yen" {
                let efficiency = candidates
                    .iter()
                    .map(|candidate| {
                        let amount = candidate["toll"]["amountYen"].as_f64().unwrap();
                        assert!(amount > 0.0, "{where_}: priced candidates need an amount");
                        candidate["duration"]["shutokoSeconds"].as_f64().unwrap() / amount
                    })
                    .collect::<Vec<_>>();
                for pair in efficiency.windows(2) {
                    assert!(
                        pair[0] >= pair[1],
                        "{where_}: shutoko seconds per yen must not increase: {efficiency:?}"
                    );
                }
            }
        }
    }
}

/// 商品対象の判定（UI と同じ規則）。topologyOnly と未検証の radial は対象外で、
/// legacyRing は常に対象。
fn is_product_eligible(candidate: &Value) -> bool {
    match candidate["pairKind"].as_str().unwrap_or("legacyRing") {
        "topologyOnly" => false,
        "radialReturn" => {
            candidate["eligibilityStatus"] == "verified_one_section_ahead"
                && candidate["loopValidationStatus"] == "declared_route_validated"
        }
        _ => true,
    }
}
