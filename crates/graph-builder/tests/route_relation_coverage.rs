//! Full route relation coverage contract for the schema 4 release path.
//!
//! The pair derivation input ledger hashes a `RouteMembershipIndex`, so the
//! full-coverage release has to expand every route relation in the OSM snapshot
//! and account for the ones it cannot expand.

use shutoko_graph_builder::{
    all_route_relation_ids, bind_ramps_to_graph, bound_ramp_evidence_from_inventory,
    build_route_membership_indices_with_coverage, build_topology, compute_sha256,
    default_route_relation_ids, derive_pair_candidates_from_source_bytes, route_memberships_sha256,
    validate_requested_route_relation_ids, Graph, OsmRampBindingsFile, OverpassResponse,
    RampInventoryFile, RouteMembershipBuildOptions, RouteMembershipCoverage,
    RouteRelationCoverageStatus, PAIR_DERIVATION_REPORT_SCHEMA_VERSION, PAIR_DERIVATION_RULE,
    ROUTE_RELATION_NO_MEMBERSHIP_CODE,
};
use std::sync::OnceLock;

const OSM_BYTES: &[u8] = include_bytes!("../../../fixtures/osm/shutoko-all.json");
const INVENTORY_BYTES: &[u8] = include_bytes!("../../../data/ramp-inventory.json");
const BINDINGS_BYTES: &[u8] = include_bytes!("../../../data/osm-ramp-bindings.json");
const SUPPORT_DECISIONS_BYTES: &[u8] = include_bytes!("../../../data/ramp-support-decisions.json");
const ADJACENCY_BYTES: &[u8] = include_bytes!("../../../data/billing-pair-adjacency.json");
const TARIFFS_BYTES: &[u8] = include_bytes!("../../../data/od-tariffs.json");
const SEED_BYTES: &[u8] = include_bytes!("../../../data/billing-pairs-seed.json");

const C1_RELATION_ID: i64 = 4256008;
const ROUTE_2_RELATION_ID: i64 = 4256339;

fn real_osm() -> &'static OverpassResponse {
    static OSM: OnceLock<OverpassResponse> = OnceLock::new();
    OSM.get_or_init(|| serde_json::from_slice(OSM_BYTES).unwrap())
}

fn real_bound_graph() -> &'static Graph {
    static GRAPH: OnceLock<Graph> = OnceLock::new();
    GRAPH.get_or_init(|| {
        let (mut graph, _snap) = build_topology(
            real_osm(),
            &shutoko_graph_builder::TopologyConfig {
                release_id: "route-relation-coverage-test".to_string(),
                vehicle_profile: "passenger-car-etc".to_string(),
            },
        )
        .unwrap();
        let inventory: RampInventoryFile = serde_json::from_slice(INVENTORY_BYTES).unwrap();
        let bindings: OsmRampBindingsFile = serde_json::from_slice(BINDINGS_BYTES).unwrap();
        let (ramps, _artifact, _notes) = bind_ramps_to_graph(&mut graph, &inventory, &bindings);
        graph.ramps = ramps;
        graph
    })
}

fn full_coverage() -> &'static RouteMembershipCoverage {
    static COVERAGE: OnceLock<RouteMembershipCoverage> = OnceLock::new();
    COVERAGE.get_or_init(|| {
        let graph = real_bound_graph();
        let inventory: RampInventoryFile = serde_json::from_slice(INVENTORY_BYTES).unwrap();
        let bindings: OsmRampBindingsFile = serde_json::from_slice(BINDINGS_BYTES).unwrap();
        let evidence = bound_ramp_evidence_from_inventory(graph, &inventory, &bindings).unwrap();
        build_route_membership_indices_with_coverage(
            real_osm(),
            graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: compute_sha256(OSM_BYTES),
                relation_ids: None,
                bound_ramp_evidence: evidence,
            },
        )
        .unwrap()
    })
}

#[test]
fn every_route_relation_in_the_snapshot_is_selected_and_validated() {
    let ids = all_route_relation_ids(real_osm());
    assert_eq!(
        ids.len(),
        26,
        "the OSM snapshot route relation count changed"
    );
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
    assert_eq!(
        validate_requested_route_relation_ids(real_osm(), &ids).unwrap(),
        ids
    );
}

#[test]
fn legacy_default_selection_stays_on_c1_and_route_2() {
    assert_eq!(
        default_route_relation_ids(real_osm()),
        Some(vec![C1_RELATION_ID, ROUTE_2_RELATION_ID])
    );
}

#[test]
#[ignore = "expands every route relation of the real OSM snapshot; run with --ignored"]
fn full_coverage_accounts_for_every_route_relation() {
    let coverage = full_coverage();
    let relations = &coverage.route_relations.relations;
    assert_eq!(relations.len(), 26);
    assert_eq!(
        relations
            .iter()
            .map(|relation| relation.relation_id)
            .collect::<Vec<_>>(),
        all_route_relation_ids(real_osm())
    );
    for relation in relations {
        match relation.status {
            RouteRelationCoverageStatus::Pass => {
                assert!(
                    relation.membership_ids.iter().all(|id| coverage
                        .route_memberships
                        .iter()
                        .any(|membership| &membership.membership_id == id)),
                    "relation {} references an unknown membership",
                    relation.relation_id
                );
                if relation.membership_ids.is_empty() {
                    assert_eq!(
                        relation.reason_code.as_deref(),
                        Some(ROUTE_RELATION_NO_MEMBERSHIP_CODE),
                        "{relation:?}"
                    );
                }
            }
            RouteRelationCoverageStatus::Fail => {
                assert!(relation.membership_ids.is_empty());
                assert!(relation.reason_code.is_some(), "{relation:?}");
                assert!(relation.reason.is_some(), "{relation:?}");
            }
        }
    }
    // The legacy C1 and route 2 results must survive the full sweep.
    for membership_id in [
        "route:C1:inner",
        "route:C1:outer",
        "route:2:inbound",
        "route:2:outbound",
    ] {
        assert!(
            coverage
                .route_memberships
                .iter()
                .any(|membership| membership.membership_id == membership_id),
            "{membership_id} is missing from the full coverage index"
        );
    }
    for relation_id in [C1_RELATION_ID, ROUTE_2_RELATION_ID] {
        let relation = relations
            .iter()
            .find(|relation| relation.relation_id == relation_id)
            .unwrap();
        assert_eq!(relation.status, RouteRelationCoverageStatus::Pass);
        assert!(!relation.membership_ids.is_empty());
        assert!(relation.reason_code.is_none(), "{relation:?}");
    }
    // Relations that cannot be expanded stay visible instead of being dropped.
    let failed = relations
        .iter()
        .filter(|relation| relation.status == RouteRelationCoverageStatus::Fail)
        .count();
    assert!(
        failed > 0,
        "the fixture is expected to keep unexpandable relations"
    );
    assert_eq!(
        coverage.route_relations.expanded_relation_count() + failed,
        relations.len()
    );
}

#[test]
#[ignore = "expands every route relation of the real OSM snapshot; run with --ignored"]
fn full_coverage_index_is_not_the_legacy_relation_subset() {
    let coverage = full_coverage();
    let graph = real_bound_graph();
    let inventory: RampInventoryFile = serde_json::from_slice(INVENTORY_BYTES).unwrap();
    let bindings: OsmRampBindingsFile = serde_json::from_slice(BINDINGS_BYTES).unwrap();
    let evidence = bound_ramp_evidence_from_inventory(graph, &inventory, &bindings).unwrap();
    let legacy = build_route_membership_indices_with_coverage(
        real_osm(),
        graph,
        &RouteMembershipBuildOptions {
            source_snapshot_sha256: compute_sha256(OSM_BYTES),
            relation_ids: default_route_relation_ids(real_osm()),
            bound_ramp_evidence: evidence,
        },
    )
    .unwrap();
    assert_ne!(
        route_memberships_sha256(&coverage.route_memberships).unwrap(),
        route_memberships_sha256(&legacy.route_memberships).unwrap()
    );
    assert_eq!(legacy.route_relations.relations.len(), 2);
}

#[test]
#[ignore = "derives every candidate from the real OSM snapshot; run with --ignored"]
fn pair_derivation_reports_every_membership_and_route_relation() {
    let coverage = full_coverage();
    let report = derive_pair_candidates_from_source_bytes(
        real_bound_graph(),
        &coverage.route_memberships,
        &coverage.route_relations.relations,
        OSM_BYTES,
        INVENTORY_BYTES,
        SUPPORT_DECISIONS_BYTES,
        BINDINGS_BYTES,
        ADJACENCY_BYTES,
        TARIFFS_BYTES,
        SEED_BYTES,
    )
    .unwrap();

    assert_eq!(report.schema_version, PAIR_DERIVATION_REPORT_SCHEMA_VERSION);
    assert_eq!(report.rule, PAIR_DERIVATION_RULE);
    assert_eq!(report.summary.candidate_total, 11);
    assert_eq!(report.summary.eligible_for_review, 9);
    assert_eq!(report.summary.hold, 2);

    // Every expanded membership has a manifest record, including the ones that
    // no candidate pair references.
    let mut manifest_ids = report
        .relation_manifest
        .iter()
        .map(|record| record.membership_id.as_str())
        .collect::<Vec<_>>();
    let mut membership_ids = coverage
        .route_memberships
        .iter()
        .map(|membership| membership.membership_id.as_str())
        .collect::<Vec<_>>();
    manifest_ids.sort_unstable();
    membership_ids.sort_unstable();
    assert_eq!(manifest_ids, membership_ids);
    assert!(report
        .relation_manifest
        .iter()
        .any(|record| record.candidate_pair_ids.is_empty()));
    for membership_id in [
        "route:C1:inner",
        "route:C1:outer",
        "route:2:inbound",
        "route:2:outbound",
    ] {
        let record = report
            .relation_manifest
            .iter()
            .find(|record| record.membership_id == membership_id)
            .unwrap_or_else(|| panic!("{membership_id} is missing from the relation manifest"));
        assert!(!record.candidate_pair_ids.is_empty(), "{membership_id}");
    }
    let c1_inner = report
        .relation_manifest
        .iter()
        .find(|record| record.membership_id == "route:C1:inner")
        .unwrap();
    assert_eq!(c1_inner.status, "fail");
    assert_eq!(c1_inner.route_plan_unresolved, 1);
    let c1_outer = report
        .relation_manifest
        .iter()
        .find(|record| record.membership_id == "route:C1:outer")
        .unwrap();
    assert_eq!(c1_outer.status, "pass");
    assert_eq!(c1_outer.relation_ids, vec![C1_RELATION_ID]);

    // Every route relation in the snapshot is reported with its expansion result.
    assert_eq!(report.relation_coverage.len(), 26);
    assert_eq!(report.summary.relation_coverage.relation_total, 26);
    assert_eq!(
        report.summary.relation_coverage.relation_expanded
            + report.summary.relation_coverage.relation_failed,
        report.relation_coverage.len()
    );
    assert_eq!(
        report.summary.relation_coverage.relation_failed,
        coverage.route_relations.failed_relations().count()
    );
    for relation in &report.relation_coverage {
        let source = coverage
            .route_relations
            .relations
            .iter()
            .find(|source| source.relation_id == relation.relation_id)
            .unwrap();
        assert_eq!(relation, source);
    }
}
