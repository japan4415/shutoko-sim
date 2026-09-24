use serde_json::Value;
use shutoko_routing_core::handoff::build_split_maps_handoff;
use shutoko_routing_core::{
    parse_device_verification_manifest, DeviceVerificationManifestError, Edge, EdgeKind, LatLng,
    SplitMapsHandoff, DEVICE_VERIFICATION_MANIFEST_SCHEMA_VERSION, URL_BUILDER_VERSION,
};

const VALID_MANIFEST: &str = include_str!("../../../fixtures/device-verification/valid.json");
const INVALID_EXPIRY: &str =
    include_str!("../../../fixtures/device-verification/invalid-expiry.json");
const INVALID_MISSING_ENVIRONMENT: &str =
    include_str!("../../../fixtures/device-verification/invalid-missing-environment.json");
const MANIFEST_SCHEMA: &str =
    include_str!("../../../fixtures/device-verification/device-verification-manifest.schema.json");

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
    let merge = edge("m", "mid", 10_000);
    let rest = edge("mid", "b", 10_000);
    build_split_maps_handoff(
        &LatLng {
            lat: 35.0,
            lon: 139.0,
        },
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
        schema["$defs"]["verification"]["allOf"][0]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        2
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
