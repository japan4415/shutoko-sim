use serde_json::Value;
use sha2::{Digest, Sha256};
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

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

fn edge_hash(edge_ids: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(edge_ids).unwrap());
    format!("{:x}", hasher.finalize())
}

fn refresh_edge_hashes(graph: &mut Value) {
    for membership in graph["routeMemberships"].as_array_mut().unwrap() {
        for segment in membership["segments"].as_array_mut().unwrap() {
            let edge_ids = string_list(&segment["orderedEdgeIds"]);
            segment["orderedEdgeIdsSha256"] = Value::from(edge_hash(&edge_ids));
        }
    }
    for pair in graph["billingPairs"].as_array_mut().unwrap() {
        if pair["pairKind"].as_str() == Some("radialReturn") {
            for segment in pair["resolvedRouteSegments"].as_array_mut().unwrap() {
                let edge_ids = string_list(&segment["edgeIds"]);
                segment["edgeIdsSha256"] = Value::from(edge_hash(&edge_ids));
            }
        }
    }
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

    let mut missing_member_order = with_fragment("legacy");
    missing_member_order["routeMemberships"][0]["segments"][0]
        .as_object_mut()
        .unwrap()
        .remove("memberOrderMatchesRelation");
    assert!(prepare_json(&missing_member_order.to_string(), "{}").is_err());

    let mut invalid_member_order = with_fragment("legacy");
    invalid_member_order["routeMemberships"][0]["segments"][0]["memberOrderMatchesRelation"] =
        Value::from(false);
    assert!(prepare_json(&invalid_member_order.to_string(), "{}").is_err());

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

    let mut graph = with_fragment("radial");
    graph["billingPairs"][0]["entryEndpoint"]["directedSegments"][0]
        .as_object_mut()
        .unwrap()
        .remove("osmNodeIds");
    assert!(prepare_json(&graph.to_string(), "{}").is_err());

    let mut schema_2: Value =
        serde_json::from_str(include_str!("../../../fixtures/synthetic-graph.json")).unwrap();
    schema_2["routeMemberships"] = Value::Null;
    assert!(prepare_json(&schema_2.to_string(), "{}").is_err());
}

#[test]
fn schema_4_rejects_non_terminal_merge_and_non_initial_branch_edges() {
    let mut merge = with_fragment("radial");
    let merge_extra = "fixture:edge:merge:extra";
    merge["edges"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": merge_extra,
            "from": "fixture:node:merge",
            "to": "fixture:node:merge",
            "kind": "shutoko",
            "durationSeconds": 30,
            "distanceMeters": 300
        }));
    let mut membership_edges =
        string_list(&merge["routeMemberships"][1]["segments"][1]["orderedEdgeIds"]);
    membership_edges.push(merge_extra.to_owned());
    merge["routeMemberships"][1]["segments"][1]["orderedEdgeIds"] =
        serde_json::to_value(membership_edges).unwrap();
    let mut resolved_edges =
        string_list(&merge["billingPairs"][0]["resolvedRouteSegments"][0]["edgeIds"]);
    resolved_edges.push(merge_extra.to_owned());
    merge["billingPairs"][0]["resolvedRouteSegments"][0]["edgeIds"] =
        serde_json::to_value(resolved_edges).unwrap();
    refresh_edge_hashes(&mut merge);
    assert!(prepare_json(&merge.to_string(), "{}").is_err());

    let mut branch = with_fragment("radial");
    let branch_extra = "fixture:edge:branch:extra";
    branch["edges"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": branch_extra,
            "from": "fixture:node:branch",
            "to": "fixture:node:branch",
            "kind": "shutoko",
            "durationSeconds": 30,
            "distanceMeters": 300
        }));
    let mut membership_edges =
        string_list(&branch["routeMemberships"][3]["segments"][0]["orderedEdgeIds"]);
    membership_edges.insert(0, branch_extra.to_owned());
    branch["routeMemberships"][3]["segments"][0]["orderedEdgeIds"] =
        serde_json::to_value(membership_edges).unwrap();
    let mut resolved_edges =
        string_list(&branch["billingPairs"][0]["resolvedRouteSegments"][2]["edgeIds"]);
    resolved_edges.insert(0, branch_extra.to_owned());
    branch["billingPairs"][0]["resolvedRouteSegments"][2]["edgeIds"] =
        serde_json::to_value(resolved_edges).unwrap();
    refresh_edge_hashes(&mut branch);
    assert!(prepare_json(&branch.to_string(), "{}").is_err());
}

#[test]
fn schema_4_mandatory_lap_is_one_contiguous_relation_mainline_subpath() {
    let mut contiguous = with_fragment("radial");
    let prefix = "fixture:edge:lap:prefix";
    contiguous["edges"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": prefix,
            "from": "fixture:node:merge",
            "to": "fixture:node:merge",
            "kind": "shutoko",
            "durationSeconds": 30,
            "distanceMeters": 300
        }));
    let mut source_edges = vec![prefix.to_owned()];
    source_edges.extend(string_list(
        &contiguous["routeMemberships"][2]["segments"][0]["orderedEdgeIds"],
    ));
    contiguous["routeMemberships"][2]["segments"][0]["orderedEdgeIds"] =
        serde_json::to_value(source_edges).unwrap();
    refresh_edge_hashes(&mut contiguous);
    assert!(prepare_json(&contiguous.to_string(), "{}").is_ok());

    let mut non_contiguous = contiguous;
    let skipped = "fixture:edge:lap:skipped";
    non_contiguous["edges"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": skipped,
            "from": "fixture:node:lap:mid",
            "to": "fixture:node:lap:mid",
            "kind": "shutoko",
            "durationSeconds": 30,
            "distanceMeters": 300
        }));
    let mut source_edges =
        string_list(&non_contiguous["routeMemberships"][2]["segments"][0]["orderedEdgeIds"]);
    source_edges.insert(2, skipped.to_owned());
    non_contiguous["routeMemberships"][2]["segments"][0]["orderedEdgeIds"] =
        serde_json::to_value(source_edges).unwrap();
    refresh_edge_hashes(&mut non_contiguous);
    assert!(prepare_json(&non_contiguous.to_string(), "{}").is_err());

    let mut multiple = with_fragment("radial");
    multiple["routeMemberships"][2]["segments"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "segmentId": "fixture:relation:loop:forward:second",
            "sourceKind": "relationMainline",
            "sourceRelationId": "fixture:relation:loop",
            "sourceSnapshotSha256": "b2a0b24aa896e9d92425ff81539194531e036bda0764aa0792f4cbadf61c044a",
            "bindingEvidenceId": null,
            "orderedEdgeIds": ["fixture:edge:lap:2"],
            "orderedEdgeIdsSha256": ""
        }));
    multiple["billingPairs"][0]["resolvedRouteSegments"][1]["sourceSegmentIds"] =
        serde_json::json!([
            "fixture:relation:loop:forward:main",
            "fixture:relation:loop:forward:second"
        ]);
    refresh_edge_hashes(&mut multiple);
    assert!(prepare_json(&multiple.to_string(), "{}").is_err());
}

#[test]
fn schema_4_legacy_membership_requires_one_ordered_relation_segment() {
    let mut extra = with_fragment("legacy");
    let extra_edges = vec!["fixture:edge:legacy:loop".to_owned()];
    extra["routeMemberships"][0]["segments"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "segmentId": "fixture:relation:C1:inner:extra",
            "sourceKind": "relationMainline",
            "sourceRelationId": "fixture:relation:C1",
            "sourceSnapshotSha256": "b2a0b24aa896e9d92425ff81539194531e036bda0764aa0792f4cbadf61c044a",
            "bindingEvidenceId": null,
            "orderedEdgeIds": extra_edges,
            "orderedEdgeIdsSha256": ""
        }));
    refresh_edge_hashes(&mut extra);
    assert!(prepare_json(&extra.to_string(), "{}").is_err());

    let mut ordered = with_fragment("legacy");
    ordered["routeMemberships"][0]["segments"][0]["orderedEdgeIds"] =
        serde_json::json!(["fixture:edge:legacy:mainline", "fixture:edge:legacy:loop"]);
    refresh_edge_hashes(&mut ordered);
    let ordered_edges =
        string_list(&ordered["routeMemberships"][0]["segments"][0]["orderedEdgeIds"]);
    assert_eq!(ordered_edges.len(), 2);
    assert!(prepare_json(&ordered.to_string(), "{}").is_ok());

    let mut reordered = ordered;
    reordered["routeMemberships"][0]["segments"][0]["orderedEdgeIds"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    refresh_edge_hashes(&mut reordered);
    assert!(prepare_json(&reordered.to_string(), "{}").is_err());
}

#[test]
fn schema_4_rejects_endpoint_route_mismatch_and_unverified_short_connector() {
    for mutate in [
        |graph: &mut Value| graph["ramps"][0]["route"] = Value::from("other-route"),
        |graph: &mut Value| graph["ramps"][0]["direction"] = Value::from("other-direction"),
        |graph: &mut Value| {
            graph["billingPairs"][0]["routePlan"]["anchor"]["excludedShortConnector"]["edgeCount"] =
                Value::from(2)
        },
        |graph: &mut Value| {
            graph["billingPairs"][0]["routePlan"]["anchor"]["excludedShortConnector"]
                ["distanceMeters"] = Value::from(101)
        },
    ] {
        let mut graph = with_fragment("radial");
        mutate(&mut graph);
        assert!(prepare_json(&graph.to_string(), "{}").is_err());
    }
}

#[test]
fn schema_4_rejects_inconsistent_radial_capability_and_statuses() {
    for status in ["unverified", "topology_only"] {
        let mut graph = with_fragment("radial");
        graph["billingPairs"][0]["pairEligibility"]["status"] = Value::from(status);
        graph["billingPairs"][0]["pairEligibility"]["oneSectionAheadVerified"] = Value::from(false);
        assert!(prepare_json(&graph.to_string(), "{}").is_ok(), "{status}");
    }

    for (status, verified) in [
        ("verified_one_section_ahead", false),
        ("unverified", true),
        ("topology_only", true),
    ] {
        let mut graph = with_fragment("radial");
        graph["billingPairs"][0]["pairEligibility"]["status"] = Value::from(status);
        graph["billingPairs"][0]["pairEligibility"]["oneSectionAheadVerified"] =
            Value::from(verified);
        assert!(prepare_json(&graph.to_string(), "{}").is_err(), "{status}");
    }

    for capability in ["structural_no_loop", "unsupported"] {
        let mut graph = with_fragment("radial");
        graph["billingPairs"][0]["routingCapability"] = Value::from(capability);
        assert!(
            prepare_json(&graph.to_string(), "{}").is_err(),
            "{capability}"
        );
    }

    for status in ["unresolved", "topology_only"] {
        let mut graph = with_fragment("radial");
        graph["billingPairs"][0]["loopValidation"]["status"] = Value::from(status);
        assert!(prepare_json(&graph.to_string(), "{}").is_err(), "{status}");
    }

    let mut contradictory = with_fragment("radial");
    contradictory["billingPairs"][0]["routingCapability"] = Value::from("unsupported");
    contradictory["billingPairs"][0]["pairEligibility"]["status"] = Value::from("unverified");
    contradictory["billingPairs"][0]["pairEligibility"]["oneSectionAheadVerified"] =
        Value::from(true);
    contradictory["billingPairs"][0]["loopValidation"]["status"] = Value::from("unresolved");
    assert!(prepare_json(&contradictory.to_string(), "{}").is_err());
}

/// 料金 v3 の legacyRing ペア。製品スコープと証拠は engine / Worker 共通の値を入れる。
fn priced_v3_legacy_graph() -> Value {
    let mut graph = with_fragment("legacy");
    let tariff = &mut graph["billingPairs"][0]["tariff"];
    tariff["assignmentId"] = Value::from("assignment:1:daikancho-kasumigaseki");
    tariff["ruleId"] = Value::from("shutoko-etc-ordinary-2022-04");
    tariff["evidenceId"] = Value::from("evidence:2022-04:p02:1-daikancho-kasumigaseki");
    tariff["distanceEvidenceId"] = Value::from("evidence:2022-04:p02:1-daikancho-kasumigaseki");
    tariff["fareLabel"] = Value::from("普通車ETC基本料金（割引適用前）");
    tariff["vehicleClass"] = Value::from("ordinary");
    tariff["paymentMethod"] = Value::from("etc");
    tariff["fareBasis"] = Value::from("base_toll_excluding_discounts");
    tariff["discountsExcluded"] = Value::from(true);
    tariff["tollSource"] = Value::from("official_distance_rule");
    graph
}

#[test]
fn schema_4_priced_tariff_period_may_come_from_prices_only() {
    // top-level の適用期間が無くても prices[] が読めれば prepare できる。
    // Worker 側の graph 検証も同じ規則に揃える（web/src/worker/pipeline.ts）。
    let mut graph = priced_v3_legacy_graph();
    assert!(graph["billingPairs"][0]["tariff"]["effectiveFrom"].is_null());
    assert!(!graph["billingPairs"][0]["tariff"]["prices"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(prepare_json(&graph.to_string(), "{}").is_ok());

    // どちらにも適用期間が無い形だけは prepare できない。
    graph["billingPairs"][0]["tariff"]["prices"] = Value::from(Vec::<Value>::new());
    assert!(prepare_json(&graph.to_string(), "{}").is_err());
}
