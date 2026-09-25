use shutoko_graph_builder::{
    bind_ramps_to_graph, bound_ramp_evidence_from_inventory, build_route_membership_indices,
    build_topology, compute_pair_derivation_input_hashes, compute_sha256, derive_pair_candidates,
    derive_pair_candidates_from_source_bytes, pair_derivation_report_to_deterministic_json,
    validate_billing_pair_adjacency, BillingPairAdjacencyFile, OdTariffsFile, OsmRampBindingsFile,
    OverpassResponse, PairDerivationGateStatus, PairDerivationInputHashes,
    PairDerivationPromotionDecision, RampInventoryFile, RouteMembershipBuildOptions,
    RouteMembershipIndex, TopologyConfig, BILLING_PAIR_ADJACENCY_SCHEMA_VERSION,
    PAIR_DERIVATION_REPORT_SCHEMA_VERSION, PAIR_DERIVATION_RULE,
};

const OSM_BYTES: &[u8] = include_bytes!("../../../fixtures/osm/shutoko-all.json");
const INVENTORY_BYTES: &[u8] = include_bytes!("../../../data/ramp-inventory.json");
const SUPPORT_DECISIONS_BYTES: &[u8] = include_bytes!("../../../data/ramp-support-decisions.json");
const BINDINGS_BYTES: &[u8] = include_bytes!("../../../data/osm-ramp-bindings.json");
const ADJACENCY_BYTES: &[u8] = include_bytes!("../../../data/billing-pair-adjacency.json");
const TARIFFS_BYTES: &[u8] = include_bytes!("../../../data/od-tariffs.json");
const SEED_BYTES: &[u8] = include_bytes!("../../../data/billing-pairs-seed.json");

struct RealDerivationFixture {
    graph: shutoko_graph_builder::Graph,
    route_memberships: Vec<RouteMembershipIndex>,
    adjacency: BillingPairAdjacencyFile,
    tariffs: OdTariffsFile,
    inventory: RampInventoryFile,
    bindings: OsmRampBindingsFile,
    input_hashes: PairDerivationInputHashes,
}

fn real_fixture() -> RealDerivationFixture {
    let osm: OverpassResponse = serde_json::from_slice(OSM_BYTES).unwrap();
    let (mut graph, _snap) = build_topology(
        &osm,
        &TopologyConfig {
            release_id: "pair-derivation-test".to_string(),
            vehicle_profile: "passenger-car-etc".to_string(),
        },
    )
    .unwrap();
    let inventory = serde_json::from_slice(INVENTORY_BYTES).unwrap();
    let bindings = serde_json::from_slice(BINDINGS_BYTES).unwrap();
    let (ramps, _artifact, _notes) = bind_ramps_to_graph(&mut graph, &inventory, &bindings);
    graph.ramps = ramps;
    let source_snapshot_sha256 = compute_sha256(OSM_BYTES);
    let evidence = bound_ramp_evidence_from_inventory(&graph, &inventory, &bindings).unwrap();
    let route_memberships = build_route_membership_indices(
        &osm,
        &graph,
        &RouteMembershipBuildOptions {
            source_snapshot_sha256,
            relation_ids: Some(vec![4256008, 4256339]),
            bound_ramp_evidence: evidence,
        },
    )
    .unwrap();
    let adjacency = serde_json::from_slice(ADJACENCY_BYTES).unwrap();
    let tariffs = serde_json::from_slice(TARIFFS_BYTES).unwrap();
    let input_hashes = compute_pair_derivation_input_hashes(
        OSM_BYTES,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        &route_memberships,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
    .unwrap();
    RealDerivationFixture {
        graph,
        route_memberships,
        adjacency,
        tariffs,
        inventory,
        bindings,
        input_hashes,
    }
}

fn derive(fixture: &RealDerivationFixture) -> shutoko_graph_builder::PairDerivationReport {
    derive_pair_candidates(
        &fixture.graph,
        &fixture.route_memberships,
        &fixture.adjacency,
        &fixture.tariffs,
        &fixture.inventory,
        &fixture.bindings,
        fixture.input_hashes.clone(),
    )
    .unwrap()
}

#[test]
fn derives_all_official_candidates_with_independent_gates() {
    let fixture = real_fixture();
    let report = derive(&fixture);
    assert_eq!(report.schema_version, PAIR_DERIVATION_REPORT_SCHEMA_VERSION);
    assert_eq!(report.rule, PAIR_DERIVATION_RULE);
    assert!(!report.automatic_seed_write);
    assert_eq!(report.candidates.len(), 11);
    assert_eq!(report.summary.candidate_total, 11);
    assert_eq!(report.summary.eligible_for_review, 2);
    assert_eq!(report.summary.hold, 9);
    assert_eq!(
        report
            .candidates
            .iter()
            .filter(|candidate| candidate.promotion_decision
                == PairDerivationPromotionDecision::EligibleForReview)
            .count(),
        report.summary.eligible_for_review
    );
    assert_eq!(
        report
            .candidates
            .iter()
            .filter(|candidate| !candidate.automatic_seed_write)
            .count(),
        11
    );
    for hash in [
        &report.input_hashes.osm_snapshot_sha256,
        &report.input_hashes.ramp_ledger_sha256,
        &report.input_hashes.route_membership_index_sha256,
        &report.input_hashes.billing_pair_adjacency_sha256,
        &report.input_hashes.od_tariffs_sha256,
    ] {
        assert_eq!(hash.len(), 64);
        assert!(hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }
    for candidate in &report.candidates {
        assert_eq!(candidate.route_plan.resolved_roles.len(), 4);
        assert_eq!(candidate.tariff.prices.len(), 2);
        assert_eq!(candidate.tariff.status, "priced");
        assert!(candidate.tariff.assignment_id.is_some());
    }

    let pair = |pair_id: &str| {
        report
            .candidates
            .iter()
            .find(|candidate| candidate.pair_id == pair_id)
            .unwrap()
    };
    for pair_id in [
        "bp:c1-outer:kandabashi-takaracho",
        "bp:c1-outer:kasumigaseki-daikancho",
    ] {
        let candidate = pair(pair_id);
        assert_eq!(
            candidate.promotion_decision,
            PairDerivationPromotionDecision::EligibleForReview,
            "{pair_id}: {:?}",
            candidate.rejection_reasons
        );
        assert_eq!(
            candidate.gates.first_exit.status,
            PairDerivationGateStatus::Passed
        );
    }

    let blocked_inner = pair("bp:c1-outer:shibakoen-iikura");
    assert_eq!(
        blocked_inner.gates.entry_binding.status,
        PairDerivationGateStatus::Failed
    );
    assert!(blocked_inner
        .rejection_reasons
        .contains(&"ENTRY_BINDING_UNSUPPORTED".to_string()));
    let blocked_inner_shibakoen = pair("bp:c1-inner:shibakoen-shiodome");
    assert_eq!(
        blocked_inner_shibakoen.gates.entry_binding.status,
        PairDerivationGateStatus::Failed
    );
    assert!(blocked_inner_shibakoen
        .rejection_reasons
        .contains(&"ENTRY_BINDING_UNSUPPORTED".to_string()));
    for pair_id in [
        "bp:c1-outer:ginza-shibakoen",
        "bp:c1-inner:kasumigaseki-shibakoen",
    ] {
        let candidate = pair(pair_id);
        assert_eq!(
            candidate.promotion_decision,
            PairDerivationPromotionDecision::Hold
        );
        assert!(candidate
            .rejection_reasons
            .contains(&"LEGACY_ENDPOINT_EDGE_MISMATCH".to_string()));
        assert!(candidate
            .rejection_reasons
            .contains(&"RELATION_EXIT_APPROACH_BOUNDARY_MISMATCH".to_string()));
    }
    for pair_id in [
        "bp:c1-inner:daikancho-kasumigaseki",
        "bp:c1-inner:takaracho-kandabashi",
    ] {
        let candidate = pair(pair_id);
        assert_eq!(
            candidate.promotion_decision,
            PairDerivationPromotionDecision::Hold
        );
        assert!(candidate
            .rejection_reasons
            .contains(&"EXIT_BINDING_UNSUPPORTED".to_string()));
        assert!(candidate
            .rejection_reasons
            .contains(&"RELATION_EXIT_RAMP_NOT_FOUND".to_string()));
    }
    let blocked_shintomicho = pair("bp:c1-inner:ginza-shintomicho");
    assert_eq!(
        blocked_shintomicho.promotion_decision,
        PairDerivationPromotionDecision::Hold
    );
    assert_eq!(
        blocked_shintomicho.gates.official_adjacency.status,
        PairDerivationGateStatus::Unresolved
    );
    assert!(blocked_shintomicho
        .rejection_reasons
        .contains(&"ROUTE_PLAN_EVIDENCE_UNRESOLVED".to_string()));
    for pair_id in [
        "bp:2-inbound:meguro:c1-inner:tengenji",
        "bp:2-inbound:meguro:c1-outer:tengenji",
    ] {
        let radial = pair(pair_id);
        assert_eq!(
            radial.promotion_decision,
            PairDerivationPromotionDecision::Hold
        );
        assert_eq!(
            radial.gates.first_exit.status,
            PairDerivationGateStatus::Unresolved
        );
        assert!(radial
            .rejection_reasons
            .contains(&"EXIT_BINDING_UNRESOLVED".to_string()));
        assert!(!radial
            .rejection_reasons
            .contains(&"ROUTE_MEMBERSHIP_EXIT_NOT_FOUND".to_string()));
    }

    let c1_outer = report
        .relation_manifest
        .iter()
        .find(|manifest| manifest.membership_id == "route:C1:outer")
        .unwrap();
    assert_eq!(c1_outer.status, "fail");
    assert!(c1_outer.route_plan_unresolved > 0);
    assert_eq!(c1_outer.relation_ids, vec![4256008]);
    let c1_inner = report
        .relation_manifest
        .iter()
        .find(|manifest| manifest.membership_id == "route:C1:inner")
        .unwrap();
    assert_eq!(c1_inner.status, "fail");
    assert!(c1_inner
        .candidate_pair_ids
        .contains(&"bp:c1-inner:ginza-shintomicho".to_string()));
    for membership_id in ["route:2:inbound", "route:2:outbound"] {
        let radial = report
            .relation_manifest
            .iter()
            .find(|manifest| manifest.membership_id == membership_id)
            .unwrap();
        assert_eq!(radial.status, "pass");
        assert_eq!(radial.relation_ids, vec![4256339]);
    }
}

#[test]
fn report_is_byte_identical_across_three_generations_and_never_writes_seed() {
    let fixture = real_fixture();
    let seed_before = SEED_BYTES.to_vec();
    let first = pair_derivation_report_to_deterministic_json(&derive(&fixture)).unwrap();
    let second = pair_derivation_report_to_deterministic_json(&derive(&fixture)).unwrap();
    let third = pair_derivation_report_to_deterministic_json(&derive(&fixture)).unwrap();
    assert_eq!(first, second);
    assert_eq!(second, third);
    assert!(first.ends_with('\n'));
    let source_report = derive_pair_candidates_from_source_bytes(
        &fixture.graph,
        &fixture.route_memberships,
        OSM_BYTES,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
    .unwrap();
    assert_eq!(
        first,
        pair_derivation_report_to_deterministic_json(&source_report).unwrap()
    );
    let mut mismatched_hashes = fixture.input_hashes.clone();
    mismatched_hashes.route_membership_index_sha256 = "0".repeat(64);
    let error = derive_pair_candidates(
        &fixture.graph,
        &fixture.route_memberships,
        &fixture.adjacency,
        &fixture.tariffs,
        &fixture.inventory,
        &fixture.bindings,
        mismatched_hashes,
    )
    .unwrap_err();
    assert_eq!(error.code, "PAIR_DERIVATION_INPUT_HASH_MISMATCH");
    assert_eq!(SEED_BYTES, seed_before.as_slice());
}

#[test]
fn adjacency_contract_rejects_missing_or_mismatched_route_evidence() {
    let fixture = real_fixture();
    assert_eq!(
        fixture.adjacency.schema_version,
        BILLING_PAIR_ADJACENCY_SCHEMA_VERSION
    );
    validate_billing_pair_adjacency(&fixture.adjacency).unwrap();

    let mut missing_plan = fixture.adjacency.clone();
    missing_plan.pairs[0].route_plan = None;
    let error = validate_billing_pair_adjacency(&missing_plan)
        .unwrap_err()
        .join(" ");
    assert!(error.contains("has no route plan"));

    let mut wrong_membership = fixture.adjacency.clone();
    wrong_membership.pairs[0].route_plan = Some(
        shutoko_graph_builder::BillingPairAdjacencyRoutePlan::SameNode {
            membership_id: "route:C1:inner".to_string(),
            anchor_node_id: "n:499831338".to_string(),
            first_exit_initial_edge_id: "e:w24039781:27:f".to_string(),
            exit_approach_edge_ids: vec!["e:w297864314:0:f".to_string()],
        },
    );
    let error = validate_billing_pair_adjacency(&wrong_membership)
        .unwrap_err()
        .join(" ");
    assert!(error.contains("incomplete sameNode plan"));

    let mut radial_mismatch = fixture.adjacency.clone();
    let radial = radial_mismatch
        .pairs
        .iter_mut()
        .find(|pair| {
            pair.pair_kind == shutoko_graph_builder::BillingPairAdjacencyKind::RadialReturn
        })
        .unwrap();
    if let Some(shutoko_graph_builder::BillingPairAdjacencyRoutePlan::DirectedJunction {
        anchor,
        ..
    }) = radial.route_plan.as_mut()
    {
        anchor.direction = "outer".to_string();
    }
    let error = validate_billing_pair_adjacency(&radial_mismatch)
        .unwrap_err()
        .join(" ");
    assert!(error.contains("invalid directedJunction plan"));
}
