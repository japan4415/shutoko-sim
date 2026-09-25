use serde_json::json;
use shutoko_routing_core::{
    calculate_tariff_micros_yen, calculate_tariff_yen, prepare_json, read_tariff_v3, search_json,
    TariffResolver, TariffRuleV3, TariffScope,
};
use time::OffsetDateTime;

fn rule(rate: u64, maximum: u64, minimum_distance: u64) -> TariffRuleV3 {
    serde_json::from_value(json!({
        "ruleId": "rule",
        "vehicleClass": "ordinary",
        "paymentMethod": "etc",
        "fareBasis": "base_toll_excluding_discounts",
        "discountsExcluded": [
            "midnight_discount",
            "central_tokyo_inflow_discount",
            "environmental_road_pricing_discount",
            "etc2_discount",
            "frequent_user_discount"
        ],
        "distanceUnitMeters": 100,
        "effectiveFrom": "2022-03-31T15:00:00Z",
        "effectiveTo": null,
        "rateMicrosYenPerUnit": rate,
        "terminalChargeYen": 150,
        "taxBasisPoints": 11000,
        "minimumYen": 300,
        "maximumYen": maximum,
        "minimumDistanceMeters": minimum_distance,
        "rounding": {"mode": "half_up", "multipleYen": 10},
        "sourceRefs": [{"location": "fixture"}]
    }))
    .unwrap()
}

fn catalog_json() -> String {
    include_str!("../../../data/od-tariffs.json").to_owned()
}

#[test]
fn reads_the_reviewed_tariff_v3_catalog() {
    let catalog = read_tariff_v3(&catalog_json()).unwrap();
    assert_eq!(catalog.version, 3);
    assert_eq!(catalog.assignments.len(), 10);
    assert_eq!(catalog.tariff_rules.len(), 2);
}

#[test]
fn half_open_boundary_and_pending_evidence_are_not_replaced_by_a_fallback() {
    let resolver = TariffResolver::new(read_tariff_v3(&catalog_json()).unwrap()).unwrap();
    let before = resolver
        .resolve_od(
            "ramp:c1-outer:kasumigaseki-entry",
            "ramp:c1-outer:daikancho-exit",
            "2026-09-30T14:59:59Z",
            &TariffScope::product(),
        )
        .unwrap();
    assert_eq!(before.amount_yen, Some(570));
    assert_eq!(
        before.rule_id.as_deref(),
        Some("shutoko-etc-ordinary-2022-04")
    );
    assert_eq!(
        before.evidence_id.as_deref(),
        Some("evidence:2025-04:p03:c1-kasumigaseki-daikancho")
    );

    let at_boundary = resolver
        .resolve_od(
            "ramp:c1-outer:kasumigaseki-entry",
            "ramp:c1-outer:daikancho-exit",
            "2026-09-30T15:00:00Z",
            &TariffScope::product(),
        )
        .unwrap();
    assert_eq!(at_boundary.amount_yen, None);
    assert_eq!(at_boundary.billing_distance_meters, None);
    assert_eq!(at_boundary.evidence_id, None);
}

#[test]
fn integer_microyen_calculation_has_revision_and_quantum_boundaries() {
    let old = rule(2_952_000, 1_950, 4_300);
    let new = rule(3_247_200, 2_130, 3_900);
    assert_eq!(
        calculate_tariff_micros_yen(14_200, &old).unwrap(),
        630_000_000
    );
    assert_eq!(
        calculate_tariff_micros_yen(14_200, &new).unwrap(),
        670_000_000
    );
    assert_eq!(calculate_tariff_yen(3_900, &new).unwrap(), 300);
    assert_eq!(calculate_tariff_yen(4_000, &new).unwrap(), 310);
    assert!(calculate_tariff_yen(4_050, &new).is_err());
}

#[test]
fn graph_reader_accepts_a_v3_tariff_object_and_keeps_legacy_graph_reader_compatible() {
    let mut graph: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/graph-v4/graph-radial-fixture.json"
    ))
    .unwrap();
    let catalog: serde_json::Value = serde_json::from_str(&catalog_json()).unwrap();
    graph["odTariffs"] = catalog;
    prepare_json(&graph.to_string(), "{}").unwrap();
}

#[test]
fn v3_candidates_propagate_official_provenance_and_reject_scope_mismatch() {
    let catalog: serde_json::Value = serde_json::from_str(&catalog_json()).unwrap();
    let graph = json!({
        "schemaVersion": 2,
        "releaseId": "tariff-v3-test",
        "vehicleProfile": "passenger-car-etc",
        "nodes": [
            {"id": "i", "lat": 35.0, "lon": 139.0},
            {"id": "a", "lat": 35.001, "lon": 139.001},
            {"id": "o", "lat": 35.002, "lon": 139.002}
        ],
        "edges": [
            {"id": "entry", "from": "i", "to": "a", "kind": "entry", "durationSeconds": 30, "distanceMeters": 200},
            {"id": "loop", "from": "a", "to": "a", "kind": "shutoko", "durationSeconds": 600, "distanceMeters": 10000},
            {"id": "exit", "from": "a", "to": "o", "kind": "exit", "durationSeconds": 30, "distanceMeters": 200}
        ],
        "ramps": [
            {"id": "ramp:c1-outer:kasumigaseki-entry", "facilityId": "f1", "name": "entry", "route": "C1", "direction": "outer", "kind": "general_entry", "edgeId": "entry", "nodeId": "i", "mainlineNodeId": "a"},
            {"id": "ramp:c1-outer:daikancho-exit", "facilityId": "f2", "name": "exit", "route": "C1", "direction": "outer", "kind": "general_exit", "edgeId": "exit", "nodeId": "o", "mainlineNodeId": "a"}
        ],
        "billingPairs": [{
            "id": "bp:c1-outer:kasumigaseki-daikancho",
            "entryId": "entry",
            "exitId": "exit",
            "anchorNodeId": "a",
            "entryToAnchorEdgeIds": ["entry"],
            "anchorToExitEdgeIds": ["exit"],
            "status": "verified",
            "vehicleProfile": "passenger-car-etc",
            "entryRampId": "ramp:c1-outer:kasumigaseki-entry",
            "exitRampId": "ramp:c1-outer:daikancho-exit",
            "prices": [{"amountYen": 570, "effectiveFrom": "2022-03-31T15:00:00Z", "effectiveTo": "2026-09-30T15:00:00Z"}]
        }],
        "odTariffs": catalog
    });
    let request = json!({
        "requestId": "v3-search",
        "releaseId": "tariff-v3-test",
        "originNodeId": "i",
        "minMinutes": 1,
        "maxMinutes": 60,
        "vehicleProfile": "passenger-car-etc",
        "vehicleClass": "ordinary",
        "paymentMethod": "etc",
        "fareBasis": "base_toll_excluding_discounts",
        "discountsExcluded": true,
        "pricingAt": "2026-09-30T14:59:59Z"
    });
    let result: serde_json::Value =
        serde_json::from_str(&search_json(&graph.to_string(), &request.to_string(), "{}").unwrap())
            .unwrap();
    let toll = &result["candidates"][0]["toll"];
    assert_eq!(toll["amountYen"], 570);
    assert_eq!(toll["ruleId"], "shutoko-etc-ordinary-2022-04");
    assert_eq!(
        toll["evidenceId"],
        "evidence:2025-04:p03:c1-kasumigaseki-daikancho"
    );
    assert_eq!(toll["fareLabel"], "普通車ETC基本料金（割引適用前）");
    assert_eq!(toll["tollSource"], "official_distance_rule");
    assert_eq!(result["candidates"][0]["tariffStatus"], "priced");

    let mut mismatch = request;
    mismatch["paymentMethod"] = json!("cash");
    assert!(search_json(&graph.to_string(), &mismatch.to_string(), "{}").is_err());
}

#[test]
fn assignment_id_lookup_rejects_every_mismatched_identity_selector() {
    let resolver = TariffResolver::new(read_tariff_v3(&catalog_json()).unwrap()).unwrap();
    let assignment_id = "assignment:c1-outer:kandabashi-takaracho";
    let scope = TariffScope::product();
    let pricing_at = || {
        OffsetDateTime::parse(
            "2026-09-30T14:59:59Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
    };
    let cases = [
        (
            "ramp:c1-outer:ginza-entry",
            "ramp:c1-outer:takaracho-exit",
            "bp:c1-outer:kandabashi-takaracho",
        ),
        (
            "ramp:c1-outer:kandabashi-entry",
            "ramp:c1-outer:shibakoen-exit",
            "bp:c1-outer:kandabashi-takaracho",
        ),
        (
            "ramp:c1-outer:kandabashi-entry",
            "ramp:c1-outer:takaracho-exit",
            "bp:c1-outer:ginza-shibakoen",
        ),
    ];
    for (entry, exit, pair) in cases {
        let error = resolver
            .resolve_at(
                Some(assignment_id),
                Some(entry),
                Some(exit),
                Some(pair),
                pricing_at(),
                &scope,
            )
            .unwrap_err();
        assert_eq!(error.code, "TARIFF_ASSIGNMENT_MISMATCH");
    }
}

#[test]
fn catalog_rejects_same_fare_evidence_from_another_od() {
    let mut catalog: serde_json::Value = serde_json::from_str(&catalog_json()).unwrap();
    let target = catalog["assignments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|assignment| assignment["assignmentId"] == "assignment:c1-outer:ginza-shibakoen")
        .unwrap();
    target["billingDistanceMeters"] = json!(1700);
    target["prices"][0]["evidenceId"] = json!("evidence:2025-04:p03:c1-kandabashi-takaracho");
    target["prices"][0]["distanceEvidenceId"] =
        json!("evidence:2025-04:p03:c1-kandabashi-takaracho");
    target["prices"][0]["observedDistanceMeters"] = json!(1700);
    let target_base = catalog["distanceEvidence"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|evidence| evidence["evidenceId"] == "evidence:2025-04:p03:c1-ginza-shibakoen")
        .unwrap();
    target_base["distanceMeters"] = json!(1700);
    target_base["distanceLabel"] = json!("1.7km");

    let error = read_tariff_v3(&catalog.to_string()).unwrap_err();
    assert_eq!(error.code, "TARIFF_PRICE_EVIDENCE_MISMATCH");
}

#[test]
fn catalog_rejects_pending_evidence_from_another_od() {
    let mut catalog: serde_json::Value = serde_json::from_str(&catalog_json()).unwrap();
    let target = catalog["assignments"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|assignment| assignment["assignmentId"] == "assignment:c1-outer:ginza-shibakoen")
        .unwrap();
    target["prices"][1]["evidenceId"] = json!("pending:2026-10:od:c1-outer:kandabashi-takaracho");
    target["prices"][1]["distanceEvidenceId"] =
        json!("pending:2026-10:od:c1-outer:kandabashi-takaracho");

    let error = read_tariff_v3(&catalog.to_string()).unwrap_err();
    assert_eq!(error.code, "TARIFF_PRICE_EVIDENCE_MISMATCH");
}

#[test]
fn catalog_rejects_evidence_from_a_document_outside_the_price_rule() {
    let mut catalog: serde_json::Value = serde_json::from_str(&catalog_json()).unwrap();
    let target_evidence = catalog["distanceEvidence"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|evidence| evidence["evidenceId"] == "evidence:2025-04:p03:c1-ginza-shibakoen")
        .unwrap();
    target_evidence["documentId"] = json!("shutoko-2026-10-revision-material");
    target_evidence["edition"] = json!("2026-10");
    target_evidence["documentSha256"] =
        json!("f80126994b3deee36e198f947f3f4f4c3219dd16473bbd9bc9dd296115345702");

    let error = read_tariff_v3(&catalog.to_string()).unwrap_err();
    assert_eq!(error.code, "TARIFF_PRICE_EVIDENCE_MISMATCH");
}

#[test]
fn graph_prepare_rejects_a_pair_with_a_foreign_embedded_assignment() {
    let mut graph: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/graph-v4/graph-radial-fixture.json"
    ))
    .unwrap();
    graph["odTariffs"] = serde_json::from_str(&catalog_json()).unwrap();
    graph["billingPairs"][0]["tariff"] = json!({
        "status": "priced",
        "amountYen": 300,
        "billingDistanceMeters": 1700,
        "effectiveFrom": "2022-03-31T15:00:00Z",
        "effectiveTo": "2026-09-30T15:00:00Z",
        "prices": [],
        "assignmentId": "assignment:c1-outer:kandabashi-takaracho",
        "ruleId": "shutoko-etc-ordinary-2022-04",
        "evidenceId": "evidence:2025-04:p03:c1-kandabashi-takaracho",
        "distanceEvidenceId": "evidence:2025-04:p03:c1-kandabashi-takaracho",
        "fareLabel": "普通車ETC基本料金（割引適用前）",
        "vehicleClass": "ordinary",
        "paymentMethod": "etc",
        "fareBasis": "base_toll_excluding_discounts",
        "discountsExcluded": true,
        "tollSource": "official_distance_rule"
    });

    let error = match prepare_json(&graph.to_string(), "{}") {
        Ok(_) => panic!("foreign embedded assignment was accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("TARIFF_ASSIGNMENT_MISMATCH"));
}
