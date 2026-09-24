use serde_json::Value;
use shutoko_routing_core::handoff::build_split_maps_handoff;
use shutoko_routing_core::{
    evaluate_device_verification_gate, parse_device_verification_manifest, search_json,
    validate_radial_return_candidate, CandidateV2Handoff, DeviceVerificationGateBlocker,
    DeviceVerificationGateDecision, DeviceVerificationManifestError, DeviceVerificationResult,
    Edge, EdgeKind, LatLng, MapsHandoffError, RadialReturnCandidate, SplitMapsHandoff,
    DEVICE_VERIFICATION_MANIFEST_SCHEMA_VERSION, URL_BUILDER_VERSION,
};

const VALID_MANIFEST: &str = include_str!("../../../fixtures/device-verification/valid.json");
const PENDING_MANIFEST: &str = include_str!("../../../data/device-verification-manifest.json");
const INVALID_EXPIRY: &str =
    include_str!("../../../fixtures/device-verification/invalid-expiry.json");
const INVALID_MISSING_ENVIRONMENT: &str =
    include_str!("../../../fixtures/device-verification/invalid-missing-environment.json");
const MANIFEST_SCHEMA: &str =
    include_str!("../../../fixtures/device-verification/device-verification-manifest.schema.json");
const SCHEMA4_GRAPH: &str = include_str!("../../../fixtures/graph-v4/graph-radial-fixture.json");

fn evaluate_gate(
    manifest_json: Option<&str>,
    route_plan_id: &str,
    release_id: &str,
    handoff: &SplitMapsHandoff,
    evaluated_at: &str,
) -> DeviceVerificationGateDecision {
    evaluate_device_verification_gate(
        manifest_json,
        route_plan_id,
        release_id,
        handoff,
        Some(evaluated_at),
    )
}

fn public_handoff(
    generated: &SplitMapsHandoff,
    route_plan_id: &str,
    release_id: &str,
    decision: &DeviceVerificationGateDecision,
) -> Result<CandidateV2Handoff, MapsHandoffError> {
    CandidateV2Handoff::from_device_verification_gate(
        generated,
        route_plan_id,
        release_id,
        decision,
    )
}

fn schema4_search_with_release_gate(
    manifest_json: &str,
    evaluated_at: &str,
    pricing_at: &str,
) -> Value {
    let limits = serde_json::json!({
        "deviceVerification": {
            "manifestJson": manifest_json,
            "evaluatedAt": evaluated_at
        }
    });
    serde_json::from_str(
        &search_json(
            SCHEMA4_GRAPH,
            &serde_json::json!({
                "requestId": "device-gate-search",
                "releaseId": "graph-v4-fixture-v1",
                "originNodeId": "fixture:node:entry:ground",
                "minMinutes": 1,
                "maxMinutes": 60,
                "vehicleProfile": "passenger-car-etc",
                "pricingAt": pricing_at
            })
            .to_string(),
            &limits.to_string(),
        )
        .unwrap(),
    )
    .unwrap()
}

fn passing_schema4_manifest() -> String {
    let mut manifest: Value = serde_json::from_str(PENDING_MANIFEST).unwrap();
    for record in manifest["verifications"].as_array_mut().unwrap() {
        record["osVersion"] = Value::from("test-os");
        record["clientVersion"] = Value::from("test-client");
        record["verifiedAt"] = Value::from("2026-09-24T00:00:00Z");
        record["result"] = Value::from("passed");
        record["expiresAt"] = Value::from("2026-10-24T00:00:00Z");
    }
    manifest.to_string()
}

fn edge(from: &str, to: &str, distance_meters: u64) -> Edge {
    Edge {
        id: format!("edge:{from}:{to}"),
        from: from.to_owned(),
        to: to.to_owned(),
        kind: EdgeKind::Shutoko,
        duration_seconds: 60,
        distance_meters,
        name: None,
    }
}

fn split_fixture() -> SplitMapsHandoff {
    split_fixture_with_origin(LatLng {
        lat: 35.0,
        lon: 139.0,
    })
}

fn split_fixture_with_origin(origin: LatLng) -> SplitMapsHandoff {
    let merge = edge("m", "mid", 10_000);
    let rest = edge("mid", "b", 10_000);
    build_split_maps_handoff(
        &origin,
        "entry",
        "m",
        &[&merge, &rest],
        "b",
        "exit",
        |node_id| {
            let (lat, lon) = match node_id {
                "entry" => (35.1, 139.1),
                "m" => (35.2, 139.2),
                "mid" => (35.3, 139.3),
                "b" => (35.4, 139.4),
                "exit" => (35.5, 139.5),
                _ => return None,
            };
            Some(LatLng { lat, lon })
        },
    )
    .unwrap()
}

fn radial_candidate_with_handoff(handoff: CandidateV2Handoff) -> RadialReturnCandidate {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../fixtures/candidate-v2/radial-valid.json"
    ))
    .unwrap();
    value["handoff"] = serde_json::to_value(handoff).unwrap();
    serde_json::from_value(value).unwrap()
}

#[test]
fn valid_fixture_binds_route_release_builder_and_leg_hashes() {
    let manifest = parse_device_verification_manifest(VALID_MANIFEST).unwrap();
    assert_eq!(
        manifest.schema_version,
        DEVICE_VERIFICATION_MANIFEST_SCHEMA_VERSION
    );
    assert_eq!(manifest.url_builder_version, URL_BUILDER_VERSION);
    assert_eq!(manifest.legs.len(), 3);
    assert_eq!(manifest.verifications.len(), 4);
    manifest
        .validate_binding(
            "fixture:route-plan:radial:2-outbound",
            "graph-v4-fixture-v1",
            &split_fixture(),
        )
        .unwrap();
}

#[test]
fn json_schema_matches_the_serialized_rust_contract() {
    let schema: Value = serde_json::from_str(MANIFEST_SCHEMA).unwrap();
    let manifest = parse_device_verification_manifest(VALID_MANIFEST).unwrap();
    let value = serde_json::to_value(manifest).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(
        schema["properties"]["schemaVersion"]["const"],
        DEVICE_VERIFICATION_MANIFEST_SCHEMA_VERSION
    );
    assert_eq!(
        schema["properties"]["urlBuilderVersion"]["const"],
        URL_BUILDER_VERSION
    );
    for field in schema["required"].as_array().unwrap() {
        assert!(value.get(field.as_str().unwrap()).is_some(), "{field}");
    }
    assert_eq!(schema["properties"]["legs"]["minItems"], 3);
    assert_eq!(schema["properties"]["legs"]["maxItems"], 3);
    assert_eq!(schema["properties"]["verifications"]["minItems"], 4);
    assert_eq!(schema["properties"]["verifications"]["maxItems"], 4);
    assert_eq!(
        schema["properties"]["verifications"]["allOf"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(schema["$defs"]["leg"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "expectedRoad"));
    assert!(schema["$defs"]["verification"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "clientVersion"));
    assert_eq!(
        schema["$defs"]["verification"]["properties"]["result"]["enum"],
        serde_json::json!(["passed", "failed", "missing", "expired"])
    );
    assert_eq!(
        schema["$defs"]["verification"]["allOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        schema["$defs"]["verification"]["allOf"][0]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        schema["$defs"]["verification"]["allOf"][1]["allOf"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn invalid_fixtures_are_rejected() {
    assert_eq!(
        parse_device_verification_manifest(INVALID_EXPIRY),
        Err(DeviceVerificationManifestError::InvalidVerificationTimestamp)
    );
    assert_eq!(
        parse_device_verification_manifest(INVALID_MISSING_ENVIRONMENT),
        Err(DeviceVerificationManifestError::InvalidVerificationMatrix)
    );
}

#[test]
fn binding_mismatches_are_rejected() {
    let mut manifest = parse_device_verification_manifest(VALID_MANIFEST).unwrap();
    let handoff = split_fixture();
    assert_eq!(
        manifest.validate_binding("other-route-plan", "graph-v4-fixture-v1", &handoff),
        Err(DeviceVerificationManifestError::RoutePlanIdMismatch)
    );
    assert_eq!(
        manifest.validate_binding(
            "fixture:route-plan:radial:2-outbound",
            "other-release",
            &handoff
        ),
        Err(DeviceVerificationManifestError::ReleaseIdMismatch)
    );
    manifest.legs[1].url_sha256 = "0".repeat(64);
    assert_eq!(
        manifest.validate_binding(
            "fixture:route-plan:radial:2-outbound",
            "graph-v4-fixture-v1",
            &handoff
        ),
        Err(DeviceVerificationManifestError::LegUrlHashMismatch)
    );

    let mut invalid_builder = split_fixture();
    invalid_builder.builder_version = "google-maps-split/v2".to_owned();
    let manifest = parse_device_verification_manifest(VALID_MANIFEST).unwrap();
    assert_eq!(
        manifest.validate_binding(
            "fixture:route-plan:radial:2-outbound",
            "graph-v4-fixture-v1",
            &invalid_builder
        ),
        Err(DeviceVerificationManifestError::BuilderVersionMismatch)
    );
}

#[test]
fn passing_unexpired_manifest_opens_exact_bound_radial_handoff() {
    let generated = split_fixture();
    let decision = evaluate_gate(
        Some(VALID_MANIFEST),
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &generated,
        "2026-09-25T00:00:00Z",
    );
    assert!(decision.public_departure_enabled());
    assert_eq!(decision.blocked_by(), None);

    let public = public_handoff(
        &generated,
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &decision,
    )
    .unwrap();
    assert!(public.enabled);
    assert_eq!(public.leg_urls, generated.wire_legs().unwrap());
    assert_eq!(public.disabled_reason, None);
    validate_radial_return_candidate(&radial_candidate_with_handoff(public)).unwrap();
}

#[test]
fn checked_in_pending_manifest_keeps_public_handoff_closed() {
    let manifest = parse_device_verification_manifest(PENDING_MANIFEST).unwrap();
    assert!(manifest
        .verifications
        .iter()
        .all(|record| record.result == DeviceVerificationResult::Missing));

    let result = schema4_search_with_release_gate(
        PENDING_MANIFEST,
        "2026-09-25T00:00:00Z",
        "2026-09-16T00:00:00Z",
    );
    assert_eq!(
        result["candidates"][0]["handoff"],
        serde_json::json!({
            "enabled": false,
            "legUrls": [],
            "disabledReason": "device_verification_pending"
        })
    );
}

#[test]
fn checked_in_hashes_match_the_actual_schema4_generated_handoff() {
    let manifest = parse_device_verification_manifest(PENDING_MANIFEST).unwrap();
    let result = schema4_search_with_release_gate(
        &passing_schema4_manifest(),
        "2026-09-25T00:00:00Z",
        "2026-09-16T00:00:00Z",
    );
    let handoff = &result["candidates"][0]["handoff"];
    assert_eq!(handoff["enabled"], true);
    assert_eq!(handoff["disabledReason"], Value::Null);
    assert_eq!(handoff["legUrls"].as_array().unwrap().len(), 3);

    let expected_hashes = [
        "8790daed7cbd88dcf1edbe954cff52c41ff3904ddd8a0f56c814de40691aa4ae",
        "8119da24106c10c0a9a6356d764daa1019538e21264c20b77d2e8bfad018c621",
        "157b37796922a72a9f633bc9002c85813f9d0f2dd1bada9b4300bfbb0a462bd7",
    ];
    let expected_roles = ["surface_access", "loop_transfer", "surface_return"];
    for (index, leg) in handoff["legUrls"].as_array().unwrap().iter().enumerate() {
        assert_eq!(leg["role"], expected_roles[index]);
        assert_eq!(leg["urlSha256"], expected_hashes[index]);
        assert_eq!(leg["urlSha256"], manifest.legs[index].url_sha256);
    }
}

#[test]
fn release_gate_uses_release_time_and_reaches_passed_failed_and_expired_searches() {
    let passed = passing_schema4_manifest();
    for pricing_at in ["2020-01-01T00:00:00Z", "2030-01-01T00:00:00Z"] {
        let result = schema4_search_with_release_gate(&passed, "2026-09-25T00:00:00Z", pricing_at);
        assert_eq!(result["candidates"][0]["handoff"]["enabled"], true);
    }

    let expired_at_release =
        schema4_search_with_release_gate(&passed, "2026-11-01T00:00:00Z", "2026-09-16T00:00:00Z");
    assert_eq!(
        expired_at_release["candidates"][0]["handoff"],
        serde_json::json!({
            "enabled": false,
            "legUrls": [],
            "disabledReason": "device_verification_pending"
        })
    );

    let mut failed: Value = serde_json::from_str(&passed).unwrap();
    failed["verifications"][0]["result"] = Value::from("failed");
    let failed = schema4_search_with_release_gate(
        &failed.to_string(),
        "2026-09-25T00:00:00Z",
        "2026-09-16T00:00:00Z",
    );
    assert_eq!(failed["candidates"][0]["handoff"]["enabled"], false);

    let mut explicitly_expired: Value = serde_json::from_str(&passed).unwrap();
    explicitly_expired["verifications"][0]["result"] = Value::from("expired");
    let explicitly_expired = schema4_search_with_release_gate(
        &explicitly_expired.to_string(),
        "2026-09-25T00:00:00Z",
        "2026-09-16T00:00:00Z",
    );
    assert_eq!(
        explicitly_expired["candidates"][0]["handoff"]["enabled"],
        false
    );
}

#[test]
fn open_decision_cannot_be_reused_for_another_handoff_or_identity() {
    let generated = split_fixture();
    let decision = evaluate_gate(
        Some(VALID_MANIFEST),
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &generated,
        "2026-09-25T00:00:00Z",
    );
    let other = split_fixture_with_origin(LatLng {
        lat: 36.0,
        lon: 140.0,
    });
    assert_eq!(
        public_handoff(
            &other,
            "fixture:route-plan:radial:2-outbound",
            "graph-v4-fixture-v1",
            &decision,
        )
        .unwrap_err(),
        MapsHandoffError::DeviceVerificationBindingMismatch
    );
    assert_eq!(
        public_handoff(
            &generated,
            "other-route-plan",
            "graph-v4-fixture-v1",
            &decision,
        )
        .unwrap_err(),
        MapsHandoffError::DeviceVerificationBindingMismatch
    );
    assert_eq!(
        public_handoff(
            &generated,
            "fixture:route-plan:radial:2-outbound",
            "other-release",
            &decision,
        )
        .unwrap_err(),
        MapsHandoffError::DeviceVerificationBindingMismatch
    );
}

#[test]
fn missing_failed_expired_and_not_yet_valid_records_close_the_gate() {
    let mut failed: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    failed["verifications"][0]["result"] = Value::from("failed");

    let mut explicitly_expired: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    explicitly_expired["verifications"][0]["result"] = Value::from("expired");

    let mut missing: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    missing["verifications"][0]["result"] = Value::from("missing");
    missing["verifications"][0]["verifiedAt"] = Value::Null;
    missing["verifications"][0]["expiresAt"] = Value::Null;

    let cases = [
        (
            failed.to_string(),
            "2026-09-25T00:00:00Z",
            DeviceVerificationGateBlocker::VerificationFailed,
        ),
        (
            explicitly_expired.to_string(),
            "2026-09-25T00:00:00Z",
            DeviceVerificationGateBlocker::VerificationExpired,
        ),
        (
            missing.to_string(),
            "2026-09-25T00:00:00Z",
            DeviceVerificationGateBlocker::VerificationMissing,
        ),
        (
            VALID_MANIFEST.to_owned(),
            "2026-09-23T23:59:59Z",
            DeviceVerificationGateBlocker::VerificationNotYetValid,
        ),
        (
            VALID_MANIFEST.to_owned(),
            "2026-10-24T00:00:00Z",
            DeviceVerificationGateBlocker::VerificationExpired,
        ),
    ];
    let generated = split_fixture();
    for (manifest, evaluated_at, expected_blocker) in cases {
        let decision = evaluate_gate(
            Some(&manifest),
            "fixture:route-plan:radial:2-outbound",
            "graph-v4-fixture-v1",
            &generated,
            evaluated_at,
        );
        assert!(!decision.public_departure_enabled());
        assert_eq!(decision.blocked_by(), Some(expected_blocker));
        let public = public_handoff(
            &generated,
            "fixture:route-plan:radial:2-outbound",
            "graph-v4-fixture-v1",
            &decision,
        )
        .unwrap();
        assert!(!public.enabled);
        assert!(public.leg_urls.is_empty());
        assert_eq!(
            public.disabled_reason.as_deref(),
            Some("device_verification_pending")
        );
    }

    let missing_manifest = evaluate_gate(
        None,
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &generated,
        "2026-09-25T00:00:00Z",
    );
    assert!(!missing_manifest.public_departure_enabled());
    assert_eq!(
        missing_manifest.blocked_by(),
        Some(DeviceVerificationGateBlocker::ManifestMissing)
    );
    let public = public_handoff(
        &generated,
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &missing_manifest,
    )
    .unwrap();
    assert!(!public.enabled);
    assert!(public.leg_urls.is_empty());
}

#[test]
fn invalid_manifest_binding_or_evaluation_time_fails_closed() {
    let generated = split_fixture();
    let invalid_manifest = evaluate_gate(
        Some("not-json"),
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &generated,
        "2026-09-25T00:00:00Z",
    );
    assert_eq!(
        invalid_manifest.blocked_by(),
        Some(DeviceVerificationGateBlocker::ManifestInvalid)
    );
    let public = public_handoff(
        &generated,
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &invalid_manifest,
    )
    .unwrap();
    assert!(!public.enabled);
    assert!(public.leg_urls.is_empty());

    let binding_mismatch = evaluate_gate(
        Some(VALID_MANIFEST),
        "other-route-plan",
        "graph-v4-fixture-v1",
        &generated,
        "2026-09-25T00:00:00Z",
    );
    assert_eq!(
        binding_mismatch.blocked_by(),
        Some(DeviceVerificationGateBlocker::BindingMismatch)
    );

    let invalid_time = evaluate_gate(
        Some(VALID_MANIFEST),
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &generated,
        "2026-09-25T00:00:00+09:00",
    );
    assert_eq!(
        invalid_time.blocked_by(),
        Some(DeviceVerificationGateBlocker::EvaluationTimeInvalid)
    );

    let missing_time = evaluate_device_verification_gate(
        Some(VALID_MANIFEST),
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &generated,
        None,
    );
    assert_eq!(
        missing_time.blocked_by(),
        Some(DeviceVerificationGateBlocker::EvaluationTimeInvalid)
    );

    let mut invalid_handoff = split_fixture();
    invalid_handoff.legs[0].maps_url.push('0');
    let invalid_handoff = evaluate_gate(
        Some(VALID_MANIFEST),
        "fixture:route-plan:radial:2-outbound",
        "graph-v4-fixture-v1",
        &invalid_handoff,
        "2026-09-25T00:00:00Z",
    );
    assert_eq!(
        invalid_handoff.blocked_by(),
        Some(DeviceVerificationGateBlocker::HandoffInvalid)
    );
}

#[test]
fn closed_radial_gate_does_not_change_c1_handoff_or_warning() {
    let graph = include_str!("../../../fixtures/synthetic-graph.json");
    let request = include_str!("../../../fixtures/synthetic-request.json");
    let before: Value = serde_json::from_str(&search_json(graph, request, "{}").unwrap()).unwrap();
    let generated = split_fixture();
    let decision = evaluate_gate(
        None,
        "fixture:radial",
        "graph-v4-fixture-v1",
        &generated,
        "2026-09-25T00:00:00Z",
    );
    let public = public_handoff(
        &generated,
        "fixture:radial",
        "graph-v4-fixture-v1",
        &decision,
    )
    .unwrap();
    assert!(!public.enabled);
    let after: Value = serde_json::from_str(&search_json(graph, request, "{}").unwrap()).unwrap();
    assert_eq!(before, after);

    let candidate = &after["candidates"][0];
    assert!(candidate["handoff"]["mapsUrl"]
        .as_str()
        .unwrap()
        .starts_with("https://www.google.com/maps/dir/?api=1&origin="));
    assert!(candidate["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning == "HANDOFF_WAYPOINTS_UNVERIFIED"));
}

#[test]
fn value_domains_and_required_fields_are_explicit() {
    let mut unknown_builder: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    unknown_builder["urlBuilderVersion"] = Value::from("google-maps-split/v2");
    assert_eq!(
        parse_device_verification_manifest(&unknown_builder.to_string()),
        Err(DeviceVerificationManifestError::InvalidUrlBuilderVersion)
    );

    let mut missing_field: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    missing_field["legs"][0]
        .as_object_mut()
        .unwrap()
        .remove("expectedRoad");
    assert_eq!(
        parse_device_verification_manifest(&missing_field.to_string()),
        Err(DeviceVerificationManifestError::InvalidJson)
    );

    let mut invalid_hash: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    invalid_hash["legs"][0]["urlSha256"] = Value::from("A".repeat(64));
    assert_eq!(
        parse_device_verification_manifest(&invalid_hash.to_string()),
        Err(DeviceVerificationManifestError::InvalidLegs)
    );

    let mut inconsistent_missing: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    inconsistent_missing["verifications"][0]["result"] = Value::from("missing");
    assert_eq!(
        parse_device_verification_manifest(&inconsistent_missing.to_string()),
        Err(DeviceVerificationManifestError::InvalidVerificationResult)
    );

    for (field, value) in [
        ("osVersion", "unverified"),
        ("clientVersion", "unknown"),
        ("clientName", "safari"),
    ] {
        let mut placeholder: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
        placeholder["verifications"][0][field] = Value::from(value);
        assert_eq!(
            parse_device_verification_manifest(&placeholder.to_string()),
            Err(DeviceVerificationManifestError::InvalidEnvironmentVersion)
        );
    }

    let mut missing: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    missing["verifications"][0]["result"] = Value::from("missing");
    missing["verifications"][0]["verifiedAt"] = Value::Null;
    missing["verifications"][0]["expiresAt"] = Value::Null;
    parse_device_verification_manifest(&missing.to_string()).unwrap();

    let mut expired: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    expired["verifications"][0]["result"] = Value::from("expired");
    parse_device_verification_manifest(&expired.to_string()).unwrap();

    let mut unknown: Value = serde_json::from_str(VALID_MANIFEST).unwrap();
    unknown["verifications"][0]["result"] = Value::from("unknown");
    assert_eq!(
        parse_device_verification_manifest(&unknown.to_string()),
        Err(DeviceVerificationManifestError::InvalidJson)
    );
}
