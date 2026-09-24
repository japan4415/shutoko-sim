use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shutoko_routing_core::{
    search_json, validate_radial_return_candidate, validate_topology_only_candidate, Candidate,
    RadialReturnCandidate, TopologyOnlyCandidate,
};

fn valid() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/candidate-v2/radial-valid.json"
    ))
    .unwrap()
}

fn parse(value: &Value) -> Result<RadialReturnCandidate, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn schema4_graph() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/graph-v4/graph-radial-fixture.json"
    ))
    .unwrap()
}

fn run(graph: &Value, request: Value) -> Value {
    serde_json::from_str(
        &search_json(
            &graph.to_string(),
            &request.to_string(),
            &json!({}).to_string(),
        )
        .unwrap(),
    )
    .unwrap()
}

fn edge_hash(edge_ids: &[&str]) -> String {
    let edge_ids = edge_ids
        .iter()
        .map(|edge_id| (*edge_id).to_owned())
        .collect::<Vec<_>>();
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&edge_ids).unwrap());
    format!("{:x}", hasher.finalize())
}

#[test]
fn accepts_radial_candidate_with_contiguous_hashed_legs() {
    let candidate = parse(&valid()).unwrap();
    validate_radial_return_candidate(&candidate).unwrap();
}

#[test]
fn rejects_duplicate_and_missing_edge_route_legs() {
    let duplicate: Value = serde_json::from_str(include_str!(
        "../../../fixtures/candidate-v2/invalid-edge-route-legs-duplicate.json"
    ))
    .unwrap();
    let mut value = valid();
    value["edgeRouteLegs"] = duplicate["edgeRouteLegs"].clone();
    let candidate = parse(&value).unwrap();
    assert!(validate_radial_return_candidate(&candidate).is_err());

    let missing: Value = serde_json::from_str(include_str!(
        "../../../fixtures/candidate-v2/invalid-edge-route-legs-missing.json"
    ))
    .unwrap();
    let mut value = valid();
    value["edgeRouteLegs"] = missing["edgeRouteLegs"].clone();
    let candidate = parse(&value).unwrap();
    assert!(validate_radial_return_candidate(&candidate).is_err());
}

#[test]
fn rejects_unknown_version_kind_anchor_and_legacy_charge_field() {
    for mutate in [
        |value: &mut Value| value["routePlanVersion"] = Value::from(2),
        |value: &mut Value| value["pairKind"] = Value::from("futurePair"),
        |value: &mut Value| value["anchor"]["anchorKind"] = Value::from("sameNode"),
        |value: &mut Value| value["toll"]["chargedSectionCount"] = Value::from(1),
    ] {
        let mut value = valid();
        mutate(&mut value);
        let rejected = match parse(&value) {
            Ok(candidate) => validate_radial_return_candidate(&candidate).is_err(),
            Err(_) => true,
        };
        assert!(rejected);
    }
}

#[test]
fn rejects_malformed_disabled_or_enabled_radial_handoff() {
    for mutate in [
        |value: &mut Value| value["handoff"]["enabled"] = Value::from(true),
        |value: &mut Value| value["handoff"]["disabledReason"] = Value::from("other"),
        |value: &mut Value| {
            value["handoff"]["legUrls"] = json!([{
                "role": "surface_access",
                "mapsUrl": "https://www.google.com/maps/dir/?api=1",
                "urlSha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }]);
        },
    ] {
        let mut value = valid();
        mutate(&mut value);
        let candidate = parse(&value).unwrap();
        assert!(validate_radial_return_candidate(&candidate).is_err());
    }
}

#[test]
fn rejects_hashes_that_do_not_match_their_leg_slice() {
    let mut value = valid();
    value["routePlan"]["resolvedRouteSegments"][1]["edgeIdsSha256"] =
        Value::from("0000000000000000000000000000000000000000000000000000000000000000");
    let candidate = parse(&value).unwrap();
    assert!(validate_radial_return_candidate(&candidate).is_err());
}

#[test]
fn duration_overflow_is_rejected_without_panicking() {
    let mut value = valid();
    value["duration"]["accessSeconds"] = Value::from(u64::MAX);
    value["duration"]["shutokoSeconds"] = Value::from(1_u64);
    let candidate = parse(&value).unwrap();
    assert!(validate_radial_return_candidate(&candidate).is_err());
}

#[test]
fn schema4_radial_pair_search_builds_complete_radial_candidate() {
    let result = run(
        &schema4_graph(),
        json!({
            "requestId": "radial-search",
            "releaseId": "graph-v4-fixture-v1",
            "originNodeId": "fixture:node:entry:ground",
            "minMinutes": 1,
            "maxMinutes": 60,
            "vehicleProfile": "passenger-car-etc",
            "pricingAt": "2026-09-16T00:00:00Z"
        }),
    );
    assert_eq!(result["status"], "ok");
    let candidate = &result["candidates"][0];
    assert_eq!(candidate["pairKind"], "radialReturn");
    assert_eq!(candidate["eligibilityStatus"], "verified_one_section_ahead");
    assert_eq!(
        candidate["loopValidationStatus"],
        "declared_route_validated"
    );
    assert_eq!(candidate["tariffStatus"], "unpriced");
    assert_eq!(candidate["duration"]["shutokoSeconds"], 1440);
    assert_eq!(candidate["shutokoDistanceMeters"], 23400);
    assert!(candidate["toll"].get("chargedSectionCount").is_none());
    assert!(candidate["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .all(|reason| reason != "ONE_SECTION_TOLL"));
    assert_eq!(candidate["edgeRouteLegs"].as_array().unwrap().len(), 4);
    assert_eq!(candidate["estimatedLegs"].as_array().unwrap().len(), 2);
    assert_eq!(
        candidate["handoff"],
        json!({
            "enabled": false,
            "legUrls": [],
            "disabledReason": "device_verification_pending"
        })
    );
    let surface_distance = candidate["estimatedLegs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|leg| leg["distanceMeters"].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(
        candidate["distanceMeters"].as_u64().unwrap(),
        candidate["shutokoDistanceMeters"].as_u64().unwrap() + surface_distance
    );
    let parsed: RadialReturnCandidate = serde_json::from_value(candidate.clone()).unwrap();
    validate_radial_return_candidate(&parsed).unwrap();
}

#[test]
fn priced_radial_candidates_rank_by_time_per_yen_before_shutoko_time() {
    let mut graph = schema4_graph();
    let long_lap_edges = [
        "fixture:edge:lap:1",
        "fixture:edge:lap:long",
        "fixture:edge:lap:2",
    ];
    let long_lap_hash = edge_hash(&long_lap_edges);
    graph["edges"].as_array_mut().unwrap().push(json!({
        "id": "fixture:edge:lap:long",
        "from": "fixture:node:lap:mid",
        "to": "fixture:node:lap:mid",
        "kind": "shutoko",
        "durationSeconds": 1800,
        "distanceMeters": 10000
    }));
    graph["routeMemberships"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|membership| membership["membershipId"] == "route:fixture:loop:forward")
        .unwrap()["segments"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "segmentId": "relation:loop:long:main",
            "sourceKind": "relationMainline",
            "sourceRelationId": "fixture:relation:loop:long",
            "sourceSnapshotSha256": "b2a0b24aa896e9d92425ff81539194531e036bda0764aa0792f4cbadf61c044a",
            "bindingEvidenceId": null,
            "orderedEdgeIds": long_lap_edges,
            "orderedEdgeIdsSha256": long_lap_hash
        }));

    let mut expensive = graph["billingPairs"][1].clone();
    expensive["id"] = json!("fixture:radial-expensive");
    expensive["resolvedRouteSegments"][1]["sourceSegmentIds"] = json!(["relation:loop:long:main"]);
    expensive["resolvedRouteSegments"][1]["edgeIds"] = json!(long_lap_edges);
    expensive["resolvedRouteSegments"][1]["edgeIdsSha256"] = json!(long_lap_hash);
    expensive["tariff"] = json!({
        "status": "priced",
        "amountYen": 1000,
        "billingDistanceMeters": 23400,
        "prices": [{
            "amountYen": 1000,
            "effectiveFrom": "2026-01-01T00:00:00Z",
            "effectiveTo": null
        }]
    });
    graph["billingPairs"][1]["tariff"] = json!({
        "status": "priced",
        "amountYen": 100,
        "billingDistanceMeters": 23400,
        "prices": [{
            "amountYen": 100,
            "effectiveFrom": "2026-01-01T00:00:00Z",
            "effectiveTo": null
        }]
    });
    graph["billingPairs"]
        .as_array_mut()
        .unwrap()
        .push(expensive);

    let result = run(
        &graph,
        json!({
            "requestId": "radial-ranking",
            "releaseId": "graph-v4-fixture-v1",
            "originNodeId": "fixture:node:entry:ground",
            "minMinutes": 1,
            "maxMinutes": 120,
            "vehicleProfile": "passenger-car-etc",
            "pricingAt": "2026-09-16T00:00:00Z"
        }),
    );
    assert_eq!(result["status"], "ok");
    assert_eq!(result["rankingMode"], "time_per_yen");
    let candidates = result["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0]["toll"]["billingPairId"], "fixture:radial");
    assert_eq!(candidates[0]["duration"]["shutokoSeconds"], 1440);
    assert_eq!(candidates[0]["reasons"], json!(["BEST_TIME_PER_YEN"]));
    assert_eq!(
        candidates[1]["toll"]["billingPairId"],
        "fixture:radial-expensive"
    );
    assert_eq!(candidates[1]["duration"]["shutokoSeconds"], 3240);
    assert_eq!(candidates[1]["reasons"], json!([]));
}

#[test]
fn radial_tariff_uses_the_price_active_at_pricing_at() {
    for (
        pricing_at,
        expected_status,
        expected_amount,
        expected_effective_from,
        expected_effective_to,
    ) in [
        ("2025-12-31T23:59:59Z", "unpriced", None, None, None),
        (
            "2026-01-01T00:00:00Z",
            "priced",
            Some(500),
            Some("2026-01-01T00:00:00Z"),
            Some("2026-07-01T00:00:00Z"),
        ),
        (
            "2026-07-01T00:00:00Z",
            "priced",
            Some(600),
            Some("2026-07-01T00:00:00Z"),
            Some("2026-10-01T00:00:00Z"),
        ),
        ("2026-10-01T00:00:01Z", "expired", None, None, None),
    ] {
        let mut graph = schema4_graph();
        graph["billingPairs"][1]["tariff"] = json!({
            "status": "priced",
            "amountYen": 500,
            "billingDistanceMeters": 21000,
            "prices": [
                {
                    "amountYen": 500,
                    "effectiveFrom": "2026-01-01T00:00:00Z",
                    "effectiveTo": "2026-07-01T00:00:00Z"
                },
                {
                    "amountYen": 600,
                    "effectiveFrom": "2026-07-01T00:00:00Z",
                    "effectiveTo": "2026-10-01T00:00:00Z"
                }
            ]
        });
        let result = run(
            &graph,
            json!({
                "requestId": "radial-tariff",
                "releaseId": "graph-v4-fixture-v1",
                "originNodeId": "fixture:node:entry:ground",
                "minMinutes": 1,
                "maxMinutes": 60,
                "vehicleProfile": "passenger-car-etc",
                "pricingAt": pricing_at
            }),
        );
        assert_eq!(result["status"], "ok", "{pricing_at}: {result}");
        let candidate = &result["candidates"][0];
        assert_eq!(candidate["tariffStatus"], expected_status, "{pricing_at}");
        assert_eq!(
            candidate["toll"]["amountYen"],
            json!(expected_amount),
            "{pricing_at}"
        );
        assert_eq!(
            candidate["toll"]["effectiveFrom"],
            json!(expected_effective_from),
            "{pricing_at}"
        );
        assert_eq!(
            candidate["toll"]["effectiveTo"],
            json!(expected_effective_to),
            "{pricing_at}"
        );
        let parsed: RadialReturnCandidate = serde_json::from_value(candidate.clone()).unwrap();
        validate_radial_return_candidate(&parsed).unwrap();
    }
}

#[test]
fn radial_candidate_rejects_a_price_outside_its_pricing_time() {
    let mut value = valid();
    value["tariffStatus"] = Value::from("priced");
    value["toll"]["amountYen"] = Value::from(500);
    value["toll"]["effectiveFrom"] = Value::from("2030-01-01T00:00:00Z");
    let candidate = parse(&value).unwrap();
    assert!(validate_radial_return_candidate(&candidate).is_err());
}

#[test]
fn schema4_legacy_pair_search_keeps_legacy_charge_contract() {
    let result = run(
        &schema4_graph(),
        json!({
            "requestId": "legacy-search",
            "releaseId": "graph-v4-fixture-v1",
            "originNodeId": "fixture:node:legacy:entry",
            "minMinutes": 1,
            "maxMinutes": 60,
            "vehicleProfile": "passenger-car-etc",
            "pricingAt": "2026-10-01T00:00:00Z"
        }),
    );
    assert_eq!(result["status"], "ok");
    let candidate = &result["candidates"][0];
    assert_eq!(candidate["pairKind"], "legacyRing");
    assert_eq!(candidate["toll"]["chargedSectionCount"], 1);
    assert!(candidate["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "ONE_SECTION_TOLL"));
}

#[test]
fn schema4_legacy_reverse_lookup_rejects_ambiguous_shared_edge_ramps() {
    let mut graph = schema4_graph();
    let mut duplicate = graph["ramps"][1].clone();
    duplicate["id"] = Value::from("fixture:ramp:exit-duplicate");
    graph["ramps"].as_array_mut().unwrap().push(duplicate);

    let result = run(
        &graph,
        json!({
            "requestId": "legacy-ambiguous-ramp",
            "releaseId": "graph-v4-fixture-v1",
            "originNodeId": "fixture:node:legacy:entry",
            "minMinutes": 1,
            "maxMinutes": 60,
            "vehicleProfile": "passenger-car-etc",
            "pricingAt": "2026-10-01T00:00:00Z"
        }),
    );
    assert_eq!(result["status"], "ok");
    let candidate = &result["candidates"][0];
    assert_eq!(candidate["pairKind"], "legacyRing");
    assert!(candidate["exit"]["rampId"].is_null());
    assert!(candidate["exit"]["route"].is_null());
    assert!(candidate["exit"]["direction"].is_null());
}

#[test]
fn unverified_radial_pair_is_returned_outside_product_cohort() {
    let mut graph = schema4_graph();
    graph["billingPairs"][1]["pairEligibility"] = json!({
        "status": "unverified",
        "oneSectionAheadVerified": false
    });
    let result = run(
        &graph,
        json!({
            "requestId": "radial-unverified",
            "releaseId": "graph-v4-fixture-v1",
            "originNodeId": "fixture:node:entry:ground",
            "minMinutes": 1,
            "maxMinutes": 60,
            "vehicleProfile": "passenger-car-etc",
            "pricingAt": "2026-09-16T00:00:00Z"
        }),
    );
    assert_eq!(result["status"], "ok");
    assert_eq!(result["rankingMode"], "shutoko_time");
    let candidate = &result["candidates"][0];
    assert_eq!(candidate["eligibilityStatus"], "unverified");
    assert_eq!(candidate["reasons"], json!([]));
}

#[test]
fn unresolved_radial_status_is_valid_but_not_recommended() {
    let mut value = valid();
    value["loopValidationStatus"] = Value::from("unresolved");
    let candidate = parse(&value).unwrap();
    validate_radial_return_candidate(&candidate).unwrap();
    assert!(candidate
        .reasons
        .iter()
        .all(|reason| !reason.starts_with("BEST_")));
}

#[test]
fn dynamic_od_search_returns_topology_only_candidate_without_legacy_charge_fields() {
    let mut graph = schema4_graph();
    graph["billingPairs"] = json!([]);
    graph["ramps"][0]["facilityId"] = graph["ramps"][1]["facilityId"].clone();
    graph["edges"].as_array_mut().unwrap().push(json!({
        "id": "fixture:edge:topology:cycle",
        "from": "fixture:node:exit:connector",
        "to": "fixture:node:entry:ramp-end",
        "kind": "shutoko",
        "durationSeconds": 600,
        "distanceMeters": 10000
    }));
    let result = run(
        &graph,
        json!({
            "requestId": "topology-only",
            "releaseId": "graph-v4-fixture-v1",
            "origin": { "lat": 35.1, "lon": 139.1 },
            "minMinutes": 1,
            "maxMinutes": 120,
            "vehicleProfile": "passenger-car-etc",
            "pricingAt": "2026-09-16T00:00:00Z"
        }),
    );
    assert_eq!(result["status"], "ok", "{result}");
    let candidate = &result["candidates"][0];
    assert_eq!(candidate["pairKind"], "topologyOnly");
    assert_eq!(candidate["eligibilityStatus"], "topology_only");
    assert_eq!(candidate["loopValidationStatus"], "topology_only");
    assert_eq!(candidate["tariffStatus"], "unpriced");
    assert!(candidate["toll"].get("chargedSectionCount").is_none());
    assert!(candidate["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .all(|reason| reason == "TOPOLOGY_ONLY"));
    assert!(candidate.get("edgeRouteLegs").is_none());
    let surface_distance = candidate["estimatedLegs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|leg| leg["distanceMeters"].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(
        candidate["distanceMeters"].as_u64().unwrap(),
        candidate["shutokoDistanceMeters"].as_u64().unwrap() + surface_distance
    );
    let parsed: TopologyOnlyCandidate = serde_json::from_value(candidate.clone()).unwrap();
    validate_topology_only_candidate(&parsed).unwrap();
}

#[test]
fn candidate_reader_rejects_unknown_discriminator() {
    let mut value = valid();
    value["pairKind"] = Value::from("futureCandidate");
    assert!(serde_json::from_value::<Candidate>(value).is_err());
}
