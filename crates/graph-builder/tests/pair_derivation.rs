use shutoko_graph_builder::{
    bind_ramps_to_graph, bound_ramp_evidence_from_inventory,
    build_route_membership_indices_with_coverage, build_topology,
    compute_pair_derivation_input_hashes, compute_sha256, derive_pair_candidates_from_source_bytes,
    ordered_edge_ids_sha256, pair_derivation_report_to_deterministic_json,
    validate_billing_pair_adjacency, BillingPairAdjacencyFile, OsmRampBindingsFile,
    OverpassResponse, PairDerivationGateStatus, PairDerivationInputHashes,
    PairDerivationPromotionDecision, RampInventoryFile, RouteMembershipBuildOptions,
    RouteMembershipIndex, RouteRelationCoverage, TopologyConfig,
    BILLING_PAIR_ADJACENCY_SCHEMA_VERSION, PAIR_DERIVATION_REPORT_SCHEMA_VERSION,
    PAIR_DERIVATION_RULE,
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
    route_relation_coverage: Vec<RouteRelationCoverage>,
    adjacency: BillingPairAdjacencyFile,
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
    let built = build_route_membership_indices_with_coverage(
        &osm,
        &graph,
        &RouteMembershipBuildOptions {
            source_snapshot_sha256,
            relation_ids: Some(vec![4256008, 4256339]),
            bound_ramp_evidence: evidence,
        },
    )
    .unwrap();
    let route_memberships = built.route_memberships;
    let route_relation_coverage = built.route_relations.relations;
    let adjacency = serde_json::from_slice(ADJACENCY_BYTES).unwrap();
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
        route_relation_coverage,
        adjacency,
        inventory,
        bindings,
        input_hashes,
    }
}

fn derive_from_source_with_support(
    fixture: &RealDerivationFixture,
    ramp_support_decisions: &[u8],
    ramp_inventory: &[u8],
    osm_ramp_bindings: &[u8],
    billing_pair_adjacency: &[u8],
    od_tariffs: &[u8],
) -> shutoko_graph_builder::PairDerivationReport {
    derive_pair_candidates_from_source_bytes(
        &fixture.graph,
        &fixture.route_memberships,
        &fixture.route_relation_coverage,
        OSM_BYTES,
        ramp_inventory,
        ramp_support_decisions,
        osm_ramp_bindings,
        billing_pair_adjacency,
        od_tariffs,
        SEED_BYTES,
    )
    .unwrap()
}

fn derive_from_source(
    fixture: &RealDerivationFixture,
    ramp_inventory: &[u8],
    osm_ramp_bindings: &[u8],
    billing_pair_adjacency: &[u8],
    od_tariffs: &[u8],
) -> shutoko_graph_builder::PairDerivationReport {
    derive_from_source_with_support(
        fixture,
        SUPPORT_DECISIONS_BYTES,
        ramp_inventory,
        osm_ramp_bindings,
        billing_pair_adjacency,
        od_tariffs,
    )
}

fn derive(fixture: &RealDerivationFixture) -> shutoko_graph_builder::PairDerivationReport {
    derive_from_source(
        fixture,
        INVENTORY_BYTES,
        BINDINGS_BYTES,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
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
    assert_eq!(report.summary.eligible_for_review, 9, "{report:#?}");
    assert_eq!(report.summary.hold, 2);
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
        "bp:c1-outer:ginza-shibakoen",
        "bp:c1-outer:shibakoen-iikura",
        "bp:c1-inner:kasumigaseki-shibakoen",
        "bp:c1-inner:daikancho-kasumigaseki",
        "bp:c1-inner:shibakoen-shiodome",
        "bp:c1-inner:takaracho-kandabashi",
    ] {
        assert_eq!(
            pair(pair_id).gates.official_adjacency.status,
            PairDerivationGateStatus::Passed,
            "{pair_id}"
        );
    }
    for pair_id in [
        "bp:c1-outer:kandabashi-takaracho",
        "bp:c1-outer:kasumigaseki-daikancho",
        "bp:c1-outer:ginza-shibakoen",
        "bp:c1-inner:kasumigaseki-shibakoen",
        "bp:c1-inner:daikancho-kasumigaseki",
        "bp:c1-inner:shibakoen-shiodome",
        "bp:c1-inner:takaracho-kandabashi",
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
        assert!(candidate.rejection_reasons.is_empty(), "{pair_id}");
    }

    let blocked_inner = pair("bp:c1-outer:shibakoen-iikura");
    assert_eq!(
        blocked_inner.promotion_decision,
        PairDerivationPromotionDecision::Hold
    );
    assert_eq!(
        blocked_inner.gates.entry_binding.status,
        PairDerivationGateStatus::Failed
    );
    assert!(blocked_inner
        .rejection_reasons
        .contains(&"ENTRY_BINDING_UNSUPPORTED".to_string()));

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
            PairDerivationPromotionDecision::EligibleForReview,
            "{pair_id}: {:?}",
            radial.rejection_reasons
        );
        assert_eq!(
            radial.gates.first_exit.status,
            PairDerivationGateStatus::Passed
        );
        assert!(radial.rejection_reasons.is_empty(), "{pair_id}");
    }

    let c1_outer = report
        .relation_manifest
        .iter()
        .find(|manifest| manifest.membership_id == "route:C1:outer")
        .unwrap();
    assert_eq!(c1_outer.status, "pass");
    assert_eq!(c1_outer.route_plan_unresolved, 0);
    assert_eq!(c1_outer.relation_ids, vec![4256008]);
    let c1_inner = report
        .relation_manifest
        .iter()
        .find(|manifest| manifest.membership_id == "route:C1:inner")
        .unwrap();
    assert_eq!(c1_inner.status, "fail");
    assert_eq!(c1_inner.route_plan_unresolved, 1);
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
    // The relation manifest covers every route membership, including the ones no
    // candidate pair references, and reports one coverage record per relation.
    let mut manifest_ids = report
        .relation_manifest
        .iter()
        .map(|manifest| manifest.membership_id.as_str())
        .collect::<Vec<_>>();
    let mut membership_ids = fixture
        .route_memberships
        .iter()
        .map(|membership| membership.membership_id.as_str())
        .collect::<Vec<_>>();
    manifest_ids.sort_unstable();
    membership_ids.sort_unstable();
    assert_eq!(manifest_ids, membership_ids, "{report:#?}");
    for manifest in &report.relation_manifest {
        let membership = fixture
            .route_memberships
            .iter()
            .find(|membership| membership.membership_id == manifest.membership_id)
            .unwrap();
        let mut expected_relation_ids = membership
            .segments
            .iter()
            .filter_map(|segment| segment.source_relation_id.as_deref())
            .filter_map(|value| value.parse::<i64>().ok())
            .collect::<Vec<_>>();
        expected_relation_ids.sort_unstable();
        expected_relation_ids.dedup();
        assert_eq!(manifest.relation_ids, expected_relation_ids);
        assert_eq!(
            manifest.route_plan_resolved + manifest.route_plan_unresolved,
            manifest.candidate_pair_ids.len()
        );
        assert_eq!(
            manifest.status,
            if manifest.route_plan_unresolved == 0 {
                "pass"
            } else {
                "fail"
            }
        );
    }
    assert_eq!(report.relation_coverage.len(), 2, "{report:#?}");
    assert_eq!(report.summary.relation_coverage.relation_total, 2);
    assert_eq!(report.summary.relation_coverage.relation_expanded, 2);
    assert_eq!(report.summary.relation_coverage.relation_failed, 0);
    for relation in &report.relation_coverage {
        assert_eq!(
            relation.status,
            shutoko_graph_builder::RouteRelationCoverageStatus::Pass
        );
        assert!(!relation.membership_ids.is_empty());
        assert!(relation.reason.is_none(), "{relation:?}");
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
    assert_eq!(SEED_BYTES, seed_before.as_slice());
}

#[test]
fn all_five_input_hashes_bind_source_bytes_and_memberships() {
    let fixture = real_fixture();
    let mut osm = OSM_BYTES.to_vec();
    osm.push(b' ');
    let osm_hash = compute_pair_derivation_input_hashes(
        &osm,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        &fixture.route_memberships,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
    .unwrap();
    assert_ne!(
        osm_hash.osm_snapshot_sha256,
        fixture.input_hashes.osm_snapshot_sha256
    );

    let mut inventory_value: serde_json::Value = serde_json::from_slice(INVENTORY_BYTES).unwrap();
    inventory_value["description"] = serde_json::Value::String("hash mutation".to_string());
    let inventory = serde_json::to_vec(&inventory_value).unwrap();
    let inventory_hash = compute_pair_derivation_input_hashes(
        OSM_BYTES,
        &inventory,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        &fixture.route_memberships,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
    .unwrap();
    assert_ne!(
        inventory_hash.ramp_ledger_sha256,
        fixture.input_hashes.ramp_ledger_sha256
    );

    let mut support_value: serde_json::Value =
        serde_json::from_slice(SUPPORT_DECISIONS_BYTES).unwrap();
    support_value["description"] = serde_json::Value::String("hash mutation".to_string());
    let support_decisions = serde_json::to_vec(&support_value).unwrap();
    let support_hash = compute_pair_derivation_input_hashes(
        OSM_BYTES,
        INVENTORY_BYTES,
        &support_decisions,
        BINDINGS_BYTES,
        &fixture.route_memberships,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
    .unwrap();
    assert_ne!(
        support_hash.ramp_ledger_sha256,
        fixture.input_hashes.ramp_ledger_sha256
    );

    let mut bindings_value: serde_json::Value = serde_json::from_slice(BINDINGS_BYTES).unwrap();
    bindings_value["sourceDate"] = serde_json::Value::String("2000-01-01".to_string());
    let bindings = serde_json::to_vec(&bindings_value).unwrap();
    let bindings_hash = compute_pair_derivation_input_hashes(
        OSM_BYTES,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        &bindings,
        &fixture.route_memberships,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
    .unwrap();
    assert_ne!(
        bindings_hash.ramp_ledger_sha256,
        fixture.input_hashes.ramp_ledger_sha256
    );

    let mut memberships = fixture.route_memberships.clone();
    memberships[0].segments[0].source_relation_id = Some("999999999".to_string());
    let membership_hash = compute_pair_derivation_input_hashes(
        OSM_BYTES,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        &memberships,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    )
    .unwrap();
    assert_ne!(
        membership_hash.route_membership_index_sha256,
        fixture.input_hashes.route_membership_index_sha256
    );

    let mut adjacency = ADJACENCY_BYTES.to_vec();
    adjacency.push(b' ');
    let adjacency_hash = compute_pair_derivation_input_hashes(
        OSM_BYTES,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        &fixture.route_memberships,
        &adjacency,
        TARIFFS_BYTES,
    )
    .unwrap();
    assert_ne!(
        adjacency_hash.billing_pair_adjacency_sha256,
        fixture.input_hashes.billing_pair_adjacency_sha256
    );

    let mut tariffs = TARIFFS_BYTES.to_vec();
    tariffs.push(b' ');
    let tariff_hash = compute_pair_derivation_input_hashes(
        OSM_BYTES,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        &fixture.route_memberships,
        ADJACENCY_BYTES,
        &tariffs,
    )
    .unwrap();
    assert_ne!(
        tariff_hash.od_tariffs_sha256,
        fixture.input_hashes.od_tariffs_sha256
    );
}

#[test]
fn legacy_seed_identity_rejects_changed_meaning() {
    let fixture = real_fixture();
    let mut adjacency = fixture.adjacency.clone();
    adjacency
        .pairs
        .iter_mut()
        .find(|pair| pair.pair_id == "bp:c1-outer:kandabashi-takaracho")
        .unwrap()
        .entry_name = "変更入口".to_string();
    let adjacency = serde_json::to_vec(&adjacency).unwrap();
    let report = derive_from_source(
        &fixture,
        INVENTORY_BYTES,
        BINDINGS_BYTES,
        &adjacency,
        TARIFFS_BYTES,
    );
    assert_eq!(
        report.input_hashes.billing_pair_adjacency_sha256,
        compute_sha256(&adjacency)
    );
    assert_ne!(
        report.input_hashes.billing_pair_adjacency_sha256,
        fixture.input_hashes.billing_pair_adjacency_sha256
    );
    let candidate = report
        .candidates
        .iter()
        .find(|candidate| candidate.pair_id == "bp:c1-outer:kandabashi-takaracho")
        .unwrap();
    assert_eq!(
        candidate.gates.official_adjacency.status,
        PairDerivationGateStatus::Failed
    );
    assert!(candidate
        .gates
        .official_adjacency
        .reason_codes
        .contains(&"LEGACY_SEED_NAME_MISMATCH".to_string()));
    assert!(candidate
        .gates
        .official_adjacency
        .reason_codes
        .contains(&"LEGACY_SEED_RAMP_IDENTITY_MISMATCH".to_string()));
    assert_eq!(
        candidate.promotion_decision,
        PairDerivationPromotionDecision::Hold
    );
}

#[test]
fn legacy_first_exit_rejects_unprojected_unresolved_candidate() {
    let fixture = real_fixture();
    let target_pair_id = "bp:c1-outer:kandabashi-takaracho";
    let target = fixture
        .adjacency
        .pairs
        .iter()
        .find(|pair| pair.pair_id == target_pair_id)
        .unwrap();
    let membership_id = match target.route_plan.as_ref().unwrap() {
        shutoko_graph_builder::BillingPairAdjacencyRoutePlan::SameNode {
            membership_id,
            first_exit_initial_edge_id,
            ..
        } => {
            let membership = fixture
                .route_memberships
                .iter()
                .find(|membership| membership.membership_id == *membership_id)
                .unwrap();
            let initial_edge_id = first_exit_initial_edge_id.clone();
            let segment = membership
                .segments
                .iter()
                .find(|segment| segment.ordered_edge_ids.contains(&initial_edge_id))
                .unwrap();
            let initial_index = segment
                .ordered_edge_ids
                .iter()
                .position(|edge_id| edge_id == &initial_edge_id)
                .unwrap();
            segment.ordered_edge_ids[initial_index].clone()
        }
        _ => panic!("legacy candidate must have a sameNode plan"),
    };
    let mainline_edge = fixture
        .graph
        .edges
        .iter()
        .find(|edge| edge.id == membership_id)
        .unwrap();
    let edge_ids = vec![mainline_edge.id.clone()];
    let edge_ids_sha256 = ordered_edge_ids_sha256(&edge_ids).unwrap();
    let target_node_id = fixture
        .graph
        .edges
        .iter()
        .find(|edge| edge.from == mainline_edge.to)
        .unwrap()
        .to
        .clone();
    let from_osm_node_id = mainline_edge
        .to
        .strip_prefix("n:")
        .unwrap()
        .parse()
        .unwrap();
    let to_osm_node_id = target_node_id.strip_prefix("n:").unwrap().parse().unwrap();
    let template_inventory = fixture
        .inventory
        .ramps
        .iter()
        .find(|ramp| {
            ramp.route == "C1"
                && ramp.direction == "outer"
                && ramp.kind == shutoko_graph_builder::RampKind::GeneralExit
        })
        .unwrap();
    let mut unprojected_inventory = template_inventory.clone();
    unprojected_inventory.ramp_id = "ramp:test:unprojected-exit".to_string();
    unprojected_inventory.facility_id = "fac:test:unprojected-exit".to_string();
    unprojected_inventory.facility_name = "未投影出口".to_string();
    unprojected_inventory.support_state = Some("unresolved".to_string());
    unprojected_inventory.routing_capability = Some("unsupported".to_string());
    let mut inventory = fixture.inventory.clone();
    inventory.ramps.push(unprojected_inventory);

    let mut candidate = fixture.bindings.binding_candidates.first().unwrap().clone();
    candidate.candidate_id = "test:unprojected-exit".to_string();
    candidate.ramp_id = "ramp:test:unprojected-exit".to_string();
    candidate.status = "unresolved".to_string();
    candidate.direction = "outer".to_string();
    candidate.route_evidence.route_id = "C1".to_string();
    candidate.route_evidence.direction = "outer".to_string();
    candidate.unresolved_reason_codes = vec!["TEST_UNPROJECTED_EXIT".to_string()];
    let mut segment = candidate.directed_segments.first().unwrap().clone();
    segment.segment_id = "ramp:test:unprojected-exit:segment:0".to_string();
    segment.osm_way_ids = vec![9_999_999_991];
    segment.osm_node_ids = vec![from_osm_node_id, to_osm_node_id];
    segment.edge_ids = edge_ids;
    segment.from_node_id = mainline_edge.to.clone();
    segment.to_node_id = target_node_id;
    segment.edge_ids_sha256 = edge_ids_sha256;
    candidate.directed_segments = vec![segment];
    let mut bindings = fixture.bindings.clone();
    bindings.binding_candidates.push(candidate);
    let mut support_decisions: serde_json::Value =
        serde_json::from_slice(SUPPORT_DECISIONS_BYTES).unwrap();
    support_decisions["decisions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "rampId": "ramp:test:unprojected-exit",
            "supportState": "unresolved",
            "bindingCandidateEvidence": {
                "rampIdInverseMap": {
                    "rampId": "ramp:test:unprojected-exit",
                    "candidateId": "test:unprojected-exit"
                },
                "status": "unresolved",
                "mainlineNodeId": mainline_edge.to
            }
        }));
    let report = derive_from_source_with_support(
        &fixture,
        &serde_json::to_vec(&support_decisions).unwrap(),
        &serde_json::to_vec(&inventory).unwrap(),
        &serde_json::to_vec(&bindings).unwrap(),
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
    );
    let target = report
        .candidates
        .iter()
        .find(|candidate| candidate.pair_id == target_pair_id)
        .unwrap();
    assert_eq!(
        target.gates.first_exit.status,
        PairDerivationGateStatus::Failed
    );
    assert!(target
        .gates
        .first_exit
        .reason_codes
        .contains(&"RELATION_FIRST_EXIT_UNRESOLVED".to_string()));
    assert_eq!(
        target.promotion_decision,
        PairDerivationPromotionDecision::Hold
    );
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
