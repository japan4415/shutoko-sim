use serde_json::{json, Value};
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
