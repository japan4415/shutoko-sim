use serde_json::Value;
use shutoko_routing_core::prepare_json;

fn full_graph() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/graph-v4/graph-radial-fixture.json"
    ))
    .unwrap()
}

fn fragment(name: &str) -> Value {
    match name {
        "legacy" => serde_json::from_str(include_str!(
            "../../../fixtures/graph-v4/wire-legacy-pair.json"
        ))
        .unwrap(),
        "radial" => serde_json::from_str(include_str!(
            "../../../fixtures/graph-v4/wire-radial-pair.json"
        ))
        .unwrap(),
        "unknown-pair" => serde_json::from_str(include_str!(
            "../../../fixtures/graph-v4/invalid-unknown-pair-kind.json"
        ))
        .unwrap(),
        "unknown-anchor" => serde_json::from_str(include_str!(
            "../../../fixtures/graph-v4/invalid-unknown-anchor-kind.json"
        ))
        .unwrap(),
        "unknown-version" => serde_json::from_str(include_str!(
            "../../../fixtures/graph-v4/invalid-unknown-schema-version.json"
        ))
        .unwrap(),
        "partial" => serde_json::from_str(include_str!(
            "../../../fixtures/graph-v4/invalid-partial-pair.json"
        ))
        .unwrap(),
        _ => unreachable!(),
    }
}

fn with_fragment(name: &str) -> Value {
    let mut graph = full_graph();
    graph["billingPairs"] = fragment(name)["billingPairs"].clone();
    graph
}

#[test]
fn schema_2_3_remain_backward_compatible() {
    let mut schema_2: Value =
        serde_json::from_str(include_str!("../../../fixtures/synthetic-graph.json")).unwrap();
    assert!(prepare_json(&schema_2.to_string(), "{}").is_ok());
    schema_2["schemaVersion"] = Value::from(3);
    assert!(prepare_json(&schema_2.to_string(), "{}").is_ok());
}

#[test]
fn schema_4_reads_legacy_and_radial_pair_unions() {
    let prepared = prepare_json(&full_graph().to_string(), "{}").unwrap();
    assert_eq!(prepared.graph().billing_pairs.len(), 1);
    assert_eq!(prepared.radial_billing_pairs().len(), 1);
    serde_json::from_value::<shutoko_routing_core::LegacyRingBillingPair>(
        fragment("legacy")["billingPairs"][0].clone(),
    )
    .unwrap();
    serde_json::from_value::<shutoko_routing_core::RadialReturnBillingPair>(
        fragment("radial")["billingPairs"][0].clone(),
    )
    .unwrap();
    let mut graph = full_graph();
    graph["billingPairs"] = fragment("legacy")["billingPairs"].clone();
    let prepared = prepare_json(&graph.to_string(), "{}").unwrap();
    assert_eq!(prepared.graph().schema_version, 4);
    assert_eq!(prepared.graph().billing_pairs.len(), 1);
    assert!(prepared.radial_billing_pairs().is_empty());
    assert_eq!(prepared.route_memberships().len(), 4);

    graph["billingPairs"] = fragment("radial")["billingPairs"].clone();
    let prepared = prepare_json(&graph.to_string(), "{}").unwrap();
    assert!(prepared.graph().billing_pairs.is_empty());
    assert_eq!(prepared.radial_billing_pairs().len(), 1);
    assert!(matches!(
        prepared.radial_billing_pairs()[0].route_plan.anchor,
        shutoko_routing_core::RouteAnchor::DirectedJunction(_)
    ));
}

#[test]
fn schema_4_rejects_unknown_kinds_versions_and_partial_data() {
    for name in ["unknown-pair", "unknown-anchor", "partial"] {
        assert!(
            prepare_json(&with_fragment(name).to_string(), "{}").is_err(),
            "{name}"
        );
    }
    assert!(prepare_json(&fragment("unknown-version").to_string(), "{}").is_err());

    let mut graph = with_fragment("radial");
    graph["billingPairs"][0]["routePlanVersion"] = Value::from(2);
    assert!(prepare_json(&graph.to_string(), "{}").is_err());

    let mut graph = with_fragment("legacy");
    graph["billingPairs"][0]["anchor"]
        .as_object_mut()
        .unwrap()
        .remove("anchorKind");
    assert!(prepare_json(&graph.to_string(), "{}").is_err());

    let mut graph = with_fragment("legacy");
    graph.as_object_mut().unwrap().remove("routeMemberships");
    assert!(prepare_json(&graph.to_string(), "{}").is_err());

    let mut graph = with_fragment("legacy");
    graph["billingPairs"][0]["anchor"]["routeId"] = Value::from("C2");
    assert!(prepare_json(&graph.to_string(), "{}").is_err());

    let mut graph = with_fragment("radial");
    graph["billingPairs"][0]["resolvedRouteSegments"][0]["sourceSegmentIds"] =
        serde_json::json!(["fixture:relation:r1:inbound:main"]);
    assert!(prepare_json(&graph.to_string(), "{}").is_err());

    let mut schema_2: Value =
        serde_json::from_str(include_str!("../../../fixtures/synthetic-graph.json")).unwrap();
    schema_2["routeMemberships"] = Value::Null;
    assert!(prepare_json(&schema_2.to_string(), "{}").is_err());
}
