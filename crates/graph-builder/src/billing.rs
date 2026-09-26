//! Billing pair generation via pathfinding and constraint validation.
//!
//! Maps human-verified seed definitions (`BillingPairSeed`) to resolved graph edges and
//! derives directional, continuous paths:
//! - `entryToAnchorEdgeIds`: Begins with `Entry` edge, traverses `Shutoko` to `anchorNodeId`.
//! - `anchorToExitEdgeIds`: Begins at `anchorNodeId`, traverses `Shutoko`, concludes with `Exit`.
//!
//! Ensures both path segments combine into a simple path (no hidden loops or cycle crossovers)
//! and satisfy all graph routing restrictions.

use crate::inventory::{
    validate_od_tariffs, OdTariffsFile, OsmRampBindingsFile, RampInventoryFile,
};
use crate::manifest::compute_sha256;
use crate::model::{BillingPair, Edge, EdgeKind, Graph, Price, VerificationStatus};
use crate::route_membership::{
    bound_ramp_evidence_from_inventory, ordered_edge_ids_sha256, promote_verified_radial_pair,
    resolve_diagnostic_radial_route_plan, route_memberships_sha256,
    validate_route_membership_structure, BoundRampEvidence, DirectedRoutePlanResolution,
    RouteMembershipIndex, RouteMembershipSourceKind, RouteRelationCoverage,
    RouteRelationCoverageStatus,
};
use crate::seed::{
    AnchorKind, ArcPolicy, BillingPairSeed, BillingPairSeedEntry, BillingPairsSeedFile,
    BindingCandidate, BindingCandidateStatus, DiagnosticEndpoint, DiagnosticRoutePlan,
    DirectedEndpointSegment, DirectedJunctionAnchor, EndpointSupportState, EntryCorridor,
    ExcludedShortConnector, FirstGeneralExit, FirstGeneralExitRule, LoopValidation,
    LoopValidationStatus, MandatoryLap, PairEligibility, PairEligibilityStatus, PairKind,
    ParsedBillingPairsSeed, ReturnCorridor, RoutingCapability, SeedPrice, SeedProvenance,
    TariffStatus,
};
use crate::validate::{
    contains_forbidden_transition, has_non_empty_shutoko_loop, parse_iso_date,
    validate_billing_pair, validate_billing_pair_adjacency,
    validate_relation_constrained_legacy_first_exit, validate_url, RampSupportDecisionsFile,
    RelationConstrainedFirstExitStatus, ValidationError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};

/// Error encountered during billing pair generation from a seed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillingError {
    NoEntryEdgeForOsmWay(i64),
    NoExitEdgeForOsmWay(i64),
    AnchorNodeNotFound(String),
    NoPathEntryToAnchor(String),
    NoPathAnchorToExit(String),
    UnverifiedSectionMarkedAsVerified(String),
    InvalidProvenance(String),
    ValidationFailed(ValidationError),
}

impl std::fmt::Display for BillingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEntryEdgeForOsmWay(way_id) => {
                write!(f, "no Entry edge found in graph for OSM way {}", way_id)
            }
            Self::NoExitEdgeForOsmWay(way_id) => {
                write!(f, "no Exit edge found in graph for OSM way {}", way_id)
            }
            Self::AnchorNodeNotFound(node_id) => {
                write!(f, "anchor node \"{}\" not found in graph", node_id)
            }
            Self::NoPathEntryToAnchor(msg) => {
                write!(
                    f,
                    "cannot find valid Shutoko path from entry to anchor: {}",
                    msg
                )
            }
            Self::NoPathAnchorToExit(msg) => {
                write!(
                    f,
                    "cannot find valid Shutoko path from anchor to exit: {}",
                    msg
                )
            }
            Self::UnverifiedSectionMarkedAsVerified(id) => {
                write!(
                    f,
                    "seed \"{}\" marked status=verified but oneSectionAheadVerified is false",
                    id
                )
            }
            Self::InvalidProvenance(msg) => {
                write!(f, "invalid seed provenance: {}", msg)
            }
            Self::ValidationFailed(ve) => write!(f, "{}", ve),
        }
    }
}

impl std::error::Error for BillingError {}

/// Record of a seed entry that was rejected during generation or validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedSeedRecord {
    pub seed_id: String,
    pub reason: String,
}

/// Aggregated report of billing pair generation across all seeds.
#[derive(Debug, Clone)]
pub struct BillingGenerationReport {
    pub valid_pairs: Vec<BillingPair>,
    pub rejected_pairs: Vec<RejectedSeedRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadialRoutePlanGeneration {
    pub seed_id: String,
    pub resolution: Option<DirectedRoutePlanResolution>,
    pub error: Option<String>,
}

pub fn validate_radial_seed_binding_candidates(
    seed_file: &ParsedBillingPairsSeed,
    bindings: &OsmRampBindingsFile,
) -> Result<(), String> {
    for pair in seed_file.radial_pairs() {
        for endpoint in [&pair.entry_endpoint, &pair.exit_endpoint] {
            for candidate in &endpoint.binding_candidates {
                let audited = bindings
                    .binding_candidates
                    .iter()
                    .find(|binding| binding.candidate_id == candidate.candidate_id)
                    .ok_or_else(|| {
                        format!(
                            "radial pair {} candidate {} is absent from OSM binding evidence",
                            pair.id, candidate.candidate_id
                        )
                    })?;
                let expected_status = match candidate.status {
                    BindingCandidateStatus::Unresolved => "unresolved",
                    BindingCandidateStatus::Unsupported => "unsupported",
                };
                if audited.ramp_id != endpoint.ramp_id || audited.status != expected_status {
                    return Err(format!(
                        "radial pair {} candidate {} has conflicting ramp or support status",
                        pair.id, candidate.candidate_id
                    ));
                }
                if audited.directed_segments.len() != candidate.directed_segments.len() {
                    return Err(format!(
                        "radial pair {} candidate {} has a different directed segment count",
                        pair.id, candidate.candidate_id
                    ));
                }
                for (seed_segment, audited_segment) in candidate
                    .directed_segments
                    .iter()
                    .zip(&audited.directed_segments)
                {
                    if seed_segment.segment_id != audited_segment.segment_id
                        || seed_segment.osm_way_ids != audited_segment.osm_way_ids
                        || seed_segment.osm_node_ids != audited_segment.osm_node_ids
                        || seed_segment.edge_ids != audited_segment.edge_ids
                        || seed_segment.from_node_id != audited_segment.from_node_id
                        || seed_segment.to_node_id != audited_segment.to_node_id
                        || seed_segment.edge_ids_sha256 != audited_segment.edge_ids_sha256
                    {
                        return Err(format!(
                            "radial pair {} candidate {} differs from audited OSM binding evidence",
                            pair.id, candidate.candidate_id
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn generate_diagnostic_radial_route_plans(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    seed_file: &ParsedBillingPairsSeed,
) -> Vec<RadialRoutePlanGeneration> {
    seed_file
        .radial_pairs()
        .into_iter()
        .map(
            |seed| match resolve_diagnostic_radial_route_plan(graph, route_memberships, seed) {
                Ok(resolution) => RadialRoutePlanGeneration {
                    seed_id: seed.id.clone(),
                    resolution: Some(resolution),
                    error: None,
                },
                Err(error) => RadialRoutePlanGeneration {
                    seed_id: seed.id.clone(),
                    resolution: None,
                    error: Some(error.to_string()),
                },
            },
        )
        .collect()
}

/// Dijkstra search state for deterministic simple path finding.
#[derive(Clone, Eq, PartialEq)]
struct SearchState<'a> {
    cost: u64,
    node: &'a str,
    path: Vec<String>,
}

impl<'a> Ord for SearchState<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap by cost, tie-break by path length then edge IDs
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.path.len().cmp(&self.path.len()))
            .then_with(|| other.path.cmp(&self.path))
    }
}

impl<'a> PartialOrd for SearchState<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Find shortest simple path of `Shutoko` edges between two nodes using Dijkstra,
/// respecting forbidden transitions and excluding a set of forbidden visited nodes.
pub(crate) fn find_shutoko_path(
    graph: &Graph,
    start_node: &str,
    target_node: &str,
    initial_edge_id: Option<&str>,
    terminal_edge_id: Option<&str>,
    forbidden_nodes: &HashSet<&str>,
) -> Option<Vec<String>> {
    if start_node == target_node {
        // Check transition if initial and terminal edges are adjacent
        if let (Some(init), Some(term)) = (initial_edge_id, terminal_edge_id) {
            if contains_forbidden_transition(
                &[init.to_string(), term.to_string()],
                &graph.forbidden_transitions,
            ) {
                return None;
            }
        }
        return Some(Vec::new());
    }

    let mut outgoing: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
    let edge_map: HashMap<&str, &Edge> = graph.edges.iter().map(|e| (e.id.as_str(), e)).collect();

    for e in &graph.edges {
        if e.kind == EdgeKind::Shutoko {
            outgoing.entry(e.from.as_str()).or_default().push(e);
        }
    }
    // Sort outgoing deterministically by ID
    for edges in outgoing.values_mut() {
        edges.sort_by(|a, b| a.id.cmp(&b.id));
    }

    let mut heap = BinaryHeap::new();
    heap.push(SearchState {
        cost: 0,
        node: start_node,
        path: Vec::new(),
    });

    let mut best_cost_by_node: BTreeMap<&str, u64> = BTreeMap::new();

    while let Some(SearchState { cost, node, path }) = heap.pop() {
        if node == target_node {
            // Check terminal edge transition if provided
            if let Some(term) = terminal_edge_id {
                let mut full_check = Vec::with_capacity(path.len() + 2);
                if let Some(init) = initial_edge_id {
                    candidate_path_init(&mut full_check, init);
                }
                full_check.extend(path.clone());
                full_check.push(term.to_string());
                if contains_forbidden_transition(&full_check, &graph.forbidden_transitions) {
                    continue;
                }
            }
            return Some(path);
        }

        if let Some(&best) = best_cost_by_node.get(node) {
            if cost > best {
                continue;
            }
        }

        if let Some(edges) = outgoing.get(node) {
            for e in edges {
                let next_node = e.to.as_str();

                // Simple path constraint: do not revisit any node already in path
                if forbidden_nodes.contains(next_node) {
                    continue;
                }
                if path.iter().any(|id| {
                    edge_map
                        .get(id.as_str())
                        .is_some_and(|edge| edge.from == next_node || edge.to == next_node)
                }) {
                    continue;
                }

                // Check forbidden transitions
                let mut candidate_path = Vec::with_capacity(path.len() + 2);
                if let Some(init) = initial_edge_id {
                    candidate_path_init(&mut candidate_path, init);
                }
                candidate_path.extend(path.clone());
                candidate_path.push(e.id.clone());

                if contains_forbidden_transition(&candidate_path, &graph.forbidden_transitions) {
                    continue;
                }

                let next_cost = cost.saturating_add(e.duration_seconds);
                let mut new_path = path.clone();
                new_path.push(e.id.clone());

                if next_cost < *best_cost_by_node.get(next_node).unwrap_or(&u64::MAX) {
                    best_cost_by_node.insert(next_node, next_cost);
                    heap.push(SearchState {
                        cost: next_cost,
                        node: next_node,
                        path: new_path,
                    });
                }
            }
        }
    }

    None
}

fn candidate_path_init(vec: &mut Vec<String>, init: &str) {
    vec.push(init.to_string());
}

/// Attempt to generate and validate a `BillingPair` from a seed entry and graph.
pub fn generate_billing_pair(
    graph: &Graph,
    seed: &BillingPairSeed,
) -> Result<BillingPair, BillingError> {
    // 1. One-section-ahead verification assertion
    if seed.status == VerificationStatus::Verified && !seed.one_section_ahead_verified {
        return Err(BillingError::UnverifiedSectionMarkedAsVerified(
            seed.id.clone(),
        ));
    }

    // 2. Provenance format validation
    validate_url(&seed.provenance.source).map_err(|e| {
        BillingError::InvalidProvenance(format!(
            "invalid source URL \"{}\" for seed \"{}\": {}",
            seed.provenance.source, seed.id, e
        ))
    })?;
    parse_iso_date(&seed.provenance.source_date).map_err(|e| {
        BillingError::InvalidProvenance(format!(
            "invalid sourceDate \"{}\" for seed \"{}\": {}",
            seed.provenance.source_date, seed.id, e
        ))
    })?;

    // 2. Resolve entry edge
    let entry_prefix = format!("e:w{}:", seed.entry_osm_way_id);
    let mut entry_candidates: Vec<&Edge> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Entry && e.id.starts_with(&entry_prefix))
        .collect();
    entry_candidates.sort_by(|a, b| a.id.cmp(&b.id));

    let entry_edge = entry_candidates
        .first()
        .copied()
        .ok_or(BillingError::NoEntryEdgeForOsmWay(seed.entry_osm_way_id))?;

    // 3. Resolve exit edge
    let exit_prefix = format!("e:w{}:", seed.exit_osm_way_id);
    let mut exit_candidates: Vec<&Edge> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Exit && e.id.starts_with(&exit_prefix))
        .collect();
    exit_candidates.sort_by(|a, b| a.id.cmp(&b.id));

    let exit_edge = exit_candidates
        .last()
        .copied()
        .ok_or(BillingError::NoExitEdgeForOsmWay(seed.exit_osm_way_id))?;

    // 4. Resolve anchor node
    let anchor_node_id = format!("n:{}", seed.anchor_osm_node_id);
    if !graph.nodes.iter().any(|n| n.id == anchor_node_id) {
        return Err(BillingError::AnchorNodeNotFound(anchor_node_id));
    }

    // 5. Derive entryToAnchor path
    let mut entry_forbidden = HashSet::new();
    entry_forbidden.insert(entry_edge.from.as_str());

    let entry_to_anchor_shutoko = find_shutoko_path(
        graph,
        &entry_edge.to,
        &anchor_node_id,
        Some(&entry_edge.id),
        None,
        &entry_forbidden,
    )
    .ok_or_else(|| {
        BillingError::NoPathEntryToAnchor(format!(
            "from entry node {} to anchor node {}",
            entry_edge.to, anchor_node_id
        ))
    })?;

    let mut entry_to_anchor_edge_ids = vec![entry_edge.id.clone()];
    entry_to_anchor_edge_ids.extend(entry_to_anchor_shutoko);

    // 6. Collect nodes visited in entryToAnchor (to enforce simple path in anchorToExit)
    let mut visited_in_entry = HashSet::new();
    visited_in_entry.insert(entry_edge.from.as_str());
    visited_in_entry.insert(entry_edge.to.as_str());
    for id in &entry_to_anchor_edge_ids[1..] {
        if let Some(e) = graph.edges.iter().find(|edge| &edge.id == id) {
            visited_in_entry.insert(e.from.as_str());
            visited_in_entry.insert(e.to.as_str());
        }
    }
    // Anchor node is allowed as the shared boundary
    visited_in_entry.remove(anchor_node_id.as_str());
    // Also forbid exit_edge.to from appearing in Shutoko intermediate path
    visited_in_entry.insert(exit_edge.to.as_str());

    // 7. Derive anchorToExit path
    let anchor_to_exit_shutoko = find_shutoko_path(
        graph,
        &anchor_node_id,
        &exit_edge.from,
        entry_to_anchor_edge_ids.last().map(String::as_str),
        Some(&exit_edge.id),
        &visited_in_entry,
    )
    .ok_or_else(|| {
        BillingError::NoPathAnchorToExit(format!(
            "from anchor node {} to exit node {}",
            anchor_node_id, exit_edge.from
        ))
    })?;

    let mut anchor_to_exit_edge_ids = anchor_to_exit_shutoko;
    anchor_to_exit_edge_ids.push(exit_edge.id.clone());

    // 8. Convert prices
    let prices: Vec<Price> = seed
        .prices
        .iter()
        .map(|p| Price {
            amount_yen: p.amount_yen,
            effective_from: p.effective_from.clone(),
            effective_to: p.effective_to.clone(),
        })
        .collect();

    // 9. Construct pair
    let pair = BillingPair {
        id: seed.id.clone(),
        entry_id: entry_edge.id.clone(),
        exit_id: exit_edge.id.clone(),
        anchor_node_id: anchor_node_id.clone(),
        entry_to_anchor_edge_ids,
        anchor_to_exit_edge_ids,
        status: seed.status,
        vehicle_profile: seed.vehicle_profile.clone(),
        assignment_id: seed.assignment_id.clone(),
        prices,
        entry_name: seed.entry_name.clone(),
        exit_name: seed.exit_name.clone(),
        entry_ramp_id: None,
        exit_ramp_id: None,
        billing_distance_meters: None,
    };

    // 10. Run comprehensive validator
    validate_billing_pair(graph, &pair).map_err(BillingError::ValidationFailed)?;

    Ok(pair)
}

pub(crate) fn generate_billing_pair_for_relation_review(
    graph: &Graph,
    seed: &BillingPairSeed,
) -> Result<BillingPair, BillingError> {
    if seed.status == VerificationStatus::Verified && !seed.one_section_ahead_verified {
        return Err(BillingError::UnverifiedSectionMarkedAsVerified(
            seed.id.clone(),
        ));
    }
    let mut review_seed = seed.clone();
    review_seed.status = VerificationStatus::Unverified;
    let mut pair = generate_billing_pair(graph, &review_seed)?;
    pair.status = seed.status;
    Ok(pair)
}

/// Process a collection of billing pair seeds against a graph, recording valid pairs
/// and explicitly capturing rejection reasons for invalid entries.
pub fn generate_and_validate_billing_pairs(
    graph: &Graph,
    seed_file: &BillingPairsSeedFile,
) -> BillingGenerationReport {
    let mut valid_pairs = Vec::new();
    let mut rejected_pairs = Vec::new();

    for seed in &seed_file.billing_pairs {
        match generate_billing_pair(graph, seed) {
            Ok(pair) => valid_pairs.push(pair),
            Err(e) => rejected_pairs.push(RejectedSeedRecord {
                seed_id: seed.id.clone(),
                reason: e.to_string(),
            }),
        }
    }

    valid_pairs.sort_by(|a, b| a.id.cmp(&b.id));
    rejected_pairs.sort_by(|a, b| a.seed_id.cmp(&b.seed_id));

    BillingGenerationReport {
        valid_pairs,
        rejected_pairs,
    }
}

pub fn generate_and_validate_parsed_billing_pairs(
    graph: &Graph,
    seed_file: &ParsedBillingPairsSeed,
) -> BillingGenerationReport {
    match seed_file {
        ParsedBillingPairsSeed::Schema1(seed) => generate_and_validate_billing_pairs(graph, seed),
        ParsedBillingPairsSeed::Schema2(seed) => {
            let mut valid_pairs = Vec::new();
            let mut rejected_pairs = Vec::new();

            for entry in &seed.billing_pairs {
                match entry {
                    BillingPairSeedEntry::LegacyRing(seed) => {
                        match generate_billing_pair(graph, seed) {
                            Ok(pair) => valid_pairs.push(pair),
                            Err(error) => rejected_pairs.push(RejectedSeedRecord {
                                seed_id: seed.id.clone(),
                                reason: error.to_string(),
                            }),
                        }
                    }
                    BillingPairSeedEntry::RadialReturn(seed) => {
                        rejected_pairs.push(RejectedSeedRecord {
                            seed_id: seed.id.clone(),
                            reason:
                                "radialReturn pair requires schema 4 route-plan resolution and is not emitted by the legacy billing-pair adapter"
                                    .to_string(),
                        });
                    }
                }
            }

            valid_pairs.sort_by(|a, b| a.id.cmp(&b.id));
            rejected_pairs.sort_by(|a, b| a.seed_id.cmp(&b.seed_id));

            BillingGenerationReport {
                valid_pairs,
                rejected_pairs,
            }
        }
    }
}

pub fn generate_and_validate_parsed_billing_pairs_for_relation_review(
    graph: &Graph,
    seed_file: &ParsedBillingPairsSeed,
) -> BillingGenerationReport {
    match seed_file {
        ParsedBillingPairsSeed::Schema1(seed) => {
            let mut valid_pairs = Vec::new();
            let mut rejected_pairs = Vec::new();
            for entry in &seed.billing_pairs {
                match generate_billing_pair_for_relation_review(graph, entry) {
                    Ok(pair) => valid_pairs.push(pair),
                    Err(error) => rejected_pairs.push(RejectedSeedRecord {
                        seed_id: entry.id.clone(),
                        reason: error.to_string(),
                    }),
                }
            }
            valid_pairs.sort_by(|a, b| a.id.cmp(&b.id));
            rejected_pairs.sort_by(|a, b| a.seed_id.cmp(&b.seed_id));
            BillingGenerationReport {
                valid_pairs,
                rejected_pairs,
            }
        }
        ParsedBillingPairsSeed::Schema2(seed) => {
            let mut valid_pairs = Vec::new();
            let mut rejected_pairs = Vec::new();
            for entry in &seed.billing_pairs {
                match entry {
                    BillingPairSeedEntry::LegacyRing(seed) => {
                        match generate_billing_pair_for_relation_review(graph, seed) {
                            Ok(pair) => valid_pairs.push(pair),
                            Err(error) => rejected_pairs.push(RejectedSeedRecord {
                                seed_id: seed.id.clone(),
                                reason: error.to_string(),
                            }),
                        }
                    }
                    BillingPairSeedEntry::RadialReturn(seed) => {
                        rejected_pairs.push(RejectedSeedRecord {
                            seed_id: seed.id.clone(),
                            reason:
                                "radialReturn pair requires schema 4 route-plan resolution and is not emitted by the legacy billing-pair adapter"
                                    .to_string(),
                        });
                    }
                }
            }
            valid_pairs.sort_by(|a, b| a.id.cmp(&b.id));
            rejected_pairs.sort_by(|a, b| a.seed_id.cmp(&b.seed_id));
            BillingGenerationReport {
                valid_pairs,
                rejected_pairs,
            }
        }
    }
}

pub fn validate_promoted_legacy_pairs_from_source(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    inventory: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
    support_decisions_json: &[u8],
    adjacency_json: &[u8],
) -> Result<(), String> {
    let support_decisions: RampSupportDecisionsFile =
        serde_json::from_slice(support_decisions_json).map_err(|error| error.to_string())?;
    let adjacency: BillingPairAdjacencyFile =
        serde_json::from_slice(adjacency_json).map_err(|error| error.to_string())?;
    for pair in graph
        .billing_pairs
        .iter()
        .filter(|pair| pair.status == VerificationStatus::Verified)
    {
        let record = adjacency
            .pairs
            .iter()
            .find(|record| record.pair_id == pair.id)
            .ok_or_else(|| format!("verified pair {} has no adjacency record", pair.id))?;
        let BillingPairAdjacencyRoutePlan::SameNode {
            membership_id,
            anchor_node_id,
            first_exit_initial_edge_id,
            exit_approach_edge_ids,
        } = record
            .route_plan
            .as_ref()
            .ok_or_else(|| format!("verified pair {} has no sameNode route plan", pair.id))?
        else {
            return Err(format!(
                "verified pair {} is not a legacyRing plan",
                pair.id
            ));
        };
        if !route_memberships
            .iter()
            .any(|membership| membership.membership_id == *membership_id)
        {
            return Err(format!(
                "verified pair {} references unknown membership {}",
                pair.id, membership_id
            ));
        }
        let result = validate_relation_constrained_legacy_first_exit(
            graph,
            route_memberships,
            inventory,
            bindings,
            &support_decisions,
            &adjacency,
            membership_id,
            anchor_node_id,
            first_exit_initial_edge_id,
            &record.exit_ramp_id,
            exit_approach_edge_ids,
        )
        .map_err(|errors| format!("{}: {}", pair.id, errors.join("; ")))?;
        if result.status != RelationConstrainedFirstExitStatus::Verified {
            return Err(format!(
                "verified pair {} did not pass relation-constrained First Exit: {:?}",
                pair.id, result.status
            ));
        }
    }
    Ok(())
}

pub const BILLING_PAIR_ADJACENCY_SCHEMA_VERSION: u32 = 1;
pub const PAIR_DERIVATION_REPORT_SCHEMA_VERSION: u32 = 2;
pub const PAIR_DERIVATION_RULE: &str = "billingPairDerivation/v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BillingPairAdjacencyKind {
    LegacyRing,
    RadialReturn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BillingPairAdjacencyReviewStatus {
    Reviewed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairAdjacencyFile {
    pub schema_version: u32,
    pub source: String,
    pub source_date: String,
    pub description: String,
    pub pairs: Vec<BillingPairAdjacency>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairAdjacency {
    pub evidence_id: String,
    pub pair_id: String,
    pub pair_kind: BillingPairAdjacencyKind,
    pub review_status: BillingPairAdjacencyReviewStatus,
    pub route_id: String,
    pub direction: String,
    pub entry_ramp_id: String,
    pub exit_ramp_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_id: Option<String>,
    pub entry_name: String,
    pub exit_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_osm_way_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_osm_way_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_plan: Option<BillingPairAdjacencyRoutePlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BillingPairAdjacencyRoutePlan {
    SameNode {
        membership_id: String,
        anchor_node_id: String,
        first_exit_initial_edge_id: String,
        exit_approach_edge_ids: Vec<String>,
    },
    DirectedJunction {
        entry_corridor: BillingPairAdjacencyEntryCorridor,
        anchor: Box<BillingPairAdjacencyDirectedAnchor>,
        mandatory_lap: BillingPairAdjacencyMandatoryLap,
        return_corridor: BillingPairAdjacencyReturnCorridor,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairAdjacencyEntryCorridor {
    pub membership_id: String,
    pub terminal_edge_id: String,
    pub merge_node_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairAdjacencyDirectedAnchor {
    pub route_id: String,
    pub direction: String,
    pub merge_node_id: String,
    pub branch_node_id: String,
    pub merge_terminal_edge_id: String,
    pub branch_initial_edge_id: String,
    pub excluded_short_connector: BillingPairAdjacencyExcludedShortConnector,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairAdjacencyExcludedShortConnector {
    pub from_node_id: String,
    pub to_node_id: String,
    pub osm_way_id: i64,
    pub edge_count: u32,
    pub distance_meters: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairAdjacencyMandatoryLap {
    pub membership_id: String,
    pub first_edge_id: String,
    pub last_edge_id: String,
    pub lap_count: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairAdjacencyReturnCorridor {
    pub membership_id: String,
    pub start_node_id: String,
    pub initial_edge_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDerivationInputHashes {
    pub osm_snapshot_sha256: String,
    pub ramp_ledger_sha256: String,
    pub route_membership_index_sha256: String,
    pub billing_pair_adjacency_sha256: String,
    pub od_tariffs_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairDerivationGateStatus {
    Passed,
    Failed,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDerivationGate {
    pub status: PairDerivationGateStatus,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDerivationGates {
    pub official_adjacency: PairDerivationGate,
    pub route: PairDerivationGate,
    pub direction: PairDerivationGate,
    pub first_exit: PairDerivationGate,
    pub mandatory_lap: PairDerivationGate,
    pub entry_binding: PairDerivationGate,
    pub exit_binding: PairDerivationGate,
    pub tariff_assignment: PairDerivationGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairDerivationProductEligibilityStatus {
    VerifiedOneSectionAhead,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairDerivationPromotionDecision {
    EligibleForReview,
    Hold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairDerivationRouteRole {
    EntryApproach,
    MandatoryLap,
    ReturnCorridor,
    ExitApproach,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairCandidateEndpointReport {
    pub ramp_id: String,
    pub name: String,
    pub support_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_reason_code: Option<String>,
    pub binding_evidence_id: Option<String>,
    pub route_membership_id: Option<String>,
    pub gate: PairDerivationGate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairCandidateRouteRoleReport {
    pub role: PairDerivationRouteRole,
    pub status: PairDerivationGateStatus,
    pub edge_ids_sha256: Option<String>,
    pub source_segment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairCandidateRoutePlanReport {
    pub route_plan_id: String,
    pub edge_ids_sha256: Option<String>,
    pub loop_validation_status: String,
    pub source_segment_ids: Vec<String>,
    pub resolved_roles: Vec<PairCandidateRouteRoleReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairCandidateTariffPriceReport {
    pub status: String,
    pub tariff_status: String,
    pub amount_yen: Option<u64>,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub rule_id: String,
    pub evidence_id: String,
    pub distance_evidence_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairCandidateTariffReport {
    pub status: String,
    pub assignment_id: Option<String>,
    pub billing_distance_meters: Option<u64>,
    pub prices: Vec<PairCandidateTariffPriceReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairCandidateProductEligibility {
    pub status: PairDerivationProductEligibilityStatus,
    pub official_adjacency_evidence_id: String,
    pub one_section_ahead_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairCandidateReport {
    pub candidate_id: String,
    pub pair_id: String,
    pub pair_kind: BillingPairAdjacencyKind,
    pub route_id: String,
    pub direction: String,
    pub entry: PairCandidateEndpointReport,
    pub exit: PairCandidateEndpointReport,
    pub route_plan: PairCandidateRoutePlanReport,
    pub gates: PairDerivationGates,
    pub product_eligibility: PairCandidateProductEligibility,
    pub tariff: PairCandidateTariffReport,
    pub promotion_decision: PairDerivationPromotionDecision,
    pub automatic_seed_write: bool,
    pub rejection_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDerivationRelationManifest {
    pub membership_id: String,
    pub route_id: String,
    pub direction: String,
    pub relation_ids: Vec<i64>,
    pub candidate_pair_ids: Vec<String>,
    pub route_plan_resolved: usize,
    pub route_plan_unresolved: usize,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDerivationRelationCoverageSummary {
    pub relation_total: usize,
    pub relation_expanded: usize,
    pub relation_failed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDerivationSummary {
    pub candidate_total: usize,
    pub eligible_for_review: usize,
    pub hold: usize,
    pub relation_coverage: PairDerivationRelationCoverageSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairDerivationReport {
    pub schema_version: u32,
    pub rule: String,
    pub automatic_seed_write: bool,
    pub input_hashes: PairDerivationInputHashes,
    pub relation_coverage: Vec<RouteRelationCoverage>,
    pub relation_manifest: Vec<PairDerivationRelationManifest>,
    pub candidates: Vec<PairCandidateReport>,
    pub summary: PairDerivationSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairDerivationError {
    pub code: &'static str,
    pub message: String,
}

impl PairDerivationError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PairDerivationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PairDerivationError {}

pub fn compute_pair_derivation_input_hashes(
    osm_snapshot: &[u8],
    ramp_inventory: &[u8],
    ramp_support_decisions: &[u8],
    osm_ramp_bindings: &[u8],
    route_memberships: &[RouteMembershipIndex],
    billing_pair_adjacency: &[u8],
    od_tariffs: &[u8],
) -> Result<PairDerivationInputHashes, PairDerivationError> {
    let ledger = serde_json::Value::Array(vec![
        serde_json::from_slice::<Value>(ramp_inventory).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
        })?,
        serde_json::from_slice::<Value>(ramp_support_decisions).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
        })?,
        serde_json::from_slice::<Value>(osm_ramp_bindings).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
        })?,
    ]);
    let ledger_bytes = serde_json::to_vec(&ledger).map_err(|error| {
        PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
    })?;
    Ok(PairDerivationInputHashes {
        osm_snapshot_sha256: compute_sha256(osm_snapshot),
        ramp_ledger_sha256: compute_sha256(&ledger_bytes),
        route_membership_index_sha256: route_memberships_sha256(route_memberships).map_err(
            |error| PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string()),
        )?,
        billing_pair_adjacency_sha256: compute_sha256(billing_pair_adjacency),
        od_tariffs_sha256: compute_sha256(od_tariffs),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn derive_pair_candidates_from_source_bytes(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    relation_coverage: &[RouteRelationCoverage],
    osm_snapshot: &[u8],
    ramp_inventory: &[u8],
    ramp_support_decisions: &[u8],
    osm_ramp_bindings: &[u8],
    billing_pair_adjacency: &[u8],
    od_tariffs: &[u8],
    billing_pair_seed: &[u8],
) -> Result<PairDerivationReport, PairDerivationError> {
    let inventory =
        serde_json::from_slice::<RampInventoryFile>(ramp_inventory).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
        })?;
    let bindings =
        serde_json::from_slice::<OsmRampBindingsFile>(osm_ramp_bindings).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
        })?;
    let support_decisions =
        serde_json::from_slice::<RampSupportDecisionsFile>(ramp_support_decisions).map_err(
            |error| PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string()),
        )?;
    let adjacency = serde_json::from_slice::<BillingPairAdjacencyFile>(billing_pair_adjacency)
        .map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
        })?;
    let tariffs = serde_json::from_slice::<OdTariffsFile>(od_tariffs).map_err(|error| {
        PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
    })?;
    let billing_pair_seed = std::str::from_utf8(billing_pair_seed).map_err(|error| {
        PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
    })?;
    let billing_pair_seed =
        crate::seed::parse_billing_pairs_seed(billing_pair_seed).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_INPUT_INVALID", error.to_string())
        })?;
    let input_hashes = compute_pair_derivation_input_hashes(
        osm_snapshot,
        ramp_inventory,
        ramp_support_decisions,
        osm_ramp_bindings,
        route_memberships,
        billing_pair_adjacency,
        od_tariffs,
    )?;
    derive_pair_candidates(
        graph,
        route_memberships,
        relation_coverage,
        &adjacency,
        &tariffs,
        &inventory,
        &bindings,
        &support_decisions,
        &billing_pair_seed,
        input_hashes,
    )
}

pub fn pair_derivation_report_to_deterministic_json(
    report: &PairDerivationReport,
) -> Result<String, serde_json::Error> {
    let mut output = serde_json::to_string_pretty(report)?;
    output.push('\n');
    Ok(output)
}

fn endpoint_support_state_wire_value(state: EndpointSupportState) -> &'static str {
    match state {
        EndpointSupportState::VerifiedBound => "verified_bound",
        EndpointSupportState::Unresolved => "unresolved",
        EndpointSupportState::Unsupported => "unsupported",
    }
}

fn gate<S: AsRef<str>>(
    status: PairDerivationGateStatus,
    reason_codes: impl IntoIterator<Item = S>,
) -> PairDerivationGate {
    let mut reason_codes = reason_codes
        .into_iter()
        .map(|reason| reason.as_ref().to_string())
        .collect::<Vec<_>>();
    reason_codes.sort();
    reason_codes.dedup();
    PairDerivationGate {
        status,
        reason_codes,
    }
}

fn passed_gate() -> PairDerivationGate {
    gate(PairDerivationGateStatus::Passed, Vec::<String>::new())
}

struct ResolvedEndpoint {
    report: PairCandidateEndpointReport,
    state: EndpointSupportState,
    edge_id: Option<String>,
    directed_segments: Vec<DirectedEndpointSegment>,
    binding_candidates: Vec<BindingCandidate>,
}

fn directed_segment_from_evidence(evidence: &BoundRampEvidence) -> DirectedEndpointSegment {
    DirectedEndpointSegment {
        segment_id: format!("{}:segment:0", evidence.binding_evidence_id),
        osm_way_ids: evidence.osm_way_ids.clone(),
        osm_node_ids: evidence.osm_node_ids.clone(),
        edge_ids: evidence.edge_ids.clone(),
        from_node_id: evidence.from_node_id.clone(),
        to_node_id: evidence.to_node_id.clone(),
        edge_ids_sha256: evidence.edge_ids_sha256.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_endpoint(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    inventory: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
    bound_evidence: &[BoundRampEvidence],
    ramp_id: &str,
    expected_kind: crate::model::RampKind,
    role: &str,
) -> Result<ResolvedEndpoint, PairDerivationError> {
    let inventory_item = inventory
        .ramps
        .iter()
        .find(|ramp| ramp.ramp_id == ramp_id)
        .ok_or_else(|| {
            PairDerivationError::new(
                "PAIR_DERIVATION_ENDPOINT_UNKNOWN",
                format!("{role} ramp {ramp_id} is absent from inventory"),
            )
        })?;
    if inventory_item.kind != expected_kind {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_ENDPOINT_KIND_MISMATCH",
            format!("{role} ramp {ramp_id} has the wrong ramp kind"),
        ));
    }
    let unresolved_candidate = bindings
        .binding_candidates
        .iter()
        .filter(|candidate| candidate.ramp_id == ramp_id && candidate.status == "unresolved")
        .min_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    if inventory_item.support_state.as_deref() == Some("unresolved") {
        // 恒久的な利用不可ではなく exact binding が未解決という状態。時間帯モデルで
        // 解決しうる証拠（access:conditional など）は reason code で区別する。
        let role_code = role.to_ascii_uppercase();
        return Ok(ResolvedEndpoint {
            report: PairCandidateEndpointReport {
                ramp_id: ramp_id.to_string(),
                name: inventory_item.facility_name.clone(),
                support_state: endpoint_support_state_wire_value(EndpointSupportState::Unresolved)
                    .to_string(),
                support_reason_code: inventory_item.support_reason_code.clone(),
                binding_evidence_id: unresolved_candidate.map(|candidate| {
                    format!("osm-ramp-binding-candidate:{}", candidate.candidate_id)
                }),
                route_membership_id: None,
                gate: gate(
                    PairDerivationGateStatus::Unresolved,
                    [format!("{role_code}_BINDING_UNRESOLVED")],
                ),
            },
            state: EndpointSupportState::Unresolved,
            edge_id: None,
            directed_segments: Vec::new(),
            binding_candidates: Vec::new(),
        });
    }
    if inventory_item.support_state.as_deref() == Some("verified_bound") {
        let graph_ramp = graph.ramps.iter().find(|ramp| ramp.id == ramp_id);
        let evidence = bound_evidence
            .iter()
            .find(|evidence| evidence.ramp_id == ramp_id);
        let membership_id = evidence.and_then(|evidence| {
            route_memberships
                .iter()
                .find(|membership| {
                    membership.segments.iter().any(|segment| {
                        segment.source_kind == RouteMembershipSourceKind::BoundRamp
                            && segment.binding_evidence_id.as_deref()
                                == Some(evidence.binding_evidence_id.as_str())
                    })
                })
                .map(|membership| membership.membership_id.clone())
        });
        let mut reason_codes = Vec::new();
        if graph_ramp.is_none() {
            reason_codes.push(format!(
                "{}_BINDING_NOT_PROJECTED",
                role.to_ascii_uppercase()
            ));
        }
        if evidence.is_none() || membership_id.is_none() {
            reason_codes.push(format!(
                "{}_BINDING_EVIDENCE_MISSING",
                role.to_ascii_uppercase()
            ));
        }
        let gate_status = if reason_codes.is_empty() {
            PairDerivationGateStatus::Passed
        } else {
            PairDerivationGateStatus::Failed
        };
        return Ok(ResolvedEndpoint {
            report: PairCandidateEndpointReport {
                ramp_id: ramp_id.to_string(),
                name: inventory_item.facility_name.clone(),
                support_state: endpoint_support_state_wire_value(
                    EndpointSupportState::VerifiedBound,
                )
                .to_string(),
                support_reason_code: None,
                binding_evidence_id: evidence.map(|value| value.binding_evidence_id.clone()),
                route_membership_id: membership_id,
                gate: gate(gate_status, reason_codes),
            },
            state: EndpointSupportState::VerifiedBound,
            edge_id: graph_ramp.map(|ramp| ramp.edge_id.clone()),
            directed_segments: evidence
                .map(|value| vec![directed_segment_from_evidence(value)])
                .unwrap_or_default(),
            binding_candidates: Vec::new(),
        });
    }
    if let Some(candidate) = unresolved_candidate {
        let binding_evidence_id = format!("osm-ramp-binding-candidate:{}", candidate.candidate_id);
        let directed_segments = candidate
            .directed_segments
            .iter()
            .map(|segment| DirectedEndpointSegment {
                segment_id: segment.segment_id.clone(),
                osm_way_ids: segment.osm_way_ids.clone(),
                osm_node_ids: segment.osm_node_ids.clone(),
                edge_ids: segment.edge_ids.clone(),
                from_node_id: segment.from_node_id.clone(),
                to_node_id: segment.to_node_id.clone(),
                edge_ids_sha256: segment.edge_ids_sha256.clone(),
            })
            .collect::<Vec<_>>();
        return Ok(ResolvedEndpoint {
            report: PairCandidateEndpointReport {
                ramp_id: ramp_id.to_string(),
                name: inventory_item.facility_name.clone(),
                support_state: endpoint_support_state_wire_value(EndpointSupportState::Unresolved)
                    .to_string(),
                support_reason_code: None,
                binding_evidence_id: Some(binding_evidence_id),
                route_membership_id: None,
                gate: gate(
                    PairDerivationGateStatus::Unresolved,
                    [format!("{}_BINDING_UNRESOLVED", role.to_ascii_uppercase())],
                ),
            },
            state: EndpointSupportState::Unresolved,
            edge_id: directed_segments
                .first()
                .and_then(|segment| segment.edge_ids.first().cloned()),
            directed_segments,
            binding_candidates: vec![BindingCandidate {
                candidate_id: candidate.candidate_id.clone(),
                status: BindingCandidateStatus::Unresolved,
                directed_segments: candidate
                    .directed_segments
                    .iter()
                    .map(|segment| DirectedEndpointSegment {
                        segment_id: segment.segment_id.clone(),
                        osm_way_ids: segment.osm_way_ids.clone(),
                        osm_node_ids: segment.osm_node_ids.clone(),
                        edge_ids: segment.edge_ids.clone(),
                        from_node_id: segment.from_node_id.clone(),
                        to_node_id: segment.to_node_id.clone(),
                        edge_ids_sha256: segment.edge_ids_sha256.clone(),
                    })
                    .collect(),
            }],
        });
    }
    let reason = format!("{}_BINDING_UNSUPPORTED", role.to_ascii_uppercase());
    Ok(ResolvedEndpoint {
        report: PairCandidateEndpointReport {
            ramp_id: ramp_id.to_string(),
            name: inventory_item.facility_name.clone(),
            support_state: endpoint_support_state_wire_value(EndpointSupportState::Unsupported)
                .to_string(),
            support_reason_code: None,
            binding_evidence_id: None,
            route_membership_id: None,
            gate: gate(PairDerivationGateStatus::Failed, [reason]),
        },
        state: EndpointSupportState::Unsupported,
        edge_id: None,
        directed_segments: Vec::new(),
        binding_candidates: Vec::new(),
    })
}

fn source_segment_ids_for_edges(
    route_memberships: &[RouteMembershipIndex],
    edge_ids: &[String],
) -> Vec<String> {
    let edge_ids = edge_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut result = route_memberships
        .iter()
        .flat_map(|membership| membership.segments.iter())
        .filter(|segment| {
            segment
                .ordered_edge_ids
                .iter()
                .any(|edge_id| edge_ids.contains(edge_id.as_str()))
        })
        .map(|segment| segment.segment_id.clone())
        .collect::<Vec<_>>();
    result.sort();
    result.dedup();
    result
}

fn route_role_report(
    route_memberships: &[RouteMembershipIndex],
    role: PairDerivationRouteRole,
    status: PairDerivationGateStatus,
    edge_ids: Option<&[String]>,
) -> PairCandidateRouteRoleReport {
    let edge_ids_sha256 = edge_ids.and_then(|edge_ids| {
        if edge_ids.is_empty() {
            None
        } else {
            ordered_edge_ids_sha256(edge_ids).ok()
        }
    });
    PairCandidateRouteRoleReport {
        role,
        status,
        edge_ids_sha256,
        source_segment_ids: edge_ids
            .map(|edge_ids| source_segment_ids_for_edges(route_memberships, edge_ids))
            .unwrap_or_default(),
    }
}

fn unresolved_route_roles() -> Vec<PairCandidateRouteRoleReport> {
    vec![
        route_role_report(
            &[],
            PairDerivationRouteRole::EntryApproach,
            PairDerivationGateStatus::Unresolved,
            None,
        ),
        route_role_report(
            &[],
            PairDerivationRouteRole::MandatoryLap,
            PairDerivationGateStatus::Unresolved,
            None,
        ),
        route_role_report(
            &[],
            PairDerivationRouteRole::ReturnCorridor,
            PairDerivationGateStatus::Unresolved,
            None,
        ),
        route_role_report(
            &[],
            PairDerivationRouteRole::ExitApproach,
            PairDerivationGateStatus::Unresolved,
            None,
        ),
    ]
}

fn relation_mainline_lap(
    graph: &Graph,
    membership: &RouteMembershipIndex,
    anchor_node_id: &str,
    initial_edge_id: &str,
) -> Result<Vec<String>, PairDerivationError> {
    let segments = membership
        .segments
        .iter()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
        .collect::<Vec<_>>();
    let [segment] = segments.as_slice() else {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_RELATION_MAINLINE_AMBIGUOUS",
            format!(
                "membership {} must have one cyclic relationMainline segment",
                membership.membership_id
            ),
        ));
    };
    let edge_map = graph
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let ordered = &segment.ordered_edge_ids;
    let (Some(first), Some(last)) = (ordered.first(), ordered.last()) else {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_RELATION_MAINLINE_NOT_CYCLIC",
            format!(
                "membership {} has an empty mainline",
                membership.membership_id
            ),
        ));
    };
    if ordered.windows(2).any(|window| {
        edge_map
            .get(window[0].as_str())
            .map(|edge| edge.to.as_str())
            != edge_map
                .get(window[1].as_str())
                .map(|edge| edge.from.as_str())
    }) || edge_map.get(last.as_str()).map(|edge| edge.to.as_str())
        != edge_map.get(first.as_str()).map(|edge| edge.from.as_str())
    {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_RELATION_MAINLINE_NOT_CYCLIC",
            format!(
                "membership {} is not a cyclic path",
                membership.membership_id
            ),
        ));
    }
    let start = ordered
        .iter()
        .position(|edge_id| edge_id == initial_edge_id)
        .ok_or_else(|| {
            PairDerivationError::new(
                "PAIR_DERIVATION_INITIAL_EDGE_NOT_IN_RELATION",
                format!(
                    "initial edge {initial_edge_id} is absent from {}",
                    membership.membership_id
                ),
            )
        })?;
    let mut lap = ordered[start..].to_vec();
    lap.extend_from_slice(&ordered[..start]);
    let lap_boundary_matches = lap.first().zip(lap.last()).is_some_and(|(first, last)| {
        edge_map.get(first.as_str()).map(|edge| edge.from.as_str()) == Some(anchor_node_id)
            && edge_map.get(last.as_str()).map(|edge| edge.to.as_str()) == Some(anchor_node_id)
    });
    if !lap_boundary_matches {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_LAP_BOUNDARY_MISMATCH",
            format!(
                "rotated lap for {} does not start and end at {anchor_node_id}",
                membership.membership_id
            ),
        ));
    }
    Ok(lap)
}

fn legacy_seed(
    adjacency: &BillingPairAdjacency,
    source: &str,
    source_date: &str,
) -> Result<BillingPairSeed, PairDerivationError> {
    let Some(BillingPairAdjacencyRoutePlan::SameNode { anchor_node_id, .. }) =
        adjacency.route_plan.as_ref()
    else {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_ROUTE_PLAN_KIND_MISMATCH",
            format!("{} does not have a sameNode route plan", adjacency.pair_id),
        ));
    };
    let anchor_osm_node_id = anchor_node_id
        .strip_prefix("n:")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            PairDerivationError::new(
                "PAIR_DERIVATION_ROUTE_PLAN_INVALID",
                format!(
                    "{} has invalid anchor node {anchor_node_id}",
                    adjacency.pair_id
                ),
            )
        })?;
    let entry_osm_way_id = adjacency.entry_osm_way_id.ok_or_else(|| {
        PairDerivationError::new(
            "PAIR_DERIVATION_ROUTE_PLAN_INVALID",
            format!("{} has no entry OSM way", adjacency.pair_id),
        )
    })?;
    let exit_osm_way_id = adjacency.exit_osm_way_id.ok_or_else(|| {
        PairDerivationError::new(
            "PAIR_DERIVATION_ROUTE_PLAN_INVALID",
            format!("{} has no exit OSM way", adjacency.pair_id),
        )
    })?;
    Ok(BillingPairSeed {
        id: adjacency.pair_id.clone(),
        entry_osm_way_id,
        entry_name: Some(adjacency.entry_name.clone()),
        exit_osm_way_id,
        exit_name: Some(adjacency.exit_name.clone()),
        anchor_osm_node_id,
        vehicle_profile: String::new(),
        assignment_id: adjacency.assignment_id.clone(),
        status: VerificationStatus::Unverified,
        one_section_ahead_verified: false,
        provenance: SeedProvenance {
            source: source.to_string(),
            source_date: source_date.to_string(),
            notes: None,
        },
        prices: Vec::<SeedPrice>::new(),
    })
}

fn radial_seed(
    adjacency: &BillingPairAdjacency,
    entry: &ResolvedEndpoint,
    exit: &ResolvedEndpoint,
    graph: &Graph,
    source: &str,
    source_date: &str,
) -> Result<crate::seed::RadialReturnBillingPairSeed, PairDerivationError> {
    let Some(BillingPairAdjacencyRoutePlan::DirectedJunction {
        entry_corridor,
        anchor,
        mandatory_lap,
        return_corridor,
    }) = adjacency.route_plan.as_ref()
    else {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_ROUTE_PLAN_KIND_MISMATCH",
            format!(
                "{} does not have a directedJunction route plan",
                adjacency.pair_id
            ),
        ));
    };
    let entry_endpoint = DiagnosticEndpoint {
        ramp_id: adjacency.entry_ramp_id.clone(),
        name: entry.report.name.clone(),
        support_state: entry.state,
        directed_segments: entry.directed_segments.clone(),
        binding_candidates: entry.binding_candidates.clone(),
    };
    let exit_endpoint = DiagnosticEndpoint {
        ramp_id: adjacency.exit_ramp_id.clone(),
        name: exit.report.name.clone(),
        support_state: exit.state,
        directed_segments: exit.directed_segments.clone(),
        binding_candidates: exit.binding_candidates.clone(),
    };
    let route_plan = DiagnosticRoutePlan {
        entry_corridor: EntryCorridor {
            membership_id: entry_corridor.membership_id.clone(),
            terminal_edge_id: entry_corridor.terminal_edge_id.clone(),
            merge_node_id: entry_corridor.merge_node_id.clone(),
        },
        anchor: DirectedJunctionAnchor {
            anchor_kind: AnchorKind::DirectedJunction,
            route_id: anchor.route_id.clone(),
            direction: anchor.direction.clone(),
            merge_node_id: anchor.merge_node_id.clone(),
            branch_node_id: anchor.branch_node_id.clone(),
            merge_terminal_edge_id: anchor.merge_terminal_edge_id.clone(),
            branch_initial_edge_id: anchor.branch_initial_edge_id.clone(),
            arc_policy: ArcPolicy::OrdinaryLongArc,
            excluded_short_connector: ExcludedShortConnector {
                from_node_id: anchor.excluded_short_connector.from_node_id.clone(),
                to_node_id: anchor.excluded_short_connector.to_node_id.clone(),
                osm_way_id: anchor.excluded_short_connector.osm_way_id,
                edge_count: anchor.excluded_short_connector.edge_count,
                distance_meters: anchor.excluded_short_connector.distance_meters,
            },
        },
        mandatory_lap: MandatoryLap {
            membership_id: mandatory_lap.membership_id.clone(),
            first_edge_id: mandatory_lap.first_edge_id.clone(),
            last_edge_id: mandatory_lap.last_edge_id.clone(),
            lap_count: mandatory_lap.lap_count,
        },
        return_corridor: ReturnCorridor {
            membership_id: return_corridor.membership_id.clone(),
            start_node_id: return_corridor.start_node_id.clone(),
            initial_edge_id: return_corridor.initial_edge_id.clone(),
            first_general_exit: FirstGeneralExit {
                rule: FirstGeneralExitRule::FirstGeneralExit,
                expected_ramp_id: adjacency.exit_ramp_id.clone(),
                exact_directed_binding: exit.state,
            },
        },
    };
    Ok(crate::seed::RadialReturnBillingPairSeed {
        id: adjacency.pair_id.clone(),
        pair_kind: PairKind::RadialReturn,
        route_plan_version: crate::seed::RoutePlanVersion::V1,
        vehicle_profile: graph.vehicle_profile.clone(),
        assignment_id: adjacency
            .assignment_id
            .clone()
            .unwrap_or_else(|| format!("assignment:{}", adjacency.pair_id)),
        entry_endpoint,
        exit_endpoint,
        route_plan,
        routing_capability: RoutingCapability::Routable,
        pair_eligibility: PairEligibility {
            status: PairEligibilityStatus::Unverified,
            one_section_ahead_verified: false,
        },
        loop_validation: LoopValidation {
            status: LoopValidationStatus::DeclaredRouteValidated,
        },
        tariff: crate::seed::DiagnosticTariff {
            status: TariffStatus::Unpriced,
            amount_yen: None,
            billing_distance_meters: None,
            prices: Vec::new(),
        },
        provenance: SeedProvenance {
            source: source.to_string(),
            source_date: source_date.to_string(),
            notes: None,
        },
    })
}

fn tariff_report(
    tariffs: &OdTariffsFile,
    pair_id: &str,
    entry_ramp_id: &str,
    exit_ramp_id: &str,
) -> (PairCandidateTariffReport, PairDerivationGate) {
    let matches = tariffs
        .assignments
        .iter()
        .filter(|assignment| {
            assignment
                .pair_ids
                .iter()
                .any(|candidate| candidate == pair_id)
                && assignment.entry_ramp_id == entry_ramp_id
                && assignment.exit_ramp_id == exit_ramp_id
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return (
            PairCandidateTariffReport {
                status: "unresolved".to_string(),
                assignment_id: None,
                billing_distance_meters: None,
                prices: Vec::new(),
            },
            gate(
                PairDerivationGateStatus::Failed,
                ["TARIFF_ASSIGNMENT_MISSING_OR_AMBIGUOUS"],
            ),
        );
    }
    let assignment = matches[0];
    let prices = assignment
        .prices
        .iter()
        .map(|price| PairCandidateTariffPriceReport {
            status: price.status.clone(),
            tariff_status: price.tariff_status.clone(),
            amount_yen: price.amount_yen,
            effective_from: price.effective_from.clone(),
            effective_to: price.effective_to.clone(),
            rule_id: price.rule_id.clone(),
            evidence_id: price.evidence_id.clone(),
            distance_evidence_id: price.distance_evidence_id.clone(),
        })
        .collect::<Vec<_>>();
    let has_priced_evidence = prices.iter().any(|price| price.status == "priced");
    let status = if has_priced_evidence {
        "priced"
    } else {
        "unpriced"
    };
    let gate = if has_priced_evidence {
        passed_gate()
    } else {
        gate(
            PairDerivationGateStatus::Unresolved,
            ["TARIFF_PRICED_EVIDENCE_UNRESOLVED"],
        )
    };
    (
        PairCandidateTariffReport {
            status: status.to_string(),
            assignment_id: Some(assignment.assignment_id.clone()),
            billing_distance_meters: Some(assignment.billing_distance_meters),
            prices,
        },
        gate,
    )
}

struct RouteDerivation {
    report: PairCandidateRoutePlanReport,
    route_gate: PairDerivationGate,
    direction_gate: PairDerivationGate,
    first_exit_gate: PairDerivationGate,
    mandatory_lap_gate: PairDerivationGate,
    membership_ids: Vec<String>,
}

fn route_plan_id(pair_id: &str) -> String {
    format!("route-plan:{pair_id}")
}

fn candidate_id(
    adjacency: &BillingPairAdjacency,
    route_plan: &PairCandidateRoutePlanReport,
) -> String {
    let identity = serde_json::json!({
        "pairId": adjacency.pair_id,
        "routeId": adjacency.route_id,
        "direction": adjacency.direction,
        "entryRampId": adjacency.entry_ramp_id,
        "exitRampId": adjacency.exit_ramp_id,
        "routePlanId": route_plan.route_plan_id,
        "edgeIdsSha256": route_plan.edge_ids_sha256,
    });
    let hash = compute_sha256(&serde_json::to_vec(&identity).unwrap_or_default());
    format!("candidate:{hash}")
}

fn first_exit_gate(
    result: Result<crate::validate::RelationConstrainedFirstExit, Vec<String>>,
) -> PairDerivationGate {
    match result {
        Err(codes) => gate(PairDerivationGateStatus::Failed, codes),
        Ok(result) => match result.status {
            RelationConstrainedFirstExitStatus::Verified => passed_gate(),
            RelationConstrainedFirstExitStatus::Unresolved => gate(
                PairDerivationGateStatus::Unresolved,
                ["FIRST_EXIT_UNRESOLVED"],
            ),
            RelationConstrainedFirstExitStatus::Unsupported => {
                gate(PairDerivationGateStatus::Failed, ["FIRST_EXIT_UNSUPPORTED"])
            }
        },
    }
}

fn source_segment_ids(roles: &[PairCandidateRouteRoleReport]) -> Vec<String> {
    let mut result = roles
        .iter()
        .flat_map(|role| role.source_segment_ids.iter().cloned())
        .collect::<Vec<_>>();
    result.sort();
    result.dedup();
    result
}

fn route_plan_report(
    pair_id: &str,
    edge_ids: &[String],
    roles: Vec<PairCandidateRouteRoleReport>,
    loop_validation_status: &str,
) -> PairCandidateRoutePlanReport {
    PairCandidateRoutePlanReport {
        route_plan_id: route_plan_id(pair_id),
        edge_ids_sha256: if edge_ids.is_empty() {
            None
        } else {
            ordered_edge_ids_sha256(edge_ids).ok()
        },
        loop_validation_status: loop_validation_status.to_string(),
        source_segment_ids: source_segment_ids(&roles),
        resolved_roles: roles,
    }
}

#[allow(clippy::too_many_arguments)]
fn derive_legacy_route(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    adjacency_file: &BillingPairAdjacencyFile,
    inventory: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
    support_decisions: &RampSupportDecisionsFile,
    adjacency: &BillingPairAdjacency,
    source: &str,
    source_date: &str,
    entry: &ResolvedEndpoint,
    exit: &ResolvedEndpoint,
) -> RouteDerivation {
    let unresolved_report = || RouteDerivation {
        report: route_plan_report(
            &adjacency.pair_id,
            &[],
            unresolved_route_roles(),
            "unresolved",
        ),
        route_gate: gate(
            PairDerivationGateStatus::Failed,
            ["LEGACY_ROUTE_RESOLUTION_FAILED"],
        ),
        direction_gate: gate(
            PairDerivationGateStatus::Unresolved,
            ["LEGACY_DIRECTION_UNRESOLVED"],
        ),
        first_exit_gate: gate(
            PairDerivationGateStatus::Unresolved,
            ["FIRST_EXIT_UNRESOLVED"],
        ),
        mandatory_lap_gate: gate(
            PairDerivationGateStatus::Unresolved,
            ["MANDATORY_LAP_UNRESOLVED"],
        ),
        membership_ids: vec![format!(
            "route:{}:{}",
            adjacency.route_id, adjacency.direction
        )],
    };
    let Some(BillingPairAdjacencyRoutePlan::SameNode {
        membership_id,
        anchor_node_id,
        first_exit_initial_edge_id,
        exit_approach_edge_ids,
    }) = adjacency.route_plan.as_ref()
    else {
        return unresolved_report();
    };
    let mut seed = match legacy_seed(adjacency, source, source_date) {
        Ok(seed) => seed,
        Err(_) => return unresolved_report(),
    };
    seed.vehicle_profile = graph.vehicle_profile.clone();
    let pair = match generate_billing_pair_for_relation_review(graph, &seed) {
        Ok(pair) => pair,
        Err(_) => return unresolved_report(),
    };
    let _wire = match shutoko_routing_core::LegacyRingBillingPair::from_legacy(
        &pair,
        graph,
        route_memberships,
    ) {
        Ok(wire) => wire,
        Err(_) => return unresolved_report(),
    };
    let membership = route_memberships
        .iter()
        .find(|membership| membership.membership_id == *membership_id);
    let direction_matches = membership.is_some_and(|membership| {
        membership.route_id == adjacency.route_id && membership.direction == adjacency.direction
    });
    let endpoint_edges_match = entry
        .edge_id
        .as_deref()
        .is_none_or(|edge_id| edge_id == pair.entry_id)
        && exit
            .edge_id
            .as_deref()
            .is_none_or(|edge_id| edge_id == pair.exit_id);
    let initial_edge_matches = pair.anchor_to_exit_edge_ids.first().map(String::as_str)
        == Some(first_exit_initial_edge_id.as_str());
    let mut route_reason_codes = Vec::new();
    if membership.is_none() {
        route_reason_codes.push("LEGACY_ROUTE_MEMBERSHIP_MISSING");
    }
    if !endpoint_edges_match {
        route_reason_codes.push("LEGACY_ENDPOINT_EDGE_MISMATCH");
    }
    if !initial_edge_matches {
        route_reason_codes.push("LEGACY_FIRST_EXIT_INITIAL_EDGE_MISMATCH");
    }
    let route_gate = if route_reason_codes.is_empty() {
        passed_gate()
    } else {
        gate(PairDerivationGateStatus::Failed, route_reason_codes)
    };
    let direction_gate = if direction_matches {
        passed_gate()
    } else {
        gate(
            PairDerivationGateStatus::Failed,
            ["LEGACY_ROUTE_DIRECTION_MISMATCH"],
        )
    };
    let Some(membership) = membership else {
        return RouteDerivation {
            report: route_plan_report(
                &adjacency.pair_id,
                &[],
                unresolved_route_roles(),
                "unresolved",
            ),
            route_gate,
            direction_gate,
            first_exit_gate: gate(
                PairDerivationGateStatus::Unresolved,
                ["FIRST_EXIT_UNRESOLVED"],
            ),
            mandatory_lap_gate: gate(
                PairDerivationGateStatus::Unresolved,
                ["MANDATORY_LAP_UNRESOLVED"],
            ),
            membership_ids: vec![membership_id.clone()],
        };
    };
    let lap = relation_mainline_lap(
        graph,
        membership,
        anchor_node_id,
        first_exit_initial_edge_id,
    );
    let (lap_gate, lap_edge_ids) = match &lap {
        Ok(edge_ids)
            if has_non_empty_shutoko_loop(graph, anchor_node_id)
                && edge_ids
                    .first()
                    .and_then(|edge_id| graph.edges.iter().find(|edge| edge.id == *edge_id))
                    .map(|edge| edge.from.as_str())
                    == Some(anchor_node_id.as_str()) =>
        {
            (passed_gate(), edge_ids.clone())
        }
        Ok(_) => (
            gate(
                PairDerivationGateStatus::Failed,
                ["MANDATORY_LAP_NOT_VALIDATED"],
            ),
            Vec::new(),
        ),
        Err(error) => (
            gate(PairDerivationGateStatus::Failed, [error.code]),
            Vec::new(),
        ),
    };
    let first_exit = validate_relation_constrained_legacy_first_exit(
        graph,
        route_memberships,
        inventory,
        bindings,
        support_decisions,
        adjacency_file,
        membership_id,
        anchor_node_id,
        first_exit_initial_edge_id,
        &adjacency.exit_ramp_id,
        exit_approach_edge_ids,
    );
    let first_exit_gate = first_exit_gate(first_exit.clone());
    let return_edge_ids = first_exit
        .as_ref()
        .map(|resolution| resolution.mainline_edge_ids.clone())
        .unwrap_or_default();
    let mut exit_edge_ids = if first_exit_gate.status == PairDerivationGateStatus::Passed {
        exit_approach_edge_ids.clone()
    } else {
        Vec::new()
    };
    exit_edge_ids.extend(pair.anchor_to_exit_edge_ids.last().cloned());
    let entry_status = if entry.report.gate.status == PairDerivationGateStatus::Passed {
        PairDerivationGateStatus::Passed
    } else {
        PairDerivationGateStatus::Unresolved
    };
    let exit_status = if exit.report.gate.status == PairDerivationGateStatus::Passed {
        PairDerivationGateStatus::Passed
    } else {
        PairDerivationGateStatus::Unresolved
    };
    let return_status = if first_exit_gate.status == PairDerivationGateStatus::Passed {
        PairDerivationGateStatus::Passed
    } else {
        PairDerivationGateStatus::Unresolved
    };
    let lap_status = if lap_gate.status == PairDerivationGateStatus::Passed {
        PairDerivationGateStatus::Passed
    } else {
        PairDerivationGateStatus::Unresolved
    };
    let roles = vec![
        route_role_report(
            route_memberships,
            PairDerivationRouteRole::EntryApproach,
            entry_status,
            Some(&pair.entry_to_anchor_edge_ids),
        ),
        route_role_report(
            route_memberships,
            PairDerivationRouteRole::MandatoryLap,
            lap_status,
            (!lap_edge_ids.is_empty()).then_some(lap_edge_ids.as_slice()),
        ),
        route_role_report(
            route_memberships,
            PairDerivationRouteRole::ReturnCorridor,
            return_status,
            Some(&return_edge_ids),
        ),
        route_role_report(
            route_memberships,
            PairDerivationRouteRole::ExitApproach,
            exit_status,
            Some(&exit_edge_ids),
        ),
    ];
    let mut full_edge_ids = pair.entry_to_anchor_edge_ids.clone();
    full_edge_ids.extend(lap_edge_ids);
    full_edge_ids.extend(return_edge_ids);
    full_edge_ids.extend(exit_edge_ids);
    let loop_status = if lap_gate.status == PairDerivationGateStatus::Passed {
        "declared_route_validated"
    } else {
        "unresolved"
    };
    RouteDerivation {
        report: route_plan_report(&adjacency.pair_id, &full_edge_ids, roles, loop_status),
        route_gate,
        direction_gate,
        first_exit_gate,
        mandatory_lap_gate: lap_gate,
        membership_ids: vec![membership_id.clone()],
    }
}

fn derive_radial_route(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    adjacency: &BillingPairAdjacency,
    source: &str,
    source_date: &str,
    entry: &ResolvedEndpoint,
    exit: &ResolvedEndpoint,
) -> Result<RouteDerivation, PairDerivationError> {
    let Some(BillingPairAdjacencyRoutePlan::DirectedJunction {
        entry_corridor,
        anchor,
        mandatory_lap,
        return_corridor,
    }) = adjacency.route_plan.as_ref()
    else {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_ROUTE_PLAN_KIND_MISMATCH",
            format!("{} has no directedJunction route plan", adjacency.pair_id),
        ));
    };
    let membership_ids = vec![
        entry_corridor.membership_id.clone(),
        mandatory_lap.membership_id.clone(),
        return_corridor.membership_id.clone(),
    ];
    let seed = radial_seed(adjacency, entry, exit, graph, source, source_date)?;
    let resolution = match resolve_diagnostic_radial_route_plan(graph, route_memberships, &seed) {
        Ok(resolution) => resolution,
        Err(_) => {
            return Ok(RouteDerivation {
                report: route_plan_report(
                    &adjacency.pair_id,
                    &[],
                    unresolved_route_roles(),
                    "unresolved",
                ),
                route_gate: gate(
                    PairDerivationGateStatus::Failed,
                    ["RADIAL_ROUTE_RESOLUTION_FAILED"],
                ),
                direction_gate: gate(
                    PairDerivationGateStatus::Unresolved,
                    ["RADIAL_DIRECTION_UNRESOLVED"],
                ),
                first_exit_gate: gate(
                    PairDerivationGateStatus::Unresolved,
                    ["FIRST_EXIT_UNRESOLVED"],
                ),
                mandatory_lap_gate: gate(
                    PairDerivationGateStatus::Unresolved,
                    ["MANDATORY_LAP_UNRESOLVED"],
                ),
                membership_ids,
            });
        }
    };
    let route_matches = anchor.route_id == adjacency.route_id
        && anchor.direction == adjacency.direction
        && mandatory_lap.membership_id
            == format!("route:{}:{}", adjacency.route_id, adjacency.direction);
    let mut route_gate = if route_matches {
        passed_gate()
    } else {
        gate(
            PairDerivationGateStatus::Failed,
            ["RADIAL_ROUTE_MEMBERSHIP_MISMATCH"],
        )
    };
    let direction_gate = if route_matches
        && resolution.lap.route_id == adjacency.route_id
        && resolution.lap.direction == adjacency.direction
    {
        passed_gate()
    } else {
        gate(
            PairDerivationGateStatus::Failed,
            ["RADIAL_ROUTE_DIRECTION_MISMATCH"],
        )
    };
    let first_exit_gate = if resolution.first_exit.exact_directed_binding == exit.state {
        match (exit.state, resolution.first_exit.exit.as_ref()) {
            (EndpointSupportState::VerifiedBound, Some(first_exit))
                if first_exit.ramp_id == adjacency.exit_ramp_id =>
            {
                passed_gate()
            }
            (EndpointSupportState::VerifiedBound, _) => {
                gate(PairDerivationGateStatus::Failed, ["FIRST_EXIT_UNRESOLVED"])
            }
            (EndpointSupportState::Unresolved, None) => gate(
                PairDerivationGateStatus::Unresolved,
                ["FIRST_EXIT_UNRESOLVED"],
            ),
            (EndpointSupportState::Unsupported, None) => {
                gate(PairDerivationGateStatus::Failed, ["FIRST_EXIT_UNSUPPORTED"])
            }
            _ => gate(
                PairDerivationGateStatus::Failed,
                ["FIRST_EXIT_STATE_CONFLICT"],
            ),
        }
    } else {
        gate(
            PairDerivationGateStatus::Failed,
            ["FIRST_EXIT_STATE_CONFLICT"],
        )
    };
    let mandatory_lap_gate = if resolution.lap.edge_ids.is_empty() {
        gate(
            PairDerivationGateStatus::Failed,
            ["MANDATORY_LAP_NOT_VALIDATED"],
        )
    } else {
        passed_gate()
    };
    let (promoted, promotion_error) = match radial_promotion(&first_exit_gate, || {
        promote_verified_radial_pair(graph, route_memberships, &seed, &resolution)
    }) {
        Ok(promoted) => (promoted, None),
        Err(error) => (None, Some(error.code())),
    };
    if let Some(error_code) = promotion_error {
        let mut reason_codes = route_gate.reason_codes.clone();
        reason_codes.push("ROUTE_PLAN_RESOLUTION_FAILED".to_string());
        reason_codes.push(error_code.to_string());
        route_gate = gate(PairDerivationGateStatus::Failed, reason_codes);
    }
    let (roles, full_edge_ids) = if let Some(promoted) = promoted {
        let roles = promoted
            .resolved_route_segments
            .iter()
            .map(|segment| PairCandidateRouteRoleReport {
                role: match segment.role {
                    shutoko_routing_core::RoutePlanSegmentRole::EntryApproach => {
                        PairDerivationRouteRole::EntryApproach
                    }
                    shutoko_routing_core::RoutePlanSegmentRole::MandatoryLap => {
                        PairDerivationRouteRole::MandatoryLap
                    }
                    shutoko_routing_core::RoutePlanSegmentRole::ReturnCorridor => {
                        PairDerivationRouteRole::ReturnCorridor
                    }
                    shutoko_routing_core::RoutePlanSegmentRole::ExitApproach => {
                        PairDerivationRouteRole::ExitApproach
                    }
                },
                status: PairDerivationGateStatus::Passed,
                edge_ids_sha256: Some(segment.edge_ids_sha256.clone()),
                source_segment_ids: segment.source_segment_ids.clone(),
            })
            .collect::<Vec<_>>();
        let full_edge_ids = promoted
            .resolved_route_segments
            .iter()
            .flat_map(|segment| segment.edge_ids.iter().cloned())
            .collect::<Vec<_>>();
        (roles, full_edge_ids)
    } else {
        let entry_edge_ids = entry
            .directed_segments
            .iter()
            .flat_map(|segment| segment.edge_ids.iter().cloned())
            .collect::<Vec<_>>();
        let exit_edge_ids = exit
            .directed_segments
            .iter()
            .flat_map(|segment| segment.edge_ids.iter().cloned())
            .collect::<Vec<_>>();
        let roles = vec![
            route_role_report(
                route_memberships,
                PairDerivationRouteRole::EntryApproach,
                PairDerivationGateStatus::Unresolved,
                (!entry_edge_ids.is_empty()).then_some(entry_edge_ids.as_slice()),
            ),
            route_role_report(
                route_memberships,
                PairDerivationRouteRole::MandatoryLap,
                PairDerivationGateStatus::Passed,
                Some(&resolution.lap.edge_ids),
            ),
            route_role_report(
                route_memberships,
                PairDerivationRouteRole::ReturnCorridor,
                if first_exit_gate.status == PairDerivationGateStatus::Passed {
                    PairDerivationGateStatus::Passed
                } else {
                    PairDerivationGateStatus::Unresolved
                },
                Some(&resolution.first_exit.mainline_edge_ids),
            ),
            route_role_report(
                route_memberships,
                PairDerivationRouteRole::ExitApproach,
                PairDerivationGateStatus::Unresolved,
                (!exit_edge_ids.is_empty()).then_some(exit_edge_ids.as_slice()),
            ),
        ];
        let mut full_edge_ids = entry_edge_ids;
        full_edge_ids.extend(resolution.lap.edge_ids.clone());
        full_edge_ids.extend(resolution.first_exit.mainline_edge_ids.clone());
        full_edge_ids.extend(exit_edge_ids);
        (roles, full_edge_ids)
    };
    let loop_status = if mandatory_lap_gate.status == PairDerivationGateStatus::Passed {
        "declared_route_validated"
    } else {
        "unresolved"
    };
    Ok(RouteDerivation {
        report: route_plan_report(&adjacency.pair_id, &full_edge_ids, roles, loop_status),
        route_gate,
        direction_gate,
        first_exit_gate,
        mandatory_lap_gate,
        membership_ids,
    })
}

fn legacy_seed_identity_gate(
    graph: &Graph,
    adjacency: &BillingPairAdjacency,
    inventory: &RampInventoryFile,
    billing_pair_seed: &ParsedBillingPairsSeed,
) -> PairDerivationGate {
    if adjacency.pair_kind == BillingPairAdjacencyKind::RadialReturn
        || (adjacency.review_status == BillingPairAdjacencyReviewStatus::Blocked
            && adjacency.route_plan.is_none())
    {
        return passed_gate();
    }
    let Some(seed) = billing_pair_seed
        .legacy_pairs()
        .into_iter()
        .find(|seed| seed.id == adjacency.pair_id)
    else {
        return gate(
            PairDerivationGateStatus::Failed,
            ["LEGACY_SEED_IDENTITY_MISSING"],
        );
    };
    let mut reason_codes = Vec::new();
    let route_prefix = format!(
        "bp:{}-{}:",
        adjacency.route_id.to_ascii_lowercase(),
        adjacency.direction
    );
    let ramp_prefix = format!(
        "ramp:{}-{}:",
        adjacency.route_id.to_ascii_lowercase(),
        adjacency.direction
    );
    if !adjacency.pair_id.starts_with(&route_prefix)
        || !adjacency.entry_ramp_id.starts_with(&ramp_prefix)
        || !adjacency.exit_ramp_id.starts_with(&ramp_prefix)
    {
        reason_codes.push("LEGACY_SEED_PAIR_ID_MISMATCH");
    }
    if seed.vehicle_profile != graph.vehicle_profile {
        reason_codes.push("LEGACY_SEED_VEHICLE_PROFILE_MISMATCH");
    }
    if Some(seed.entry_osm_way_id) != adjacency.entry_osm_way_id {
        reason_codes.push("LEGACY_SEED_ENTRY_OSM_WAY_MISMATCH");
    }
    if Some(seed.exit_osm_way_id) != adjacency.exit_osm_way_id {
        reason_codes.push("LEGACY_SEED_EXIT_OSM_WAY_MISMATCH");
    }
    if seed.entry_name.as_deref() != Some(adjacency.entry_name.as_str())
        || seed.exit_name.as_deref() != Some(adjacency.exit_name.as_str())
    {
        reason_codes.push("LEGACY_SEED_NAME_MISMATCH");
    }
    let entry_inventory = inventory
        .ramps
        .iter()
        .find(|ramp| ramp.ramp_id == adjacency.entry_ramp_id);
    let exit_inventory = inventory
        .ramps
        .iter()
        .find(|ramp| ramp.ramp_id == adjacency.exit_ramp_id);
    let entry_identity_matches = entry_inventory.is_some_and(|ramp| {
        ramp.kind == crate::model::RampKind::GeneralEntry
            && ramp.route == adjacency.route_id
            && ramp.direction == adjacency.direction
            && adjacency
                .entry_name
                .strip_suffix("入口")
                .is_some_and(|name| name == ramp.facility_name)
    });
    let exit_identity_matches = exit_inventory.is_some_and(|ramp| {
        ramp.kind == crate::model::RampKind::GeneralExit
            && ramp.route == adjacency.route_id
            && ramp.direction == adjacency.direction
            && adjacency
                .exit_name
                .strip_suffix("出口")
                .is_some_and(|name| name == ramp.facility_name)
    });
    if !entry_identity_matches || !exit_identity_matches {
        reason_codes.push("LEGACY_SEED_RAMP_IDENTITY_MISMATCH");
    }
    let anchor_matches = match adjacency.route_plan.as_ref() {
        Some(BillingPairAdjacencyRoutePlan::SameNode { anchor_node_id, .. }) => {
            seed.anchor_osm_node_id.to_string() == anchor_node_id.strip_prefix("n:").unwrap_or("")
        }
        _ => false,
    };
    if !anchor_matches {
        reason_codes.push("LEGACY_SEED_ROUTE_PLAN_MISMATCH");
    }
    if reason_codes.is_empty() {
        passed_gate()
    } else {
        gate(PairDerivationGateStatus::Failed, reason_codes)
    }
}

fn merge_failed_gate(target: &mut PairDerivationGate, source: PairDerivationGate) {
    if source.status == PairDerivationGateStatus::Failed {
        target.status = PairDerivationGateStatus::Failed;
        target.reason_codes.extend(source.reason_codes);
        target.reason_codes.sort();
        target.reason_codes.dedup();
    }
}

fn radial_promotion<T, E>(
    first_exit_gate: &PairDerivationGate,
    promote: impl FnOnce() -> Result<T, E>,
) -> Result<Option<T>, E> {
    if first_exit_gate.status == PairDerivationGateStatus::Passed {
        promote().map(Some)
    } else {
        Ok(None)
    }
}

#[allow(clippy::too_many_arguments)]
fn derive_candidate(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    adjacency_file: &BillingPairAdjacencyFile,
    adjacency: &BillingPairAdjacency,
    tariffs: &OdTariffsFile,
    inventory: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
    support_decisions: &RampSupportDecisionsFile,
    billing_pair_seed: &ParsedBillingPairsSeed,
    bound_evidence: &[BoundRampEvidence],
    source: &str,
    source_date: &str,
) -> Result<(PairCandidateReport, Vec<String>), PairDerivationError> {
    let entry = resolve_endpoint(
        graph,
        route_memberships,
        inventory,
        bindings,
        bound_evidence,
        &adjacency.entry_ramp_id,
        crate::model::RampKind::GeneralEntry,
        "entry",
    )?;
    let exit = resolve_endpoint(
        graph,
        route_memberships,
        inventory,
        bindings,
        bound_evidence,
        &adjacency.exit_ramp_id,
        crate::model::RampKind::GeneralExit,
        "exit",
    )?;
    let route = match adjacency.pair_kind {
        BillingPairAdjacencyKind::LegacyRing => derive_legacy_route(
            graph,
            route_memberships,
            adjacency_file,
            inventory,
            bindings,
            support_decisions,
            adjacency,
            source,
            source_date,
            &entry,
            &exit,
        ),
        BillingPairAdjacencyKind::RadialReturn => derive_radial_route(
            graph,
            route_memberships,
            adjacency,
            source,
            source_date,
            &entry,
            &exit,
        )?,
    };
    let route_roles_resolved = route.report.resolved_roles.len() == 4
        && route
            .report
            .resolved_roles
            .iter()
            .zip([
                PairDerivationRouteRole::EntryApproach,
                PairDerivationRouteRole::MandatoryLap,
                PairDerivationRouteRole::ReturnCorridor,
                PairDerivationRouteRole::ExitApproach,
            ])
            .all(|(role, expected)| {
                role.role == expected
                    && role.status == PairDerivationGateStatus::Passed
                    && role.edge_ids_sha256.is_some()
            });
    let mut official_gate = match adjacency.review_status {
        BillingPairAdjacencyReviewStatus::Reviewed => passed_gate(),
        BillingPairAdjacencyReviewStatus::Blocked => gate(
            PairDerivationGateStatus::Unresolved,
            adjacency.unresolved_reasons.clone(),
        ),
    };
    merge_failed_gate(
        &mut official_gate,
        legacy_seed_identity_gate(graph, adjacency, inventory, billing_pair_seed),
    );
    let (tariff, tariff_gate) = tariff_report(
        tariffs,
        &adjacency.pair_id,
        &adjacency.entry_ramp_id,
        &adjacency.exit_ramp_id,
    );
    let gates = PairDerivationGates {
        official_adjacency: official_gate,
        route: route.route_gate,
        direction: route.direction_gate,
        first_exit: route.first_exit_gate,
        mandatory_lap: route.mandatory_lap_gate,
        entry_binding: entry.report.gate.clone(),
        exit_binding: exit.report.gate.clone(),
        tariff_assignment: tariff_gate,
    };
    let all_gates_pass = [
        &gates.official_adjacency,
        &gates.route,
        &gates.direction,
        &gates.first_exit,
        &gates.mandatory_lap,
        &gates.entry_binding,
        &gates.exit_binding,
        &gates.tariff_assignment,
    ]
    .iter()
    .all(|gate| gate.status == PairDerivationGateStatus::Passed)
        && route_roles_resolved;
    let mut rejection_reasons = gates
        .official_adjacency
        .reason_codes
        .iter()
        .chain(gates.route.reason_codes.iter())
        .chain(gates.direction.reason_codes.iter())
        .chain(gates.first_exit.reason_codes.iter())
        .chain(gates.mandatory_lap.reason_codes.iter())
        .chain(gates.entry_binding.reason_codes.iter())
        .chain(gates.exit_binding.reason_codes.iter())
        .chain(gates.tariff_assignment.reason_codes.iter())
        .cloned()
        .collect::<Vec<_>>();
    if !route_roles_resolved {
        rejection_reasons.push("ROUTE_PLAN_ROLES_UNRESOLVED".to_string());
    }
    if gates.first_exit.status != PairDerivationGateStatus::Passed
        && !rejection_reasons
            .iter()
            .any(|reason| reason.starts_with("FIRST_EXIT_"))
    {
        rejection_reasons.push("FIRST_EXIT_UNRESOLVED".to_string());
    }
    if gates.entry_binding.status != PairDerivationGateStatus::Passed
        && !rejection_reasons
            .iter()
            .any(|reason| reason.starts_with("ENTRY_"))
    {
        rejection_reasons.push("ENTRY_BINDING_UNRESOLVED".to_string());
    }
    if gates.exit_binding.status != PairDerivationGateStatus::Passed
        && !rejection_reasons
            .iter()
            .any(|reason| reason.starts_with("EXIT_"))
    {
        rejection_reasons.push("EXIT_BINDING_UNRESOLVED".to_string());
    }
    rejection_reasons.sort();
    rejection_reasons.dedup();
    let candidate = PairCandidateReport {
        candidate_id: candidate_id(adjacency, &route.report),
        pair_id: adjacency.pair_id.clone(),
        pair_kind: adjacency.pair_kind,
        route_id: adjacency.route_id.clone(),
        direction: adjacency.direction.clone(),
        entry: entry.report,
        exit: exit.report,
        product_eligibility: PairCandidateProductEligibility {
            status: if all_gates_pass {
                PairDerivationProductEligibilityStatus::VerifiedOneSectionAhead
            } else {
                PairDerivationProductEligibilityStatus::Unverified
            },
            official_adjacency_evidence_id: adjacency.evidence_id.clone(),
            one_section_ahead_verified: all_gates_pass,
        },
        route_plan: route.report,
        gates,
        tariff,
        promotion_decision: if all_gates_pass {
            PairDerivationPromotionDecision::EligibleForReview
        } else {
            PairDerivationPromotionDecision::Hold
        },
        automatic_seed_write: false,
        rejection_reasons,
    };
    Ok((candidate, route.membership_ids))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn derive_pair_candidates(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    relation_coverage: &[RouteRelationCoverage],
    adjacency: &BillingPairAdjacencyFile,
    tariffs: &OdTariffsFile,
    inventory: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
    support_decisions: &RampSupportDecisionsFile,
    billing_pair_seed: &ParsedBillingPairsSeed,
    input_hashes: PairDerivationInputHashes,
) -> Result<PairDerivationReport, PairDerivationError> {
    let computed_route_membership_hash =
        route_memberships_sha256(route_memberships).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_MEMBERSHIP_INVALID", error.to_string())
        })?;
    if computed_route_membership_hash != input_hashes.route_membership_index_sha256 {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_INPUT_HASH_MISMATCH",
            "route membership index hash does not match the report input",
        ));
    }
    validate_billing_pair_adjacency(adjacency).map_err(|errors| {
        PairDerivationError::new("PAIR_DERIVATION_ADJACENCY_INVALID", errors.join("; "))
    })?;
    validate_od_tariffs(tariffs, inventory).map_err(|errors| {
        PairDerivationError::new("PAIR_DERIVATION_TARIFF_INVALID", errors.join("; "))
    })?;
    validate_route_membership_structure(
        graph,
        route_memberships,
        &input_hashes.osm_snapshot_sha256,
    )
    .map_err(|error| {
        PairDerivationError::new("PAIR_DERIVATION_MEMBERSHIP_INVALID", error.to_string())
    })?;
    let bound_evidence =
        bound_ramp_evidence_from_inventory(graph, inventory, bindings).map_err(|error| {
            PairDerivationError::new("PAIR_DERIVATION_BINDING_INVALID", error.to_string())
        })?;
    let legacy_c1_seed_count = billing_pair_seed
        .legacy_pairs()
        .into_iter()
        .filter(|seed| seed.id.starts_with("bp:c1-"))
        .count();
    if legacy_c1_seed_count != 8 {
        return Err(PairDerivationError::new(
            "PAIR_DERIVATION_LEGACY_C1_SEED_COUNT_MISMATCH",
            format!("expected 8 legacy C1 seed pairs, got {legacy_c1_seed_count}"),
        ));
    }
    let mut ordered = adjacency.pairs.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.pair_id.cmp(&right.pair_id));
    let mut candidates = Vec::with_capacity(ordered.len());
    let mut membership_candidates = BTreeMap::<String, Vec<(String, bool)>>::new();
    for pair in ordered {
        let (candidate, membership_ids) = derive_candidate(
            graph,
            route_memberships,
            adjacency,
            pair,
            tariffs,
            inventory,
            bindings,
            support_decisions,
            billing_pair_seed,
            &bound_evidence,
            &adjacency.source,
            &adjacency.source_date,
        )?;
        let route_resolved = candidate.gates.route.status == PairDerivationGateStatus::Passed;
        for membership_id in membership_ids {
            membership_candidates
                .entry(membership_id)
                .or_default()
                .push((candidate.pair_id.clone(), route_resolved));
        }
        candidates.push(candidate);
    }
    let candidate_pair_ids = candidates
        .iter()
        .map(|candidate| candidate.pair_id.as_str())
        .collect::<HashSet<_>>();
    for seed in billing_pair_seed.legacy_pairs() {
        if seed.id.starts_with("bp:c1-") && !candidate_pair_ids.contains(seed.id.as_str()) {
            return Err(PairDerivationError::new(
                "PAIR_DERIVATION_LEGACY_SEED_IDENTITY_MISSING",
                format!("legacy C1 seed {} has no derived candidate", seed.id),
            ));
        }
    }
    for membership_id in membership_candidates.keys() {
        if !route_memberships
            .iter()
            .any(|membership| &membership.membership_id == membership_id)
        {
            return Err(PairDerivationError::new(
                "PAIR_DERIVATION_MEMBERSHIP_INVALID",
                format!("candidate references unknown membership {membership_id}"),
            ));
        }
    }
    let mut relation_manifest = route_memberships
        .iter()
        .map(|membership| {
            let mut candidate_pairs = membership_candidates
                .get(&membership.membership_id)
                .cloned()
                .unwrap_or_default();
            candidate_pairs.sort();
            candidate_pairs.dedup();
            let mut relation_ids = membership
                .segments
                .iter()
                .filter_map(|segment| segment.source_relation_id.as_deref())
                .filter_map(|value| value.parse::<i64>().ok())
                .collect::<Vec<_>>();
            relation_ids.sort();
            relation_ids.dedup();
            let route_plan_unresolved = candidate_pairs
                .iter()
                .filter(|(_, resolved)| !*resolved)
                .count();
            let candidate_total = candidate_pairs.len();
            Ok(PairDerivationRelationManifest {
                membership_id: membership.membership_id.clone(),
                route_id: membership.route_id.clone(),
                direction: membership.direction.clone(),
                relation_ids,
                candidate_pair_ids: candidate_pairs
                    .into_iter()
                    .map(|(pair_id, _)| pair_id)
                    .collect(),
                route_plan_resolved: candidate_total - route_plan_unresolved,
                route_plan_unresolved,
                // A membership without any candidate pair has no unresolved route
                // plan, so it passes; the empty candidate list keeps that visible.
                status: if route_plan_unresolved == 0 {
                    "pass".to_string()
                } else {
                    "fail".to_string()
                },
            })
        })
        .collect::<Result<Vec<_>, PairDerivationError>>()?;
    relation_manifest.sort_by(|left, right| left.membership_id.cmp(&right.membership_id));
    let eligible_for_review = candidates
        .iter()
        .filter(|candidate| {
            candidate.promotion_decision == PairDerivationPromotionDecision::EligibleForReview
        })
        .count();
    let relation_failed = relation_coverage
        .iter()
        .filter(|relation| relation.status == RouteRelationCoverageStatus::Fail)
        .count();
    let summary = PairDerivationSummary {
        candidate_total: candidates.len(),
        eligible_for_review,
        hold: candidates.len() - eligible_for_review,
        relation_coverage: PairDerivationRelationCoverageSummary {
            relation_total: relation_coverage.len(),
            relation_expanded: relation_coverage.len() - relation_failed,
            relation_failed,
        },
    };
    Ok(PairDerivationReport {
        schema_version: PAIR_DERIVATION_REPORT_SCHEMA_VERSION,
        rule: PAIR_DERIVATION_RULE.to_string(),
        automatic_seed_write: false,
        input_hashes,
        relation_coverage: relation_coverage.to_vec(),
        relation_manifest,
        candidates,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_first_exit_propagates_radial_promotion_failure() {
        let error = radial_promotion(&passed_gate(), || Err::<(), _>("promotion failed"))
            .expect_err("promotion failure must not be discarded");
        assert_eq!(error, "promotion failed");
    }

    #[test]
    fn real_radial_seed_candidates_match_audited_binding_evidence() {
        let seed = crate::seed::parse_billing_pairs_seed(include_str!(
            "../../../data/billing-pairs-seed.json"
        ))
        .unwrap();
        let bindings: OsmRampBindingsFile =
            serde_json::from_str(include_str!("../../../data/osm-ramp-bindings.json")).unwrap();
        validate_radial_seed_binding_candidates(&seed, &bindings).unwrap();
    }
}
