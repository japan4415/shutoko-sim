use serde_json::Value;
use shutoko_routing_core::{validate_radial_return_candidate, RadialReturnCandidate};

fn valid() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/candidate-v2/radial-valid.json"
    ))
    .unwrap()
}

fn parse(value: &Value) -> Result<RadialReturnCandidate, serde_json::Error> {
    serde_json::from_value(value.clone())
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
