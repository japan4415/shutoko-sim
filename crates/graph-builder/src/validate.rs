//! Validation engine for billing pairs and graph consistency.
//!
//! Enforces issue #4 completion criteria:
//! - Continuity: from/to edge endpoints align strictly along paths.
//! - Simple path: concatenated direct path (entry -> anchor -> exit) visits no node twice.
//!   Hidden laps in billing pairs are strictly rejected.
//! - Edge kinds: entryToAnchor starts with Entry, followed by Shutoko, ending at anchorNodeId.
//!   anchorToExit starts at anchorNodeId, followed by Shutoko, ending with Exit.
//! - Loop existence: from anchorNodeId, at least one non-empty directed cycle (length >= 1)
//!   using only Shutoko edges and returning to anchorNodeId must exist without forbidden transitions.
//! - Forbidden transitions: direct baseline and loop paths must never traverse forbidden sequences.
//! - Prices: valid RFC 3339 UTC timestamps, non-overlapping half-open intervals, amounts > 0.
//! - Identifier lengths <= 256 bytes.
//! - No dynamic distance-based tariff estimation.

use crate::model::{BillingPair, Edge, EdgeKind, Graph};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

/// Structured validation error describing why a billing pair was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub pair_id: String,
    pub rule: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "validation failed for pair \"{}\" [rule: {}]: {}",
            self.pair_id, self.rule, self.message
        )
    }
}

impl std::error::Error for ValidationError {}

fn err(pair_id: &str, rule: &str, message: impl Into<String>) -> ValidationError {
    ValidationError {
        pair_id: pair_id.to_string(),
        rule: rule.to_string(),
        message: message.into(),
    }
}

/// Parse RFC 3339 UTC timestamp ending strictly in "Z".
pub fn parse_utc_timestamp(s: &str) -> Result<(), String> {
    if s.is_empty() || s.len() > 40 {
        return Err("timestamp empty or exceeds 40 characters".into());
    }
    if !s.ends_with('Z') {
        return Err("timestamp must end with UTC \"Z\" suffix".into());
    }
    // Validate basic RFC 3339 format
    let parts: Vec<&str> = s[..s.len() - 1].split('T').collect();
    if parts.len() != 2 {
        return Err("timestamp must contain date and time separated by \"T\"".into());
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() != 3 {
        return Err("date portion must be YYYY-MM-DD".into());
    }
    let y: u32 = date_parts[0].parse().map_err(|_| "invalid year")?;
    let m: u32 = date_parts[1].parse().map_err(|_| "invalid month")?;
    let d: u32 = date_parts[2].parse().map_err(|_| "invalid day")?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1970 {
        return Err("date out of plausible range".into());
    }
    let time_parts: Vec<&str> = parts[1].split(':').collect();
    if time_parts.len() != 3 {
        return Err("time portion must be HH:MM:SS".into());
    }
    let hh: u32 = time_parts[0].parse().map_err(|_| "invalid hour")?;
    let mm: u32 = time_parts[1].parse().map_err(|_| "invalid minute")?;
    let ss: f64 = time_parts[2].parse().map_err(|_| "invalid second")?;
    if hh > 23 || mm > 59 || ss < 0.0 || ss >= 60.0 {
        return Err("time values out of range".into());
    }
    Ok(())
}

/// Check if a sequence of edges violates any forbidden transition in the graph.
pub fn contains_forbidden_transition(
    edge_ids: &[String],
    forbidden_transitions: &[Vec<String>],
) -> bool {
    for seq in forbidden_transitions {
        if seq.len() <= edge_ids.len() && edge_ids.windows(seq.len()).any(|w| w == seq.as_slice()) {
            return true;
        }
    }
    false
}

/// Verifies that from `anchor_node_id`, there exists at least one non-empty directed cycle
/// (length >= 1) consisting solely of `Shutoko` edges that returns to `anchor_node_id`
/// without traversing any forbidden transitions.
pub fn has_non_empty_shutoko_loop(graph: &Graph, anchor_node_id: &str) -> bool {
    let mut outgoing: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
    for e in &graph.edges {
        if e.kind == EdgeKind::Shutoko {
            outgoing.entry(e.from.as_str()).or_default().push(e);
        }
    }

    // BFS searching for cycle back to anchor_node_id
    // State: (current_node, edge_ids_in_path)
    let mut queue: VecDeque<(&str, Vec<String>)> = VecDeque::new();
    if let Some(initial_edges) = outgoing.get(anchor_node_id) {
        for e in initial_edges {
            let path = vec![e.id.clone()];
            if !contains_forbidden_transition(&path, &graph.forbidden_transitions) {
                if e.to == anchor_node_id {
                    return true;
                }
                queue.push_back((e.to.as_str(), path));
            }
        }
    }

    let mut visited_states: HashSet<(&str, String)> = HashSet::new();

    while let Some((curr, path)) = queue.pop_front() {
        if path.len() > 2000 {
            continue;
        }

        let last_edge_id = path.last().cloned().unwrap_or_default();
        if !visited_states.insert((curr, last_edge_id)) {
            continue;
        }

        if let Some(next_edges) = outgoing.get(curr) {
            for e in next_edges {
                let mut next_path = path.clone();
                next_path.push(e.id.clone());

                if contains_forbidden_transition(&next_path, &graph.forbidden_transitions) {
                    continue;
                }

                if e.to == anchor_node_id {
                    return true;
                }

                // Avoid infinite loops that do not visit anchor
                let seen_count = next_path.iter().filter(|id| *id == &e.id).count();
                if seen_count > 1 {
                    continue;
                }

                queue.push_back((e.to.as_str(), next_path));
            }
        }
    }

    false
}

/// Validate a single BillingPair against a Graph according to all system requirements.
pub fn validate_billing_pair(graph: &Graph, pair: &BillingPair) -> Result<(), ValidationError> {
    // 1. Identity and length constraints
    if pair.id.is_empty() || pair.id.len() > 256 {
        return Err(err(
            &pair.id,
            "ID_BOUNDS",
            "billing pair id must be between 1 and 256 bytes",
        ));
    }
    if pair.entry_id.is_empty() || pair.entry_id.len() > 256 {
        return Err(err(
            &pair.id,
            "ID_BOUNDS",
            "entry_id must be between 1 and 256 bytes",
        ));
    }
    if pair.exit_id.is_empty() || pair.exit_id.len() > 256 {
        return Err(err(
            &pair.id,
            "ID_BOUNDS",
            "exit_id must be between 1 and 256 bytes",
        ));
    }
    if pair.anchor_node_id.is_empty() || pair.anchor_node_id.len() > 256 {
        return Err(err(
            &pair.id,
            "ID_BOUNDS",
            "anchor_node_id must be between 1 and 256 bytes",
        ));
    }
    if pair.vehicle_profile.is_empty() || pair.vehicle_profile.len() > 256 {
        return Err(err(
            &pair.id,
            "ID_BOUNDS",
            "vehicle_profile must be between 1 and 256 bytes",
        ));
    }
    if pair.vehicle_profile != graph.vehicle_profile {
        return Err(err(
            &pair.id,
            "VEHICLE_PROFILE_MISMATCH",
            format!(
                "pair vehicle profile \"{}\" does not match graph \"{}\"",
                pair.vehicle_profile, graph.vehicle_profile
            ),
        ));
    }

    // 2. Anchor node existence
    if !graph.nodes.iter().any(|n| n.id == pair.anchor_node_id) {
        return Err(err(
            &pair.id,
            "ANCHOR_NOT_FOUND",
            format!(
                "anchor node \"{}\" does not exist in graph",
                pair.anchor_node_id
            ),
        ));
    }

    // 3. Resolve edges map
    let edge_map: BTreeMap<&str, &Edge> = graph.edges.iter().map(|e| (e.id.as_str(), e)).collect();

    // 4. Resolve pre path (entry_to_anchor)
    if pair.entry_to_anchor_edge_ids.is_empty() {
        return Err(err(
            &pair.id,
            "EMPTY_PATH",
            "entry_to_anchor_edge_ids is empty",
        ));
    }
    let mut pre_edges: Vec<&Edge> = Vec::with_capacity(pair.entry_to_anchor_edge_ids.len());
    for id in &pair.entry_to_anchor_edge_ids {
        match edge_map.get(id.as_str()) {
            Some(e) => pre_edges.push(e),
            None => {
                return Err(err(
                    &pair.id,
                    "EDGE_NOT_FOUND",
                    format!("edge \"{}\" in entry_to_anchor not found in graph", id),
                ))
            }
        }
    }

    // 5. Resolve post path (anchor_to_exit)
    if pair.anchor_to_exit_edge_ids.is_empty() {
        return Err(err(
            &pair.id,
            "EMPTY_PATH",
            "anchor_to_exit_edge_ids is empty",
        ));
    }
    let mut post_edges: Vec<&Edge> = Vec::with_capacity(pair.anchor_to_exit_edge_ids.len());
    for id in &pair.anchor_to_exit_edge_ids {
        match edge_map.get(id.as_str()) {
            Some(e) => post_edges.push(e),
            None => {
                return Err(err(
                    &pair.id,
                    "EDGE_NOT_FOUND",
                    format!("edge \"{}\" in anchor_to_exit not found in graph", id),
                ))
            }
        }
    }

    // 6. Check edge kinds and endpoints
    if pre_edges[0].id != pair.entry_id {
        return Err(err(
            &pair.id,
            "ENTRY_EDGE_MISMATCH",
            format!(
                "entry_to_anchor first edge \"{}\" does not match entry_id \"{}\"",
                pre_edges[0].id, pair.entry_id
            ),
        ));
    }
    if pre_edges[0].kind != EdgeKind::Entry {
        return Err(err(
            &pair.id,
            "INVALID_EDGE_KIND",
            format!(
                "first edge \"{}\" must have kind Entry, got {:?}",
                pre_edges[0].id, pre_edges[0].kind
            ),
        ));
    }
    for e in &pre_edges[1..] {
        if e.kind != EdgeKind::Shutoko {
            return Err(err(
                &pair.id,
                "INVALID_EDGE_KIND",
                format!(
                    "intermediate edge \"{}\" in entry_to_anchor must have kind Shutoko, got {:?}",
                    e.id, e.kind
                ),
            ));
        }
    }
    if pre_edges.last().unwrap().to != pair.anchor_node_id {
        return Err(err(
            &pair.id,
            "ANCHOR_CONNECTION_MISMATCH",
            format!(
                "entry_to_anchor final node \"{}\" does not match anchor_node_id \"{}\"",
                pre_edges.last().unwrap().to,
                pair.anchor_node_id
            ),
        ));
    }

    if post_edges[0].from != pair.anchor_node_id {
        return Err(err(
            &pair.id,
            "ANCHOR_CONNECTION_MISMATCH",
            format!(
                "anchor_to_exit first node \"{}\" does not match anchor_node_id \"{}\"",
                post_edges[0].from, pair.anchor_node_id
            ),
        ));
    }
    for e in &post_edges[..post_edges.len() - 1] {
        if e.kind != EdgeKind::Shutoko {
            return Err(err(
                &pair.id,
                "INVALID_EDGE_KIND",
                format!(
                    "intermediate edge \"{}\" in anchor_to_exit must have kind Shutoko, got {:?}",
                    e.id, e.kind
                ),
            ));
        }
    }
    let exit_edge = post_edges.last().unwrap();
    if exit_edge.id != pair.exit_id {
        return Err(err(
            &pair.id,
            "EXIT_EDGE_MISMATCH",
            format!(
                "anchor_to_exit last edge \"{}\" does not match exit_id \"{}\"",
                exit_edge.id, pair.exit_id
            ),
        ));
    }
    if exit_edge.kind != EdgeKind::Exit {
        return Err(err(
            &pair.id,
            "INVALID_EDGE_KIND",
            format!(
                "last edge \"{}\" must have kind Exit, got {:?}",
                exit_edge.id, exit_edge.kind
            ),
        ));
    }

    // 7. Continuity check
    for w in pre_edges.windows(2) {
        if w[0].to != w[1].from {
            return Err(err(
                &pair.id,
                "DISCONNECTED_PATH",
                format!(
                    "entry_to_anchor disconnected between \"{}\" (to: {}) and \"{}\" (from: {})",
                    w[0].id, w[0].to, w[1].id, w[1].from
                ),
            ));
        }
    }
    for w in post_edges.windows(2) {
        if w[0].to != w[1].from {
            return Err(err(
                &pair.id,
                "DISCONNECTED_PATH",
                format!(
                    "anchor_to_exit disconnected between \"{}\" (to: {}) and \"{}\" (from: {})",
                    w[0].id, w[0].to, w[1].id, w[1].from
                ),
            ));
        }
    }

    // 8. Simple path / No hidden lap check
    // The direct baseline entry -> anchor -> exit must be a strictly simple path.
    // Concatenated visited nodes sequence: pre[0].from, pre[0].to, ..., pre.last().to (= anchor),
    // post[0].to, ..., post.last().to.
    let mut visited_nodes = BTreeSet::new();
    visited_nodes.insert(pre_edges[0].from.as_str());
    for e in pre_edges.iter().chain(&post_edges) {
        if !visited_nodes.insert(e.to.as_str()) {
            return Err(err(
                &pair.id,
                "HIDDEN_LOOP_OR_CYCLE",
                format!(
                    "billing pair direct path must be a simple path: node \"{}\" visited more than once",
                    e.to
                ),
            ));
        }
    }

    // 9. Forbidden transitions check on direct path
    let all_direct_edge_ids: Vec<String> = pair
        .entry_to_anchor_edge_ids
        .iter()
        .chain(&pair.anchor_to_exit_edge_ids)
        .cloned()
        .collect();
    if contains_forbidden_transition(&all_direct_edge_ids, &graph.forbidden_transitions) {
        return Err(err(
            &pair.id,
            "FORBIDDEN_TRANSITION",
            "direct entry-to-exit path contains a forbidden transition sequence",
        ));
    }

    // 10. Non-empty Shutoko loop existence from anchor
    if !has_non_empty_shutoko_loop(graph, &pair.anchor_node_id) {
        return Err(err(
            &pair.id,
            "NO_SHUTOKO_LOOP",
            format!(
                "no valid non-empty Shutoko loop found from anchor node \"{}\"",
                pair.anchor_node_id
            ),
        ));
    }

    // 11. Price intervals validity
    if pair.prices.len() > 1000 {
        return Err(err(&pair.id, "PRICE_LIMIT", "exceeds 1000 price intervals"));
    }
    let mut parsed_intervals: Vec<(&str, Option<&str>)> = Vec::with_capacity(pair.prices.len());
    for price in &pair.prices {
        if price.amount_yen == 0 {
            return Err(err(
                &pair.id,
                "INVALID_PRICE_AMOUNT",
                "toll amount must be > 0 yen",
            ));
        }
        if let Err(e) = parse_utc_timestamp(&price.effective_from) {
            return Err(err(
                &pair.id,
                "INVALID_PRICE_INTERVAL",
                format!("invalid effective_from \"{}\": {}", price.effective_from, e),
            ));
        }
        if let Some(to) = &price.effective_to {
            if let Err(e) = parse_utc_timestamp(to) {
                return Err(err(
                    &pair.id,
                    "INVALID_PRICE_INTERVAL",
                    format!("invalid effective_to \"{}\": {}", to, e),
                ));
            }
            if to.as_str() <= price.effective_from.as_str() {
                return Err(err(
                    &pair.id,
                    "INVALID_PRICE_INTERVAL",
                    format!(
                        "effective_to ({}) must be > effective_from ({})",
                        to, price.effective_from
                    ),
                ));
            }
        }
        parsed_intervals.push((price.effective_from.as_str(), price.effective_to.as_deref()));
    }

    // Check non-overlapping intervals
    parsed_intervals.sort_by_key(|p| p.0);
    for pair_window in parsed_intervals.windows(2) {
        let (first_from, first_to) = pair_window[0];
        let (second_from, _) = pair_window[1];
        match first_to {
            None => {
                return Err(err(
                    &pair.id,
                    "OVERLAPPING_PRICES",
                    format!(
                        "interval starting at \"{}\" has no end date but another interval starts at \"{}\"",
                        first_from, second_from
                    ),
                ));
            }
            Some(end) if end > second_from => {
                return Err(err(
                    &pair.id,
                    "OVERLAPPING_PRICES",
                    format!(
                        "overlapping intervals: [{}, {}) overlaps with start {}",
                        first_from, end, second_from
                    ),
                ));
            }
            _ => {}
        }
    }

    Ok(())
}
