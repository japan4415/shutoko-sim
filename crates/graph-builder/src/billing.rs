//! Billing pair generation via pathfinding and constraint validation.
//!
//! Maps human-verified seed definitions (`BillingPairSeed`) to resolved graph edges and
//! derives directional, continuous paths:
//! - `entryToAnchorEdgeIds`: Begins with `Entry` edge, traverses `Shutoko` to `anchorNodeId`.
//! - `anchorToExitEdgeIds`: Begins at `anchorNodeId`, traverses `Shutoko`, concludes with `Exit`.
//!
//! Ensures both path segments combine into a simple path (no hidden loops or cycle crossovers)
//! and satisfy all graph routing restrictions.

use crate::model::{BillingPair, Edge, EdgeKind, Graph, Price, VerificationStatus};
use crate::seed::{BillingPairSeed, BillingPairsSeedFile};
use crate::validate::{contains_forbidden_transition, validate_billing_pair, ValidationError};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashSet};

/// Error encountered during billing pair generation from a seed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BillingError {
    NoEntryEdgeForOsmWay(i64),
    NoExitEdgeForOsmWay(i64),
    AnchorNodeNotFound(String),
    NoPathEntryToAnchor(String),
    NoPathAnchorToExit(String),
    UnverifiedSectionMarkedAsVerified(String),
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
fn find_shutoko_path(
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
                    graph
                        .edges
                        .iter()
                        .find(|edge| &edge.id == id)
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
        .first()
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
        prices,
    };

    // 10. Run comprehensive validator
    validate_billing_pair(graph, &pair).map_err(BillingError::ValidationFailed)?;

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
