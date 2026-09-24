//! `shutoko-graph-builder` extracts directed road graphs and snap indices from OSM data.
//!
//! Reuses core types from `shutoko-routing-core` to guarantee schema compliance.

pub mod billing;
pub mod inventory;
pub mod manifest;
pub mod model;
pub mod osm;
pub mod route_membership;
pub mod seed;
pub mod topology;
pub mod validate;

pub use inventory::{
    apply_od_tariffs_to_graph, bind_ramps_to_graph, classify_endpoint_capabilities,
    ramps_artifact_to_deterministic_json, validate_endpoint_capability_contract,
    validate_od_tariffs, validate_osm_ramp_bindings, validate_osm_ramp_bindings_against_osm,
    validate_ramp_inventory, validate_verified_billing_pair_endpoints, CanonicalRampInventoryItem,
    OdTariffsFile, OsmRampBinding, OsmRampBindingsFile, RampArtifactEntry, RampInventoryFile,
    RampsArtifact, SharedPhysicalOverride, TariffRules,
};

pub use billing::{
    generate_and_validate_billing_pairs, generate_and_validate_parsed_billing_pairs,
    generate_billing_pair, BillingError, BillingGenerationReport, RejectedSeedRecord,
};
pub use manifest::{
    build_manifest, compute_sha256, manifest_to_deterministic_json, BillingPairProvenance,
    Manifest, ManifestArtifact, ManifestConfig, ManifestCoverage, ManifestEndpointCapabilities,
};
pub use model::{
    BillingPair, Edge, EdgeKind, Graph, Node, OdTariff, Price, Ramp, RampKind, SnapIndex, SnapNode,
    VerificationStatus,
};
pub use osm::{OsmElement, OsmMember, OverpassResponse};
pub use route_membership::{
    bound_ramp_evidence_from_inventory, build_bound_ramp_memberships, build_relation_memberships,
    build_route_membership_indices, build_route_memberships, find_first_exit_on_corridor,
    find_first_exit_on_corridor_from_edge, find_first_exit_on_corridor_with_budget,
    graph_schema_v4_to_deterministic_json, ordered_edge_ids_sha256,
    validate_directed_junction_mandatory_lap, validate_mandatory_lap,
    validate_route_membership_structure, validate_route_memberships, BoundRampEvidence,
    CorridorExit, GraphSchemaV4, RouteMembershipBuildOptions, RouteMembershipError,
    RouteMembershipIndex, RouteMembershipSegment, RouteMembershipSourceKind,
    CORRIDOR_EXIT_STATE_BUDGET, ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION,
};
pub use seed::{
    parse_billing_pairs_seed, AnchorKind, ArcPolicy, BillingPairSeed, BillingPairSeedEntry,
    BillingPairsSeedFile, BillingPairsSeedFileV2, BillingPairsSeedParseError, BindingCandidate,
    BindingCandidateStatus, DiagnosticEndpoint, DiagnosticRoutePlan, DiagnosticTariff,
    DirectedEndpointSegment, DirectedJunctionAnchor, EndpointSupportState, EntryCorridor,
    ExcludedShortConnector, FirstGeneralExit, FirstGeneralExitRule, LoopValidation,
    LoopValidationStatus, MandatoryLap, PairEligibility, PairEligibilityStatus, PairKind,
    ParsedBillingPairsSeed, ReturnCorridor, RoutePlanVersion, RoutingCapability, SeedPrice,
    SeedProvenance, TariffStatus,
};
pub use topology::{
    build_topology, build_topology_with_report, duration_seconds, haversine_distance_meters,
    is_shutoko_motorway, parse_oneway, snap_index_to_deterministic_json, to_deterministic_json,
    OnewayDirection, RestrictionReport, TopologyConfig, LOCAL_SPEED_KMH, RAMP_SPEED_KMH,
    SHUTOKO_SPEED_KMH,
};
pub use validate::{
    contains_forbidden_transition, find_first_exits_from_anchor,
    find_first_exits_from_anchor_with_budget, has_non_empty_shutoko_loop, parse_iso_date,
    parse_utc_timestamp, validate_billing_pair, validate_url, ValidationError,
    FIRST_EXIT_STATE_BUDGET,
};
