//! Experimental, bounded routing on explicitly connected directed graphs.
//! This engine does not establish real-world toll eligibility or navigation safety.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::fmt;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// Routing engine version, reported as `manifest.engineVersion` so that the
/// manifest always records the search-engine version (not the builder's own).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Graph {
    pub schema_version: u32,
    pub release_id: String,
    pub vehicle_profile: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub billing_pairs: Vec<BillingPair>,
    #[serde(default)]
    pub forbidden_transitions: Vec<Vec<String>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Local,
    Entry,
    Shutoko,
    Exit,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub duration_seconds: u64,
    pub distance_meters: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    Unverified,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPair {
    pub id: String,
    pub entry_id: String,
    pub exit_id: String,
    pub anchor_node_id: String,
    pub entry_to_anchor_edge_ids: Vec<String>,
    pub anchor_to_exit_edge_ids: Vec<String>,
    pub status: VerificationStatus,
    pub vehicle_profile: String,
    #[serde(default)]
    pub prices: Vec<Price>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Price {
    pub amount_yen: u64,
    pub effective_from: String,
    pub effective_to: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRequest {
    pub request_id: String,
    pub release_id: String,
    pub origin_node_id: String,
    pub min_minutes: u64,
    pub max_minutes: u64,
    pub vehicle_profile: String,
    pub pricing_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SearchLimits {
    pub max_expanded_states: usize,
    pub beam_width: usize,
    pub max_loop_edges: usize,
    pub max_local_edges: usize,
    pub max_pairs: usize,
    pub max_candidates: usize,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_expanded_states: 100_000,
            beam_width: 200,
            max_loop_edges: 2000,
            max_local_edges: 200,
            max_pairs: 10,
            max_candidates: 3,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Duration {
    pub access_seconds: u64,
    pub shutoko_seconds: u64,
    pub return_seconds: u64,
    pub base_seconds: u64,
    pub buffer_seconds: u64,
    pub plan_seconds: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Toll {
    pub billing_pair_id: String,
    pub charged_section_count: u8,
    pub amount_yen: Option<u64>,
    pub pricing_at: String,
    pub effective_from: Option<String>,
    pub effective_to: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    pub anchor_node_id: String,
    pub edge_ids: Vec<String>,
    pub duration_seconds: u64,
    pub distance_meters: u64,
    pub validated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub release_id: String,
    pub origin_node_id: String,
    pub entry_id: String,
    pub exit_id: String,
    pub edge_ids: Vec<String>,
    pub duration: Duration,
    pub distance_meters: u64,
    pub shutoko_distance_meters: u64,
    pub toll: Toll,
    pub r#loop: Loop,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub request_id: String,
    pub release_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub ranking_mode: String,
    pub expanded_states: usize,
    pub candidates: Vec<Candidate>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoutingError {
    pub code: String,
    pub message: String,
}
impl fmt::Display for RoutingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for RoutingError {}
fn invalid(message: &str) -> RoutingError {
    RoutingError {
        code: "INVALID_INPUT".into(),
        message: message.into(),
    }
}
fn utc(s: &str) -> Result<OffsetDateTime, RoutingError> {
    if s.len() > 40 {
        return Err(invalid("timestamp too long"));
    }
    let t = OffsetDateTime::parse(s, &Rfc3339)
        .map_err(|_| invalid("invalid RFC3339 pricing timestamp"))?;
    if !s.ends_with('Z') {
        return Err(invalid("timestamps must use UTC Z suffix"));
    }
    Ok(t)
}
struct Index<'a> {
    edges: BTreeMap<&'a str, &'a Edge>,
    outgoing: BTreeMap<&'a str, Vec<&'a Edge>>,
    incoming: BTreeMap<&'a str, Vec<&'a Edge>>,
}
fn validate<'a>(
    g: &'a Graph,
    r: &SearchRequest,
    l: &SearchLimits,
) -> Result<Index<'a>, RoutingError> {
    if g.schema_version != 1
        || g.release_id.is_empty()
        || g.release_id.len() > 256
        || g.release_id != r.release_id
        || g.vehicle_profile.is_empty()
        || g.vehicle_profile.len() > 256
        || g.vehicle_profile != r.vehicle_profile
    {
        return Err(invalid("incompatible schema, release, or vehicle profile"));
    }
    if r.request_id.is_empty()
        || r.request_id.len() > 256
        || r.min_minutes == 0
        || r.min_minutes > r.max_minutes
        || r.max_minutes > 240
    {
        return Err(invalid(
            "time window must be ordered and between 1 and 240 minutes",
        ));
    }
    utc(&r.pricing_at)?;
    if l.max_expanded_states == 0
        || l.max_expanded_states > 1_000_000
        || l.beam_width == 0
        || l.beam_width > 200
        || l.max_loop_edges == 0
        || l.max_loop_edges > 2000
        || l.max_local_edges == 0
        || l.max_local_edges > 2000
        || l.max_pairs == 0
        || l.max_pairs > 100
        || l.max_candidates == 0
        || l.max_candidates > 3
    {
        return Err(invalid("search limits outside supported bounds"));
    }
    if g.nodes.len() > 100_000
        || g.edges.len() > 300_000
        || g.billing_pairs.len() > 10_000
        || g.forbidden_transitions.len() > 10_000
    {
        return Err(invalid("graph exceeds prototype size limits"));
    }
    let mut nodes = BTreeSet::new();
    for n in &g.nodes {
        if n.id.is_empty() || n.id.len() > 256 || !nodes.insert(n.id.as_str()) {
            return Err(invalid("duplicate, oversized, or empty node id"));
        }
    }
    if !nodes.contains(r.origin_node_id.as_str()) {
        return Err(invalid("unknown origin node"));
    }
    let mut ix = Index {
        edges: BTreeMap::new(),
        outgoing: BTreeMap::new(),
        incoming: BTreeMap::new(),
    };
    for e in &g.edges {
        if e.id.is_empty()
            || e.id.len() > 256
            || !nodes.contains(e.from.as_str())
            || !nodes.contains(e.to.as_str())
            || e.duration_seconds == 0
            || e.duration_seconds > 86400
            || e.distance_meters == 0
            || e.distance_meters > 10_000_000
            || ix.edges.insert(&e.id, e).is_some()
        {
            return Err(invalid("invalid edge, endpoint, weight, or duplicate id"));
        }
        ix.outgoing.entry(&e.from).or_default().push(e);
        ix.incoming.entry(&e.to).or_default().push(e);
    }
    for edges in ix.outgoing.values_mut() {
        edges.sort_by(|a, b| a.id.cmp(&b.id));
    }
    for edges in ix.incoming.values_mut() {
        edges.sort_by(|a, b| a.id.cmp(&b.id));
    }
    if g.forbidden_transitions.iter().map(Vec::len).sum::<usize>() > 20_000 {
        return Err(invalid("too many forbidden transition edges"));
    }
    for seq in &g.forbidden_transitions {
        if seq.len() < 2
            || seq.len() > 2000
            || seq.iter().any(|id| !ix.edges.contains_key(id.as_str()))
        {
            return Err(invalid("invalid forbidden transition sequence"));
        }
        for pair in seq.windows(2) {
            if ix.edges[pair[0].as_str()].to != ix.edges[pair[1].as_str()].from {
                return Err(invalid("disconnected forbidden transition sequence"));
            }
        }
    }
    let mut ids = BTreeSet::new();
    if g.billing_pairs
        .iter()
        .fold(0usize, |total, p| total.saturating_add(p.prices.len()))
        > 20_000
        || g.billing_pairs.iter().fold(0usize, |total, p| {
            total
                .saturating_add(p.entry_to_anchor_edge_ids.len())
                .saturating_add(p.anchor_to_exit_edge_ids.len())
        }) > 100_000
    {
        return Err(invalid("billing pair data exceeds prototype size limits"));
    }
    for p in &g.billing_pairs {
        if p.id.is_empty()
            || p.id.len() > 256
            || !ids.insert(&p.id)
            || p.entry_id.is_empty()
            || p.entry_id.len() > 256
            || p.exit_id.is_empty()
            || p.exit_id.len() > 256
            || p.vehicle_profile != g.vehicle_profile
            || !nodes.contains(p.anchor_node_id.as_str())
            || p.prices.len() > 1000
        {
            return Err(invalid("invalid billing pair identity or profile"));
        }
        let pre = path(&ix, &p.entry_to_anchor_edge_ids)?;
        let post = path(&ix, &p.anchor_to_exit_edge_ids)?;
        if pre.is_empty()
            || post.is_empty()
            || pre[0].id != p.entry_id
            || post.last().unwrap().id != p.exit_id
            || pre[0].kind != EdgeKind::Entry
            || pre[1..].iter().any(|e| e.kind != EdgeKind::Shutoko)
            || post.last().unwrap().kind != EdgeKind::Exit
            || post[..post.len() - 1]
                .iter()
                .any(|e| e.kind != EdgeKind::Shutoko)
            || pre.last().unwrap().to != p.anchor_node_id
            || post[0].from != p.anchor_node_id
        {
            return Err(invalid(
                "billing pair requires entry-to-anchor and anchor-to-exit paths",
            ));
        }
        // The direct entry-to-exit baseline must not already contain a lap,
        // including a cycle crossing the boundary between its two connectors.
        // The separately searched loop may still share edges with this baseline.
        let mut seen = BTreeSet::new();
        seen.insert(pre[0].from.as_str());
        if pre.iter().chain(&post).any(|e| !seen.insert(e.to.as_str())) {
            return Err(invalid("billing pair direct path must be a simple path"));
        }
        let mut periods = Vec::new();
        for price in &p.prices {
            let from = utc(&price.effective_from)?;
            let to = price.effective_to.as_deref().map(utc).transpose()?;
            if price.amount_yen == 0 || to.is_some_and(|t| t <= from) {
                return Err(invalid("invalid toll amount or interval"));
            }
            periods.push((from, to));
        }
        periods.sort_by_key(|p| p.0);
        for pair in periods.windows(2) {
            if pair[0].1.is_none_or(|end| end > pair[1].0) {
                return Err(invalid("overlapping toll intervals"));
            }
        }
    }
    Ok(ix)
}
fn path<'a>(ix: &Index<'a>, ids: &[String]) -> Result<Vec<&'a Edge>, RoutingError> {
    if ids.len() > 2000 {
        return Err(invalid("connection path exceeds 2000 edges"));
    }
    let edges: Vec<_> = ids
        .iter()
        .map(|id| {
            ix.edges
                .get(id.as_str())
                .copied()
                .ok_or_else(|| invalid("unknown path edge"))
        })
        .collect::<Result<_, _>>()?;
    if edges.windows(2).any(|w| w[0].to != w[1].from) {
        return Err(invalid("disconnected path"));
    }
    Ok(edges)
}
fn allowed(g: &Graph, edges: &[&Edge]) -> bool {
    !g.forbidden_transitions.iter().any(|seq| {
        edges.len() >= seq.len()
            && edges
                .windows(seq.len())
                .any(|w| w.iter().zip(seq).all(|(e, id)| e.id == *id))
    })
}
#[derive(Default)]
struct Budget {
    expanded: usize,
    truncated: bool,
}
impl Budget {
    fn take(&mut self, l: &SearchLimits) -> bool {
        if self.expanded >= l.max_expanded_states {
            self.truncated = true;
            false
        } else {
            self.expanded += 1;
            true
        }
    }
}
// Iterative simple-path enumeration. The same global budget includes local paths and combinations.
#[allow(clippy::too_many_arguments)]
fn paths<'a>(
    g: &Graph,
    ix: &Index<'a>,
    start: &str,
    end: &str,
    kind: EdgeKind,
    depth: usize,
    max_seconds: u64,
    l: &SearchLimits,
    budget: &mut Budget,
) -> Vec<Vec<&'a Edge>> {
    if start == end && kind == EdgeKind::Local {
        return vec![vec![]];
    }
    let mut frontier: Vec<Vec<&Edge>> = vec![vec![]];
    let mut results = Vec::new();
    let mut retained_edges = 0;
    for d in 0..depth {
        let mut next = Vec::new();
        for current in frontier {
            let at = current.last().map_or(start, |e| e.to.as_str());
            if let Some(outgoing) = ix.outgoing.get(at) {
                for e in outgoing.iter().copied().filter(|e| e.kind == kind) {
                    if !budget.take(l) {
                        return results;
                    }
                    if current.iter().map(|e| e.duration_seconds).sum::<u64>() + e.duration_seconds
                        > max_seconds
                    {
                        continue;
                    }
                    let mut candidate = current.clone();
                    candidate.push(e);
                    if !allowed(g, &candidate) {
                        continue;
                    }
                    if e.to == end {
                        if results.len() == l.beam_width
                            || retained_edges + candidate.len() > 20_000
                        {
                            budget.truncated = true;
                            return results;
                        }
                        retained_edges += candidate.len();
                        results.push(candidate);
                        continue;
                    }
                    if e.to == start || current.iter().any(|old| old.to == e.to) {
                        continue;
                    }
                    if d + 1 == depth {
                        budget.truncated = true;
                        continue;
                    }
                    if next.len() < l.beam_width {
                        next.push(candidate);
                    } else {
                        budget.truncated = true;
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    results
}

#[derive(Clone)]
struct DijkstraState<'a> {
    duration_seconds: u64,
    distance_meters: u64,
    edge_id: &'a str,
    node: &'a str,
    edge: Option<&'a Edge>,
    parent_index: Option<usize>,
    depth: usize,
    suffix: Vec<&'a str>,
}

impl<'a> PartialEq for DijkstraState<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.duration_seconds == other.duration_seconds
            && self.distance_meters == other.distance_meters
            && self.edge_id == other.edge_id
            && self.node == other.node
            && self.suffix == other.suffix
    }
}

impl<'a> Eq for DijkstraState<'a> {}

impl<'a> Ord for DijkstraState<'a> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .duration_seconds
            .cmp(&self.duration_seconds)
            .then_with(|| other.distance_meters.cmp(&self.distance_meters))
            .then_with(|| other.edge_id.cmp(self.edge_id))
            .then_with(|| other.node.cmp(self.node))
            .then_with(|| other.suffix.cmp(&self.suffix))
    }
}

impl<'a> PartialOrd for DijkstraState<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn next_suffix<'a>(
    current_suffix: &[&'a str],
    next_edge_id: &'a str,
    history_len: usize,
) -> Vec<&'a str> {
    if history_len == 0 {
        return Vec::new();
    }
    let mut next = Vec::with_capacity(history_len);
    let start = current_suffix.len().saturating_sub(history_len - 1);
    next.extend_from_slice(&current_suffix[start..]);
    next.push(next_edge_id);
    next
}

struct HistoryNode<'a> {
    edge: &'a Edge,
    parent_index: Option<usize>,
}

fn transition_allowed_forward<'a>(
    g: &Graph,
    history: &[HistoryNode<'a>],
    my_history_index: Option<usize>,
    next_edge: &'a Edge,
) -> bool {
    for seq in &g.forbidden_transitions {
        if seq.last().map(String::as_str) != Some(&next_edge.id) {
            continue;
        }
        let mut curr = my_history_index;
        let mut matched = true;
        for target_id in seq[..seq.len() - 1].iter().rev() {
            if let Some(idx) = curr {
                if history[idx].edge.id != *target_id {
                    matched = false;
                    break;
                }
                curr = history[idx].parent_index;
            } else {
                matched = false;
                break;
            }
        }
        if matched {
            return false;
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn dijkstra_local_forward<'a>(
    g: &Graph,
    ix: &Index<'a>,
    origin: &'a str,
    target_entry_edges: &[&'a Edge],
    max_local_edges: usize,
    max_seconds: u64,
    l: &SearchLimits,
    budget: &mut Budget,
) -> BTreeMap<&'a str, Vec<&'a Edge>> {
    let max_forbidden_len = g
        .forbidden_transitions
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0);
    let history_len = max_forbidden_len.saturating_sub(1);

    let mut entry_by_from: BTreeMap<&'a str, Vec<&'a Edge>> = BTreeMap::new();
    for e in target_entry_edges {
        entry_by_from.entry(e.from.as_str()).or_default().push(e);
    }
    let total_targets = target_entry_edges.len();

    let mut best_for_entry: BTreeMap<&'a str, Vec<&'a Edge>> = BTreeMap::new();
    let mut best_cost: BTreeMap<(&'a str, Vec<&'a str>), (u64, u64)> = BTreeMap::new();
    let mut history: Vec<HistoryNode<'a>> = Vec::new();
    let mut pq: BinaryHeap<DijkstraState<'a>> = BinaryHeap::new();

    best_cost.insert((origin, Vec::new()), (0, 0));
    pq.push(DijkstraState {
        duration_seconds: 0,
        distance_meters: 0,
        edge_id: "",
        node: origin,
        edge: None,
        parent_index: None,
        depth: 0,
        suffix: Vec::new(),
    });

    while let Some(current) = pq.pop() {
        let state_key = (current.node, current.suffix.clone());
        if let Some(&(best_d, best_m)) = best_cost.get(&state_key) {
            if (current.duration_seconds, current.distance_meters) > (best_d, best_m) {
                continue;
            }
        }

        if !budget.take(l) {
            break;
        }

        let my_history_index = if let Some(e) = current.edge {
            let idx = history.len();
            history.push(HistoryNode {
                edge: e,
                parent_index: current.parent_index,
            });
            Some(idx)
        } else {
            None
        };

        if let Some(entries) = entry_by_from.get(current.node) {
            for &entry_e in entries {
                if !best_for_entry.contains_key(entry_e.id.as_str())
                    && transition_allowed_forward(g, &history, my_history_index, entry_e)
                {
                    let mut path = Vec::new();
                    let mut curr = my_history_index;
                    while let Some(idx) = curr {
                        path.push(history[idx].edge);
                        curr = history[idx].parent_index;
                    }
                    path.reverse();
                    best_for_entry.insert(entry_e.id.as_str(), path);
                }
            }
            if best_for_entry.len() == total_targets {
                break;
            }
        }

        if current.depth >= max_local_edges {
            continue;
        }

        if let Some(outgoing) = ix.outgoing.get(current.node) {
            for e in outgoing
                .iter()
                .copied()
                .filter(|e| e.kind == EdgeKind::Local)
            {
                let next_d = current.duration_seconds + e.duration_seconds;
                if next_d > max_seconds {
                    continue;
                }
                let next_m = current.distance_meters + e.distance_meters;

                if !transition_allowed_forward(g, &history, my_history_index, e) {
                    continue;
                }

                let next_suf = next_suffix(&current.suffix, e.id.as_str(), history_len);
                let next_state_key = (e.to.as_str(), next_suf.clone());
                if let Some(&(best_d, best_m)) = best_cost.get(&next_state_key) {
                    if (next_d, next_m) >= (best_d, best_m) {
                        continue;
                    }
                }
                best_cost.insert(next_state_key, (next_d, next_m));
                pq.push(DijkstraState {
                    duration_seconds: next_d,
                    distance_meters: next_m,
                    edge_id: e.id.as_str(),
                    node: e.to.as_str(),
                    edge: Some(e),
                    parent_index: my_history_index,
                    depth: current.depth + 1,
                    suffix: next_suf,
                });
            }
        }
    }

    best_for_entry
}

#[allow(clippy::too_many_arguments)]
fn dijkstra_local_backward<'a>(
    g: &Graph,
    ix: &Index<'a>,
    exit_edges: &[&'a Edge],
    destination: &'a str,
    max_local_edges: usize,
    max_seconds: u64,
    l: &SearchLimits,
    budget: &mut Budget,
) -> BTreeMap<&'a str, Vec<&'a Edge>> {
    let max_forbidden_len = g
        .forbidden_transitions
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0);
    let history_len = max_forbidden_len.saturating_sub(1);

    let mut best_for_exit: BTreeMap<&'a str, Vec<&'a Edge>> = BTreeMap::new();

    for &exit_edge in exit_edges {
        if best_for_exit.contains_key(exit_edge.id.as_str()) {
            continue;
        }

        if exit_edge.to == destination {
            best_for_exit.insert(exit_edge.id.as_str(), Vec::new());
            continue;
        }

        let mut best_cost: BTreeMap<(&'a str, Vec<&'a str>), (u64, u64)> = BTreeMap::new();
        let mut history: Vec<HistoryNode<'a>> = Vec::new();
        let mut pq: BinaryHeap<DijkstraState<'a>> = BinaryHeap::new();

        history.push(HistoryNode {
            edge: exit_edge,
            parent_index: None,
        });
        let initial_history_index = Some(0);

        let initial_suffix = if history_len > 0 {
            vec![exit_edge.id.as_str()]
        } else {
            Vec::new()
        };

        best_cost.insert((exit_edge.to.as_str(), initial_suffix.clone()), (0, 0));
        pq.push(DijkstraState {
            duration_seconds: 0,
            distance_meters: 0,
            edge_id: exit_edge.id.as_str(),
            node: exit_edge.to.as_str(),
            edge: None,
            parent_index: initial_history_index,
            depth: 0,
            suffix: initial_suffix,
        });

        while let Some(current) = pq.pop() {
            let state_key = (current.node, current.suffix.clone());
            if let Some(&(best_d, best_m)) = best_cost.get(&state_key) {
                if (current.duration_seconds, current.distance_meters) > (best_d, best_m) {
                    continue;
                }
            }

            if !budget.take(l) {
                break;
            }

            let my_history_index = if let Some(e) = current.edge {
                let idx = history.len();
                history.push(HistoryNode {
                    edge: e,
                    parent_index: current.parent_index,
                });
                Some(idx)
            } else {
                current.parent_index
            };

            if current.node == destination {
                let mut path = Vec::new();
                let mut curr = my_history_index;
                while let Some(idx) = curr {
                    if idx == 0 {
                        break;
                    }
                    path.push(history[idx].edge);
                    curr = history[idx].parent_index;
                }
                path.reverse();
                best_for_exit.insert(exit_edge.id.as_str(), path);
                break;
            }

            if current.depth >= max_local_edges {
                continue;
            }

            if let Some(outgoing) = ix.outgoing.get(current.node) {
                for e in outgoing
                    .iter()
                    .copied()
                    .filter(|e| e.kind == EdgeKind::Local)
                {
                    let next_d = current.duration_seconds + e.duration_seconds;
                    if next_d > max_seconds {
                        continue;
                    }
                    let next_m = current.distance_meters + e.distance_meters;

                    if !transition_allowed_forward(g, &history, my_history_index, e) {
                        continue;
                    }

                    let next_suf = next_suffix(&current.suffix, e.id.as_str(), history_len);
                    let next_state_key = (e.to.as_str(), next_suf.clone());
                    if let Some(&(best_d, best_m)) = best_cost.get(&next_state_key) {
                        if (next_d, next_m) >= (best_d, best_m) {
                            continue;
                        }
                    }
                    best_cost.insert(next_state_key, (next_d, next_m));
                    pq.push(DijkstraState {
                        duration_seconds: next_d,
                        distance_meters: next_m,
                        edge_id: e.id.as_str(),
                        node: e.to.as_str(),
                        edge: Some(e),
                        parent_index: my_history_index,
                        depth: current.depth + 1,
                        suffix: next_suf,
                    });
                }
            }
        }
    }

    best_for_exit
}

// Validated weights and segment lengths bound a full route to 10,000 edges:
// <= 864,000,000 seconds and <= 100,000,000,000 meters, far below u64::MAX.
fn seconds(edges: &[&Edge]) -> u64 {
    edges.iter().map(|e| e.duration_seconds).sum()
}
fn meters(edges: &[&Edge]) -> u64 {
    edges.iter().map(|e| e.distance_meters).sum()
}
fn edge_ids(edges: &[&Edge]) -> Vec<String> {
    edges.iter().map(|e| e.id.clone()).collect()
}
/// Search synthetic or prevalidated graphs. Returned routes are experimental and have no Maps handoff.
pub fn search(
    g: &Graph,
    r: &SearchRequest,
    l: &SearchLimits,
) -> Result<SearchResult, RoutingError> {
    let ix = validate(g, r, l)?;
    let now = utc(&r.pricing_at)?;
    let mut budget = Budget::default();
    let mut candidates = Vec::new();
    let mut candidate_edges = 0;
    let mut pairs: Vec<_> = g
        .billing_pairs
        .iter()
        .filter(|p| p.status == VerificationStatus::Verified)
        .collect();
    pairs.sort_by(|a, b| a.id.cmp(&b.id));
    if pairs.len() > l.max_pairs {
        budget.truncated = true;
        pairs.truncate(l.max_pairs);
    }
    let any_pairs = !pairs.is_empty();
    let mut connection = false;
    let mut found_loop = false;
    let mut legal_route = false;

    let mut verified_pairs = Vec::with_capacity(pairs.len());
    let mut entry_edges = Vec::with_capacity(pairs.len());
    let mut exit_edges = Vec::with_capacity(pairs.len());
    for p in pairs {
        let pre = path(&ix, &p.entry_to_anchor_edge_ids)?;
        let post = path(&ix, &p.anchor_to_exit_edge_ids)?;
        entry_edges.push(pre[0]);
        exit_edges.push(post.last().copied().unwrap());
        verified_pairs.push((p, pre, post));
    }
    entry_edges.sort_by_key(|e| e.id.as_str());
    entry_edges.dedup_by_key(|e| e.id.as_str());
    exit_edges.sort_by_key(|e| e.id.as_str());
    exit_edges.dedup_by_key(|e| e.id.as_str());

    let forward_map = dijkstra_local_forward(
        g,
        &ix,
        &r.origin_node_id,
        &entry_edges,
        l.max_local_edges,
        r.max_minutes * 60,
        l,
        &mut budget,
    );
    let mut active_exit_edges = Vec::new();
    for (_p, pre, post) in &verified_pairs {
        let entry_edge = pre[0];
        if forward_map.contains_key(entry_edge.id.as_str()) {
            active_exit_edges.push(post.last().copied().unwrap());
        }
    }
    active_exit_edges.sort_by_key(|e| e.id.as_str());
    active_exit_edges.dedup_by_key(|e| e.id.as_str());

    let backward_map = dijkstra_local_backward(
        g,
        &ix,
        &active_exit_edges,
        &r.origin_node_id,
        l.max_local_edges,
        r.max_minutes * 60,
        l,
        &mut budget,
    );

    'pairs: for (p, pre, post) in verified_pairs {
        let entry_edge = pre[0];
        let exit_edge = post.last().unwrap();
        let (Some(access), Some(returns)) = (
            forward_map.get(entry_edge.id.as_str()),
            backward_map.get(exit_edge.id.as_str()),
        ) else {
            continue;
        };
        connection = true;

        let loops = paths(
            g,
            &ix,
            &p.anchor_node_id,
            &p.anchor_node_id,
            EdgeKind::Shutoko,
            l.max_loop_edges,
            r.max_minutes * 60,
            l,
            &mut budget,
        );
        found_loop |= !loops.is_empty();

        for cycle in &loops {
            if !budget.take(l) {
                break 'pairs;
            }
            let highway: Vec<_> = pre.iter().chain(cycle).chain(&post).copied().collect();
            let all: Vec<_> = access
                .iter()
                .chain(&highway)
                .chain(returns)
                .copied()
                .collect();
            if !allowed(g, &all) {
                continue;
            }
            legal_route = true;
            let base = seconds(&all);
            let buffer = 300.max(base.div_ceil(5));
            if base < r.min_minutes * 60 || base + buffer > r.max_minutes * 60 {
                continue;
            }
            if candidates.len() == l.beam_width || candidate_edges + all.len() > 20_000 {
                budget.truncated = true;
                break 'pairs;
            }
            candidate_edges += all.len();
            let price = p.prices.iter().find(|v| {
                utc(&v.effective_from).is_ok_and(|from| from <= now)
                    && v.effective_to
                        .as_deref()
                        .is_none_or(|to| utc(to).is_ok_and(|to| now < to))
            });
            let ids = edge_ids(&all);
            // Length-prefixed components provide stable, collision-free IDs without hashing.
            let id = std::iter::once(p.id.as_str())
                .chain(ids.iter().map(String::as_str))
                .map(|s| format!("{}:{}", s.len(), s))
                .collect::<String>();
            candidates.push(Candidate {
                id,
                release_id: r.release_id.clone(),
                origin_node_id: r.origin_node_id.clone(),
                entry_id: p.entry_id.clone(),
                exit_id: p.exit_id.clone(),
                edge_ids: ids,
                duration: Duration {
                    access_seconds: seconds(access),
                    shutoko_seconds: seconds(&highway),
                    return_seconds: seconds(returns),
                    base_seconds: base,
                    buffer_seconds: buffer,
                    plan_seconds: base + buffer,
                },
                distance_meters: meters(&all),
                shutoko_distance_meters: meters(&highway),
                toll: Toll {
                    billing_pair_id: p.id.clone(),
                    charged_section_count: 1,
                    amount_yen: price.map(|v| v.amount_yen),
                    pricing_at: r.pricing_at.clone(),
                    effective_from: price.map(|v| v.effective_from.clone()),
                    effective_to: price.and_then(|v| v.effective_to.clone()),
                },
                r#loop: Loop {
                    anchor_node_id: p.anchor_node_id.clone(),
                    edge_ids: edge_ids(cycle),
                    duration_seconds: seconds(cycle),
                    distance_meters: meters(cycle),
                    validated: true,
                },
                warnings: vec![
                    "EXPERIMENTAL_NO_HANDOFF".into(),
                    "STATIC_TRAVEL_TIME".into(),
                ],
            });
        }
    }
    let time_ranking = candidates.iter().any(|c| c.toll.amount_yen.is_none());
    candidates.sort_by(|a, b| {
        let ratio = if time_ranking {
            std::cmp::Ordering::Equal
        } else {
            ((b.duration.shutoko_seconds as u128) * (a.toll.amount_yen.unwrap_or(1) as u128)).cmp(
                &((a.duration.shutoko_seconds as u128) * (b.toll.amount_yen.unwrap_or(1) as u128)),
            )
        };
        ratio
            .then_with(|| b.duration.shutoko_seconds.cmp(&a.duration.shutoko_seconds))
            .then_with(|| {
                (a.duration.access_seconds + a.duration.return_seconds)
                    .cmp(&(b.duration.access_seconds + b.duration.return_seconds))
            })
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut selected: Vec<Candidate> = Vec::new();
    for c in candidates {
        if selected.iter().any(|s| similar(&ix, &c, s)) {
            continue;
        }
        selected.push(c);
        if selected.len() == l.max_candidates {
            break;
        }
    }
    let reason = if budget.truncated {
        Some("SEARCH_LIMIT")
    } else if !selected.is_empty() {
        None
    } else if !any_pairs {
        Some("NO_BILLING_PAIR")
    } else if !connection {
        Some("NO_CONNECTION")
    } else if !found_loop || !legal_route {
        Some("NO_LOOP")
    } else {
        Some("TIME_WINDOW")
    };
    Ok(SearchResult {
        request_id: r.request_id.clone(),
        release_id: r.release_id.clone(),
        status: if budget.truncated {
            "truncated"
        } else if selected.is_empty() {
            "no_candidates"
        } else {
            "ok"
        }
        .into(),
        reason: reason.map(str::to_owned),
        ranking_mode: if time_ranking {
            "shutoko_time"
        } else {
            "time_per_yen"
        }
        .into(),
        expanded_states: budget.expanded,
        candidates: selected,
    })
}
fn similar(ix: &Index<'_>, a: &Candidate, b: &Candidate) -> bool {
    let set = |c: &Candidate| -> BTreeSet<String> {
        c.edge_ids
            .iter()
            .filter(|id| ix.edges[id.as_str()].kind != EdgeKind::Local)
            .cloned()
            .collect()
    };
    let sa = set(a);
    let sb = set(b);
    let shared: u64 = sa
        .intersection(&sb)
        .map(|id| ix.edges[id.as_str()].distance_meters)
        .sum();
    let union: u64 = sa
        .union(&sb)
        .map(|id| ix.edges[id.as_str()].distance_meters)
        .sum();
    u128::from(shared) * 5 >= u128::from(union) * 4
}
/// Parse and validate strict JSON, then serialize the search response.
pub fn search_json(
    graph_json: &str,
    request_json: &str,
    limits_json: &str,
) -> Result<String, RoutingError> {
    if graph_json.len() > 64 * 1024 * 1024
        || request_json.len() > 16 * 1024
        || limits_json.len() > 4096
    {
        return Err(invalid("JSON payload exceeds prototype size limit"));
    }
    let g: Graph = serde_json::from_str(graph_json).map_err(|_| invalid("invalid graph JSON"))?;
    let r: SearchRequest =
        serde_json::from_str(request_json).map_err(|_| invalid("invalid request JSON"))?;
    let l: SearchLimits =
        serde_json::from_str(limits_json).map_err(|_| invalid("invalid limits JSON"))?;
    serde_json::to_string(&search(&g, &r, &l)?).map_err(|_| invalid("result serialization failed"))
}
