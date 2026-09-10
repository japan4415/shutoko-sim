//! Validation engine for billing pairs and graph consistency.
//!
//! Enforces issue #4 completion criteria:
//! - Continuity: from/to edge endpoints align strictly along paths.
//! - Simple path: concatenated direct path (entry -> anchor -> exit) visits no node twice.
//!   Hidden laps in billing pairs are strictly rejected.
//! - Edge kinds: entryToAnchor starts with Entry, followed by Shutoko, ending at anchorNodeId.
//!   anchorToExit starts at anchorNodeId, followed by Shutoko, ending with Exit.
//! - Loop existence: from anchorNodeId, at least one non-empty directed closed walk
//!   (length >= 1, intermediate nodes may be revisited) using only Shutoko edges and
//!   returning to anchorNodeId must exist without forbidden transitions.
//! - Forbidden transitions: direct baseline and loop paths must never traverse forbidden sequences.
//! - Prices: valid RFC 3339 UTC timestamps, non-overlapping half-open intervals, amounts > 0.
//! - Identifier lengths <= 256 bytes.
//! - No dynamic distance-based tariff estimation.

use crate::model::{BillingPair, Edge, EdgeKind, Graph, VerificationStatus};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime};

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
pub fn parse_utc_timestamp(s: &str) -> Result<OffsetDateTime, String> {
    if s.is_empty() || s.len() > 40 {
        return Err("timestamp empty or exceeds 40 characters".into());
    }
    if !s.ends_with('Z') {
        return Err("timestamp must end with UTC \"Z\" suffix".into());
    }
    OffsetDateTime::parse(s, &Rfc3339).map_err(|e| format!("invalid RFC 3339 timestamp: {}", e))
}

/// Validate that a string is a valid ISO 8601 calendar date (YYYY-MM-DD) representing an actual calendar date.
pub fn parse_iso_date(s: &str) -> Result<Date, String> {
    if s.len() != 10 {
        return Err("date must be 10 characters in YYYY-MM-DD format".into());
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err("date must be separated by hyphens (YYYY-MM-DD)".into());
    }
    let y: i32 = parts[0].parse().map_err(|_| "invalid year")?;
    let m_num: u8 = parts[1].parse().map_err(|_| "invalid month")?;
    let d: u8 = parts[2].parse().map_err(|_| "invalid day")?;
    if !(1..=12).contains(&m_num) {
        return Err("month out of range 1..=12".into());
    }
    let m = Month::try_from(m_num).map_err(|_| "invalid month")?;
    Date::from_calendar_date(y, m, d).map_err(|e| format!("invalid calendar date: {}", e))
}

/// Validate that a string conforms to HTTP/HTTPS URL format without whitespace.
///
/// Ensures valid scheme (http:// or https://), non-empty hostname with at least
/// two labels, valid domain label characters (alphanumeric and hyphen, no leading/trailing
/// hyphen or empty labels), no leading/trailing dot or consecutive dots, and optional valid port.
pub fn validate_url(s: &str) -> Result<(), String> {
    if s.is_empty() || s.len() > 2048 {
        return Err("source URL empty or exceeds 2048 characters".into());
    }
    if s.contains(' ') {
        return Err("source URL must not contain whitespace".into());
    }
    if !(s.starts_with("https://") || s.starts_with("http://")) {
        return Err("source URL must start with http:// or https://".into());
    }
    let rest = if let Some(stripped) = s.strip_prefix("https://") {
        stripped
    } else {
        s.strip_prefix("http://").unwrap()
    };
    let host_and_port = rest.split('/').next().unwrap_or("");
    if host_and_port.is_empty() {
        return Err("source URL must contain a non-empty host".into());
    }
    let (host, port_opt) = if let Some((h, p)) = host_and_port.split_once(':') {
        (h, Some(p))
    } else {
        (host_and_port, None)
    };
    if let Some(port_str) = port_opt {
        if port_str.is_empty() || port_str.parse::<u16>().is_err() {
            return Err("source URL contains invalid port number".into());
        }
    }
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return Err("source URL must contain a valid domain name without leading, trailing, or consecutive dots".into());
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return Err("source URL must contain a domain with at least two labels".into());
    }
    for label in &labels {
        if label.is_empty() || label.len() > 63 {
            return Err("source URL domain label must be between 1 and 63 characters".into());
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("source URL domain label must not start or end with a hyphen".into());
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("source URL domain label contains invalid characters".into());
        }
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

/// Dijkstra state for finding the first reachable exit edge from anchor.
/// Each state carries its own visited-node set so that the simple-path
/// constraint is part of the state identity (see the soundness rationale on
/// `find_first_exits_from_anchor`).
#[derive(Clone, Eq, PartialEq)]
struct ExitSearchState<'a> {
    cost: u64,
    node: &'a str,
    path: Vec<String>,
    visited: BTreeSet<&'a str>,
}

impl<'a> Ord for ExitSearchState<'a> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.path.len().cmp(&self.path.len()))
            .then_with(|| other.path.cmp(&self.path))
    }
}

impl<'a> PartialOrd for ExitSearchState<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Explicit budget on states popped from the priority queue for the simple-path
/// first-exit search. The state space (node x suffix x visited-set) is finite,
/// but enumeration of simple paths is worst-case exponential, so the search
/// stops with a reported error once this many states are popped from the queue
/// (never a silent truncation).
pub const FIRST_EXIT_STATE_BUDGET: usize = 200_000;

/// Finds the first Exit edge(s) reachable from `anchor_node_id` using only `Shutoko`
/// edges, respecting forbidden transitions.
/// Returns `(min_distance_meters, exit_edge_ids)`.
///
/// # Semantics: simple paths with a visited-set state key (issue #6 item 2)
///
/// The search enumerates **simple paths** (a node is visited at most once per path).
/// Each Dijkstra state carries its own visited-node set, and the pruning key is
/// `(node, suffix, visited_nodes)` where `suffix` is the last `L - 1` traversed edge
/// IDs (`L` = maximum length among `graph.forbidden_transitions`, same definition as
/// in `has_non_empty_shutoko_loop`).
///
/// ## Soundness rationale
///
/// 1. **Exact state equivalence**: from a given state, the set of legal future
///    continuations is fully determined by (a) the current node, (b) the suffix
///    (windows of a forbidden-transition match that end at the newest edge depend
///    only on the suffix plus the new edge; earlier windows were already checked),
///    and (c) the visited-node set (which decides the simple-path constraint).
///    Two paths arriving at the identical `(node, suffix, visited_nodes)` triple
///    therefore have strictly identical future feasibility and identical additive
///    cost structure, so keeping only the cheapest arrival per key is sound.
///    Counterexamples A/B/C of PR #5 do not arise here because their pruned paths
///    differ in the visited set and thus receive distinct keys.
/// 2. **Termination**: the key space is finite (finitely many nodes, suffix
///    sequences, and visited subsets), and each key is re-pushed only on a
///    strictly smaller cost. A `cost > min_exit_dist` break further truncates the
///    search once the first exit distance is fixed.
/// 3. **Explicit budget**: simple-path enumeration is worst-case exponential, so
///    the search also stops after popping `FIRST_EXIT_STATE_BUDGET` states from
///    the queue and returns `Err` (surfaced as `FIRST_EXIT_SEARCH_FAILED`). The
///    error message records the number of popped states; there is no silent
///    truncation.
///
/// ## Why not plain walk semantics
///
/// Under pure walk semantics (node revisits allowed), a shortest walk can loop back
/// through a node to dodge a forbidden transition and report an earlier exit than
/// the simple-path one (issue #6 item 2: a 4m looping walk vs. the 11m simple-path
/// exit). This contradicts the driver intuition that the exit reached after a full
/// lap is not the "one section ahead" exit, and can reject a correct simple-path
/// billing pair with `FIRST_EXIT_MISMATCH`. The direct path of any verified billing
/// pair is itself a strictly simple path (rule 8 of `validate_billing_pair`), so a
/// simple-path definition of "first exit" is the consistent choice.
pub fn find_first_exits_from_anchor(
    graph: &Graph,
    anchor_node_id: &str,
) -> Result<(u64, Vec<String>), String> {
    find_first_exits_from_anchor_with_budget(graph, anchor_node_id, FIRST_EXIT_STATE_BUDGET)
}

/// Same as [`find_first_exits_from_anchor`] with an explicit search budget on
/// popped states, exposed so tests can pin the budget-exceeded `Err` behaviour
/// with a small constant.
pub fn find_first_exits_from_anchor_with_budget(
    graph: &Graph,
    anchor_node_id: &str,
    state_budget: usize,
) -> Result<(u64, Vec<String>), String> {
    let mut shutoko_outgoing: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
    let mut exit_outgoing: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();

    for e in &graph.edges {
        match e.kind {
            EdgeKind::Shutoko => shutoko_outgoing.entry(e.from.as_str()).or_default().push(e),
            EdgeKind::Exit => exit_outgoing.entry(e.from.as_str()).or_default().push(e),
            _ => {}
        }
    }

    let max_forbidden_len = graph
        .forbidden_transitions
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0);
    let history_len = max_forbidden_len.saturating_sub(1);

    let mut heap = std::collections::BinaryHeap::new();
    let mut start_visited: BTreeSet<&str> = BTreeSet::new();
    start_visited.insert(anchor_node_id);
    heap.push(ExitSearchState {
        cost: 0,
        node: anchor_node_id,
        path: Vec::new(),
        visited: start_visited,
    });

    let mut best_cost_by_state: BTreeMap<(&str, Vec<String>, BTreeSet<&str>), u64> =
        BTreeMap::new();
    let mut min_exit_dist: Option<u64> = None;
    let mut first_exit_ids: Vec<String> = Vec::new();
    let mut popped: usize = 0;

    while let Some(ExitSearchState {
        cost,
        node,
        path,
        visited,
    }) = heap.pop()
    {
        if let Some(min_dist) = min_exit_dist {
            if cost > min_dist {
                break;
            }
        }

        if popped >= state_budget {
            return Err(format!(
                "first exit search from anchor node \"{}\": 探索予算超過, popped {} states (budget {})",
                anchor_node_id, popped, state_budget
            ));
        }
        popped += 1;

        let suffix: Vec<String> = if history_len > 0 {
            let start = path.len().saturating_sub(history_len);
            path[start..].to_vec()
        } else {
            Vec::new()
        };

        if let Some(&best) = best_cost_by_state.get(&(node, suffix.clone(), visited.clone())) {
            if cost > best {
                continue;
            }
        }

        // Check if there are Exit edges from this node
        if let Some(exits) = exit_outgoing.get(node) {
            for exit_e in exits {
                let mut candidate_path = path.clone();
                candidate_path.push(exit_e.id.clone());
                if contains_forbidden_transition(&candidate_path, &graph.forbidden_transitions) {
                    continue;
                }
                let total_dist = cost + exit_e.distance_meters;
                match min_exit_dist {
                    None => {
                        min_exit_dist = Some(total_dist);
                        first_exit_ids.push(exit_e.id.clone());
                    }
                    Some(cur_min) if total_dist < cur_min => {
                        min_exit_dist = Some(total_dist);
                        first_exit_ids.clear();
                        first_exit_ids.push(exit_e.id.clone());
                    }
                    Some(cur_min) if total_dist == cur_min => {
                        first_exit_ids.push(exit_e.id.clone());
                    }
                    _ => {}
                }
            }
        }

        // Expand Shutoko outgoing edges under the simple-path constraint:
        // a next node already in the visited set is not revisited.
        if let Some(next_edges) = shutoko_outgoing.get(node) {
            for e in next_edges {
                let next_node = e.to.as_str();

                let mut candidate_path = path.clone();
                candidate_path.push(e.id.clone());
                if contains_forbidden_transition(&candidate_path, &graph.forbidden_transitions) {
                    continue;
                }

                let next_cost = cost.saturating_add(e.distance_meters);
                if let Some(min_dist) = min_exit_dist {
                    if next_cost >= min_dist {
                        continue;
                    }
                }

                let mut next_visited = visited.clone();
                if !next_visited.insert(next_node) {
                    // Simple path: the node was already visited on this path.
                    continue;
                }

                let next_suffix: Vec<String> = if history_len > 0 {
                    let start = candidate_path.len().saturating_sub(history_len);
                    candidate_path[start..].to_vec()
                } else {
                    Vec::new()
                };

                let visited_for_state = next_visited.clone();
                let state_key = (next_node, next_suffix, next_visited);
                if next_cost < *best_cost_by_state.get(&state_key).unwrap_or(&u64::MAX) {
                    best_cost_by_state.insert(state_key, next_cost);
                    heap.push(ExitSearchState {
                        cost: next_cost,
                        node: next_node,
                        path: candidate_path,
                        visited: visited_for_state,
                    });
                }
            }
        }
    }

    if let Some(min_dist) = min_exit_dist {
        first_exit_ids.sort();
        first_exit_ids.dedup();
        Ok((min_dist, first_exit_ids))
    } else {
        Err(format!(
            "no exit edge reachable from anchor node \"{}\"",
            anchor_node_id
        ))
    }
}

/// Verifies that from `anchor_node_id`, there exists at least one non-empty directed
/// closed walk (length >= 1) consisting solely of `Shutoko` edges that returns to
/// `anchor_node_id` without traversing any forbidden transitions.
///
/// # Semantics: walk over a finite `(node, suffix)` state space (issue #6 item 1/3)
///
/// The judgment is the existence of a **closed walk** from the anchor back to
/// itself, not the existence of a simple cycle; intermediate nodes may be
/// revisited. The search is a BFS over states `(node, suffix)` where `suffix` is
/// the last `L - 1` traversed edge IDs (`L` = maximum length among
/// `graph.forbidden_transitions`).
///
/// ## Soundness rationale
///
/// 1. **Sound pruning**: whether a continuation is legal depends only on the
///    current node and the last `L - 1` edges. Windows of a forbidden-transition
///    match that lie entirely inside an already-checked prefix cannot change;
///    every future window ends at a newly appended edge and involves only the
///    suffix plus that edge. Hence two paths that reach the same `(node, suffix)`
///    have strictly identical sets of legal continuations, and skipping a later
///    arrival at an already-seen `(node, suffix)` is provably sound. (The former
///    per-node "no revisit" prohibition was unsound against this key: a cheap
///    early arrival could exhaust a node for a later, legitimate closed walk —
///    see the issue #6 item 1 regression test.)
/// 2. **Termination**: the state space `node x suffix` is finite (finitely many
///    nodes and finitely many edge-ID sequences of length <= L - 1), and each
///    state is enqueued at most once, so the BFS always terminates. The former
///    silent `path.len() > 2000` cutoff is therefore unnecessary and removed.
/// 3. **Reachability only**: this is an existence check, so distances are not
///    tracked.
pub fn has_non_empty_shutoko_loop(graph: &Graph, anchor_node_id: &str) -> bool {
    let mut outgoing: BTreeMap<&str, Vec<&Edge>> = BTreeMap::new();
    for e in &graph.edges {
        if e.kind == EdgeKind::Shutoko {
            outgoing.entry(e.from.as_str()).or_default().push(e);
        }
    }

    let max_forbidden_len = graph
        .forbidden_transitions
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0);
    let history_len = max_forbidden_len.saturating_sub(1);

    let mut queue: VecDeque<(&str, Vec<String>)> = VecDeque::new();
    let mut visited_states: HashSet<(&str, Vec<String>)> = HashSet::new();

    if let Some(initial_edges) = outgoing.get(anchor_node_id) {
        for e in initial_edges {
            let path = vec![e.id.clone()];
            if contains_forbidden_transition(&path, &graph.forbidden_transitions) {
                continue;
            }
            if e.to == anchor_node_id {
                return true;
            }
            let suffix: Vec<String> = if history_len > 0 {
                let start = path.len().saturating_sub(history_len);
                path[start..].to_vec()
            } else {
                Vec::new()
            };
            if visited_states.insert((e.to.as_str(), suffix)) {
                queue.push_back((e.to.as_str(), path));
            }
        }
    }

    while let Some((curr, path)) = queue.pop_front() {
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

                let suffix: Vec<String> = if history_len > 0 {
                    let start = next_path.len().saturating_sub(history_len);
                    next_path[start..].to_vec()
                } else {
                    Vec::new()
                };
                if visited_states.insert((e.to.as_str(), suffix)) {
                    queue.push_back((e.to.as_str(), next_path));
                }
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

    // 10b. First exit verification: for verified pairs, exit_id must be the first reachable Exit edge from anchor
    if pair.status == VerificationStatus::Verified {
        let (min_dist, first_exits) = find_first_exits_from_anchor(graph, &pair.anchor_node_id)
            .map_err(|e| err(&pair.id, "FIRST_EXIT_SEARCH_FAILED", e))?;
        if !first_exits.contains(&pair.exit_id) {
            let first_desc = first_exits.first().map(String::as_str).unwrap_or("none");
            return Err(err(
                &pair.id,
                "FIRST_EXIT_MISMATCH",
                format!(
                    "anchor node \"{}\" first reaches exit \"{}\" at distance {}m, but billing pair specifies exit \"{}\"",
                    pair.anchor_node_id, first_desc, min_dist, pair.exit_id
                ),
            ));
        }
    }

    // 11. Price intervals validity
    if pair.prices.len() > 1000 {
        return Err(err(&pair.id, "PRICE_LIMIT", "exceeds 1000 price intervals"));
    }
    let mut parsed_intervals: Vec<(OffsetDateTime, Option<OffsetDateTime>)> =
        Vec::with_capacity(pair.prices.len());
    for price in &pair.prices {
        if price.amount_yen == 0 {
            return Err(err(
                &pair.id,
                "INVALID_PRICE_AMOUNT",
                "toll amount must be > 0 yen",
            ));
        }
        let from_dt = match parse_utc_timestamp(&price.effective_from) {
            Ok(dt) => dt,
            Err(e) => {
                return Err(err(
                    &pair.id,
                    "INVALID_PRICE_INTERVAL",
                    format!("invalid effective_from \"{}\": {}", price.effective_from, e),
                ));
            }
        };
        let to_dt = if let Some(to) = &price.effective_to {
            let dt = match parse_utc_timestamp(to) {
                Ok(dt) => dt,
                Err(e) => {
                    return Err(err(
                        &pair.id,
                        "INVALID_PRICE_INTERVAL",
                        format!("invalid effective_to \"{}\": {}", to, e),
                    ));
                }
            };
            if dt <= from_dt {
                return Err(err(
                    &pair.id,
                    "INVALID_PRICE_INTERVAL",
                    format!(
                        "effective_to ({}) must be > effective_from ({})",
                        to, price.effective_from
                    ),
                ));
            }
            Some(dt)
        } else {
            None
        };
        parsed_intervals.push((from_dt, to_dt));
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
