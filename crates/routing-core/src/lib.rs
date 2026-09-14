//! Experimental, bounded routing on explicitly connected directed graphs.
//! This engine does not establish real-world toll eligibility or navigation safety.
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::fmt;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// Routing engine version, reported as `manifest.engineVersion` so that the
/// manifest always records the search-engine version (not the builder's own).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Snap search radius threshold in meters.
pub const SNAP_RADIUS_METERS: f64 = 200.0;

pub mod grid;
pub mod handoff;

/// Google Maps handoff payload for a candidate route.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    pub origin: LatLng,
    pub destination: LatLng,
    pub waypoints: Vec<LatLng>,
    pub maps_url: String,
    pub verification_set_version: Option<String>,
}

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_name: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Price {
    pub amount_yen: u64,
    pub effective_from: String,
    pub effective_to: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LatLng {
    pub lat: f64,
    pub lon: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRequest {
    pub request_id: String,
    pub release_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<LatLng>,
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
    /// Maximum number of nodes allowed in the graph. Defaults to 1,000,000.
    pub max_graph_nodes: usize,
    /// Maximum number of edges allowed in the graph. Defaults to 3,000,000.
    pub max_graph_edges: usize,
    /// Maximum Dijkstra expansion radius from the origin node in metres.
    /// `0.0` (the default) means unlimited — existing callers are unaffected.
    pub max_access_radius_meters: f64,
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
            max_graph_nodes: 1_000_000,
            max_graph_edges: 3_000_000,
            max_access_radius_meters: 0.0,
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
pub struct SnappedOrigin {
    pub node_id: String,
    pub lat: f64,
    pub lon: f64,
    pub distance_meters: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RampInfo {
    pub edge_id: String,
    pub name: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoJsonLineString {
    pub r#type: String,
    pub coordinates: Vec<[f64; 2]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub release_id: String,
    pub origin: Option<LatLng>,
    pub origin_node_id: String,
    pub snapped_origin: SnappedOrigin,
    pub entry: RampInfo,
    pub exit: RampInfo,
    pub entry_id: String,
    pub exit_id: String,
    pub road_names: Vec<String>,
    pub edge_ids: Vec<String>,
    pub geometry: GeoJsonLineString,
    pub duration: Duration,
    pub distance_meters: u64,
    pub shutoko_distance_meters: u64,
    pub toll: Toll,
    pub r#loop: Loop,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub handoff: Handoff,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingError {
    pub code: String,
    pub message: String,
}
impl RoutingError {
    /// Format error as JSON string conforming to RoutingErrorPayload schema.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                "{{\"code\":\"{}\",\"message\":\"{}\"}}",
                self.code, self.message
            )
        })
    }
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
/// Compute distance between two lat/lon coordinates using equirectangular approximation.
pub fn distance_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let phi = (lat1 + lat2) * 0.5 * std::f64::consts::PI / 180.0;
    let dx = (lon2 - lon1) * std::f64::consts::PI / 180.0 * phi.cos() * R;
    let dy = (lat2 - lat1) * std::f64::consts::PI / 180.0 * R;
    dx.hypot(dy)
}

// ---------------------------------------------------------------------------
// Owned index — no lifetime parameters, stored in PreparedGraph.
// ---------------------------------------------------------------------------

/// Fully-owned graph index built once by [`prepare`] and reused across many
/// [`search_prepared`] calls without any per-call rebuild cost.
struct OwnedIndex {
    /// Maps node ID → index into `graph.nodes`.
    node_pos: HashMap<String, usize>,
    /// Maps edge ID → index into `graph.edges`.
    edge_pos: HashMap<String, usize>,
    /// Maps node ID → sorted list of outgoing edge indices (sorted by edge ID
    /// for deterministic expansion order).
    outgoing: HashMap<String, Vec<usize>>,
    /// Spatial index for fast coordinate → nearest-local-node snapping.
    snap_grid: grid::OwnedSnapGrid,
}

/// A prepared graph: owns the graph data plus pre-built search indices.
///
/// Create with [`prepare`]; then pass to [`search_prepared`] as many times as
/// needed. Building the index is O(n log n + m log m) and happens **once**;
/// each subsequent search skips that cost entirely.
pub struct PreparedGraph {
    /// The owned graph data.
    pub graph: Graph,
    /// Search limits used during preparation (re-validated on each search).
    pub limits: SearchLimits,
    /// Pre-built index over the graph.
    index: OwnedIndex,
    /// Cache: (anchor_node_id, max_seconds) → set of reachable Shutoko node
    /// indices (into `graph.nodes`).  Uses interior mutability so that
    /// `search_prepared` can populate the cache via a shared reference.
    ///
    /// Task C': avoids recomputing the same Shutoko-reachability BFS when
    /// the same PreparedGraph is queried repeatedly with the same time window.
    reachable_cache: RefCell<HashMap<(String, u64), BTreeSet<usize>>>,
}

impl PreparedGraph {
    #[inline]
    fn node(&self, id: &str) -> &Node {
        &self.graph.nodes[self.index.node_pos[id]]
    }

    #[inline]
    fn edge(&self, id: &str) -> &Edge {
        &self.graph.edges[self.index.edge_pos[id]]
    }

    #[inline]
    fn has_node(&self, id: &str) -> bool {
        self.index.node_pos.contains_key(id)
    }

    #[inline]
    fn has_edge(&self, id: &str) -> bool {
        self.index.edge_pos.contains_key(id)
    }

    fn outgoing_edges(&self, node_id: &str) -> impl Iterator<Item = &Edge> {
        self.index
            .outgoing
            .get(node_id)
            .into_iter()
            .flat_map(|idxs| idxs.iter().map(|&i| &self.graph.edges[i]))
    }
}

// ---------------------------------------------------------------------------
// Index construction and request validation
// ---------------------------------------------------------------------------

/// Validate the graph structure and limits, then build the [`OwnedIndex`].
/// This is the expensive O(n log n + m log m) step; it happens once in [`prepare`].
fn build_owned_index(g: &Graph, l: &SearchLimits) -> Result<OwnedIndex, RoutingError> {
    if g.schema_version != 2
        || g.release_id.is_empty()
        || g.release_id.len() > 256
        || g.vehicle_profile.is_empty()
        || g.vehicle_profile.len() > 256
    {
        return Err(invalid("incompatible schema or vehicle profile"));
    }
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
    if l.max_access_radius_meters < 0.0 || l.max_access_radius_meters.is_nan() {
        return Err(invalid("max_access_radius_meters must be non-negative"));
    }
    if g.nodes.len() > l.max_graph_nodes
        || g.edges.len() > l.max_graph_edges
        || g.billing_pairs.len() > 10_000
        || g.forbidden_transitions.len() > 10_000
    {
        return Err(invalid("graph exceeds prototype size limits"));
    }

    // Build node_pos: HashMap<String, usize>.
    let mut node_pos: HashMap<String, usize> = HashMap::with_capacity(g.nodes.len());
    for (i, n) in g.nodes.iter().enumerate() {
        if n.id.is_empty() || n.id.len() > 256 || node_pos.insert(n.id.clone(), i).is_some() {
            return Err(invalid("duplicate, oversized, or empty node id"));
        }
        if !n.lat.is_finite()
            || !n.lon.is_finite()
            || !(-90.0..=90.0).contains(&n.lat)
            || !(-180.0..=180.0).contains(&n.lon)
        {
            return Err(invalid("node coordinates out of range or non-finite"));
        }
    }

    // Build edge_pos, outgoing, and collect local-node IDs for the snap grid.
    let mut edge_pos: HashMap<String, usize> = HashMap::with_capacity(g.edges.len());
    let mut outgoing: HashMap<String, Vec<usize>> = HashMap::with_capacity(g.nodes.len());
    // local_node_ids is a BTreeSet so iteration is in lex order, which the
    // OwnedSnapGrid construction relies on for deterministic tie-breaking.
    let mut local_node_ids: BTreeSet<String> = BTreeSet::new();

    for (i, e) in g.edges.iter().enumerate() {
        if e.id.is_empty()
            || e.id.len() > 256
            || !node_pos.contains_key(&e.from)
            || !node_pos.contains_key(&e.to)
            || e.duration_seconds == 0
            || e.duration_seconds > 86400
            || e.distance_meters == 0
            || e.distance_meters > 10_000_000
            || edge_pos.insert(e.id.clone(), i).is_some()
        {
            return Err(invalid("invalid edge, endpoint, weight, or duplicate id"));
        }
        if e.kind == EdgeKind::Local {
            local_node_ids.insert(e.from.clone());
            local_node_ids.insert(e.to.clone());
        }
        outgoing.entry(e.from.clone()).or_default().push(i);
    }

    // Sort outgoing adjacency lists by edge ID for deterministic expansion order.
    for v in outgoing.values_mut() {
        v.sort_by(|&a, &b| g.edges[a].id.cmp(&g.edges[b].id));
    }

    // Build snap grid from local-node indices.
    let local_indices: Vec<usize> = local_node_ids
        .iter()
        .map(|id| node_pos[id.as_str()])
        .collect();
    let snap_grid = grid::OwnedSnapGrid::build(local_indices, &g.nodes);

    // Validate forbidden transitions.
    if g.forbidden_transitions.iter().map(Vec::len).sum::<usize>() > 20_000 {
        return Err(invalid("too many forbidden transition edges"));
    }
    for seq in &g.forbidden_transitions {
        if seq.len() < 2
            || seq.len() > 2000
            || seq.iter().any(|id| !edge_pos.contains_key(id.as_str()))
        {
            return Err(invalid("invalid forbidden transition sequence"));
        }
        for pair in seq.windows(2) {
            if g.edges[edge_pos[pair[0].as_str()]].to != g.edges[edge_pos[pair[1].as_str()]].from {
                return Err(invalid("disconnected forbidden transition sequence"));
            }
        }
    }

    // Validate billing pairs.
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
    let mut ids = BTreeSet::new();
    for p in &g.billing_pairs {
        if p.id.is_empty()
            || p.id.len() > 256
            || !ids.insert(&p.id)
            || p.entry_id.is_empty()
            || p.entry_id.len() > 256
            || p.exit_id.is_empty()
            || p.exit_id.len() > 256
            || p.vehicle_profile != g.vehicle_profile
            || !node_pos.contains_key(p.anchor_node_id.as_str())
            || p.prices.len() > 1000
        {
            return Err(invalid("invalid billing pair identity or profile"));
        }
        let pre = path_from_index(g, &edge_pos, &p.entry_to_anchor_edge_ids)?;
        let post = path_from_index(g, &edge_pos, &p.anchor_to_exit_edge_ids)?;
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
        // The direct entry-to-exit baseline must not already contain a lap.
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

    Ok(OwnedIndex {
        node_pos,
        edge_pos,
        outgoing,
        snap_grid,
    })
}

/// Validate a search request against the prepared graph (fast, O(1) checks).
fn validate_request(pg: &PreparedGraph, r: &SearchRequest) -> Result<(), RoutingError> {
    if pg.graph.release_id != r.release_id || pg.graph.vehicle_profile != r.vehicle_profile {
        return Err(invalid("incompatible release or vehicle profile"));
    }
    match (&r.origin_node_id, &r.origin) {
        (Some(_), Some(_)) | (None, None) => {
            return Err(invalid("either origin or originNodeId must be provided"));
        }
        (Some(node_id), None) => {
            if node_id.is_empty() || node_id.len() > 256 {
                return Err(invalid("invalid origin node id"));
            }
        }
        (None, Some(origin)) => {
            if !origin.lat.is_finite()
                || !origin.lon.is_finite()
                || !(-90.0..=90.0).contains(&origin.lat)
                || !(-180.0..=180.0).contains(&origin.lon)
            {
                return Err(invalid("coordinates out of range or non-finite"));
            }
        }
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
    Ok(())
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Resolve a list of edge IDs to edge references, using a pre-built index map.
fn path_from_index<'a>(
    g: &'a Graph,
    edge_pos: &HashMap<String, usize>,
    ids: &[String],
) -> Result<Vec<&'a Edge>, RoutingError> {
    if ids.len() > 2000 {
        return Err(invalid("connection path exceeds 2000 edges"));
    }
    let edges: Vec<_> = ids
        .iter()
        .map(|id| {
            edge_pos
                .get(id.as_str())
                .map(|&i| &g.edges[i])
                .ok_or_else(|| invalid("unknown path edge"))
        })
        .collect::<Result<_, _>>()?;
    if edges.windows(2).any(|w| w[0].to != w[1].from) {
        return Err(invalid("disconnected path"));
    }
    Ok(edges)
}

/// Resolve a list of edge IDs to edge references via a [`PreparedGraph`].
fn path_pg<'pg>(pg: &'pg PreparedGraph, ids: &[String]) -> Result<Vec<&'pg Edge>, RoutingError> {
    if ids.len() > 2000 {
        return Err(invalid("connection path exceeds 2000 edges"));
    }
    let edges: Vec<_> = ids
        .iter()
        .map(|id| {
            if pg.has_edge(id.as_str()) {
                Ok(pg.edge(id.as_str()))
            } else {
                Err(invalid("unknown path edge"))
            }
        })
        .collect::<Result<_, _>>()?;
    if edges.windows(2).any(|w| w[0].to != w[1].from) {
        return Err(invalid("disconnected path"));
    }
    Ok(edges)
}

fn allowed_pg(pg: &PreparedGraph, edges: &[&Edge]) -> bool {
    !pg.graph.forbidden_transitions.iter().any(|seq| {
        edges.len() >= seq.len()
            && edges
                .windows(seq.len())
                .any(|w| w.iter().zip(seq).all(|(e, id)| e.id == *id))
    })
}

// ---------------------------------------------------------------------------
// Reachability Dijkstra (Task C')
// ---------------------------------------------------------------------------

/// BFS/Dijkstra over Shutoko edges to find all node indices reachable from
/// `anchor` within `max_seconds`.
///
/// Returns a `BTreeSet<usize>` of node indices (into `pg.graph.nodes`).
/// Using indices rather than ID strings makes the set cheap to clone and
/// makes membership checks faster (integer vs. string comparison).
fn shutoko_reachable_set_pg(pg: &PreparedGraph, anchor: &str, max_seconds: u64) -> BTreeSet<usize> {
    use std::cmp::Reverse;
    let anchor_idx = pg.index.node_pos[anchor];
    let mut dist: HashMap<usize, u64> = HashMap::new();
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::new();
    dist.insert(anchor_idx, 0);
    heap.push(Reverse((0, anchor_idx)));
    while let Some(Reverse((cost, node_idx))) = heap.pop() {
        if cost > dist.get(&node_idx).copied().unwrap_or(u64::MAX) {
            continue;
        }
        let node_id = pg.graph.nodes[node_idx].id.as_str();
        if let Some(outgoing_idxs) = pg.index.outgoing.get(node_id) {
            for &edge_idx in outgoing_idxs {
                let e = &pg.graph.edges[edge_idx];
                if e.kind != EdgeKind::Shutoko {
                    continue;
                }
                let new_cost = cost + e.duration_seconds;
                if new_cost <= max_seconds {
                    let to_idx = pg.index.node_pos[e.to.as_str()];
                    let entry = dist.entry(to_idx).or_insert(u64::MAX);
                    if new_cost < *entry {
                        *entry = new_cost;
                        heap.push(Reverse((new_cost, to_idx)));
                    }
                }
            }
        }
    }
    dist.into_keys().collect()
}

/// Retrieve the Shutoko reachability set for `anchor` at `max_seconds`,
/// using a per-`PreparedGraph` cache to avoid redundant computation across
/// repeated `search_prepared` calls with the same time window.
fn cached_reachable_set(pg: &PreparedGraph, anchor: &str, max_seconds: u64) -> BTreeSet<usize> {
    let key = (anchor.to_owned(), max_seconds);
    {
        let cache = pg.reachable_cache.borrow();
        if let Some(cached) = cache.get(&key) {
            return cached.clone(); // BTreeSet<usize>: cheap to clone
        }
    }
    // Not cached — compute, insert, and return a clone.
    let set = shutoko_reachable_set_pg(pg, anchor, max_seconds);
    pg.reachable_cache.borrow_mut().insert(key, set.clone());
    set
}

// ---------------------------------------------------------------------------
// Loop/path enumeration
// ---------------------------------------------------------------------------

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
fn paths_pg<'pg>(
    pg: &'pg PreparedGraph,
    start: &str,
    end: &str,
    kind: EdgeKind,
    depth: usize,
    max_seconds: u64,
    // Reachable node indices from the anchor via `kind`-edges within max_seconds.
    reachable: &BTreeSet<usize>,
    budget: &mut Budget,
) -> Vec<Vec<&'pg Edge>> {
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
            for e in pg.outgoing_edges(at).filter(|e| e.kind == kind) {
                if !budget.take(&pg.limits) {
                    return results;
                }
                // Skip transitions to nodes not reachable from anchor within
                // the time budget (pre-filter from BFS/Dijkstra).
                let to_idx = pg.index.node_pos[e.to.as_str()];
                if !reachable.contains(&to_idx) {
                    continue;
                }
                if current.iter().map(|e| e.duration_seconds).sum::<u64>() + e.duration_seconds
                    > max_seconds
                {
                    continue;
                }
                let mut candidate = current.clone();
                candidate.push(e);
                if !allowed_pg(pg, &candidate) {
                    continue;
                }
                if e.to == end {
                    if results.len() == pg.limits.beam_width
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
                if next.len() < pg.limits.beam_width {
                    next.push(candidate);
                } else {
                    budget.truncated = true;
                }
            }
        }
        if next.is_empty() {
            break;
        }
        // Sort frontier by (accumulated time ASC, edge-ID sequence ASC) for
        // deterministic, time-ordered expansion on large graphs.
        next.sort_by(|a, b| {
            let ta: u64 = a.iter().map(|e| e.duration_seconds).sum();
            let tb: u64 = b.iter().map(|e| e.duration_seconds).sum();
            ta.cmp(&tb).then_with(|| {
                let ia: Vec<&str> = a.iter().map(|e| e.id.as_str()).collect();
                let ib: Vec<&str> = b.iter().map(|e| e.id.as_str()).collect();
                ia.cmp(&ib)
            })
        });
        frontier = next;
    }
    results
}

// ---------------------------------------------------------------------------
// Dijkstra local-road search
// ---------------------------------------------------------------------------

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
    forbidden_transitions: &[Vec<String>],
    history: &[HistoryNode<'a>],
    my_history_index: Option<usize>,
    next_edge: &'a Edge,
) -> bool {
    for seq in forbidden_transitions {
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
fn dijkstra_local_forward_pg<'pg>(
    pg: &'pg PreparedGraph,
    origin: &'pg str,
    target_entry_edges: &[&'pg Edge],
    max_local_edges: usize,
    max_seconds: u64,
    budget: &mut Budget,
) -> HashMap<&'pg str, Vec<&'pg Edge>> {
    let ft = &pg.graph.forbidden_transitions;
    let max_forbidden_len = ft.iter().map(|s| s.len()).max().unwrap_or(0);
    let history_len = max_forbidden_len.saturating_sub(1);

    let mut entry_by_from: BTreeMap<&'pg str, Vec<&'pg Edge>> = BTreeMap::new();
    for e in target_entry_edges {
        entry_by_from.entry(e.from.as_str()).or_default().push(e);
    }
    let total_targets = target_entry_edges.len();

    let mut best_for_entry: HashMap<&'pg str, Vec<&'pg Edge>> = HashMap::new();
    let mut best_cost: HashMap<(&'pg str, Vec<&'pg str>), (u64, u64)> = HashMap::new();
    let mut history: Vec<HistoryNode<'pg>> = Vec::new();
    let mut pq: BinaryHeap<DijkstraState<'pg>> = BinaryHeap::new();

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

        if !budget.take(&pg.limits) {
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
                    && transition_allowed_forward(ft, &history, my_history_index, entry_e)
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

        // Radius limit: do not expand outgoing local edges if the current node
        // is already beyond max_access_radius_meters (0.0 = unlimited).
        if pg.limits.max_access_radius_meters > 0.0 {
            let o = pg.node(origin);
            let c = pg.node(current.node);
            if distance_meters(o.lat, o.lon, c.lat, c.lon) > pg.limits.max_access_radius_meters {
                continue;
            }
        }

        if current.depth >= max_local_edges {
            continue;
        }

        for e in pg
            .outgoing_edges(current.node)
            .filter(|e| e.kind == EdgeKind::Local)
        {
            let next_d = current.duration_seconds + e.duration_seconds;
            if next_d > max_seconds {
                continue;
            }
            let next_m = current.distance_meters + e.distance_meters;

            if !transition_allowed_forward(ft, &history, my_history_index, e) {
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

    best_for_entry
}

#[allow(clippy::too_many_arguments)]
fn dijkstra_local_backward_pg<'pg>(
    pg: &'pg PreparedGraph,
    exit_edges: &[&'pg Edge],
    destination: &'pg str,
    max_local_edges: usize,
    max_seconds: u64,
    budget: &mut Budget,
) -> HashMap<&'pg str, Vec<&'pg Edge>> {
    let ft = &pg.graph.forbidden_transitions;
    let max_forbidden_len = ft.iter().map(|s| s.len()).max().unwrap_or(0);
    let history_len = max_forbidden_len.saturating_sub(1);

    let mut best_for_exit: HashMap<&'pg str, Vec<&'pg Edge>> = HashMap::new();

    for &exit_edge in exit_edges {
        if best_for_exit.contains_key(exit_edge.id.as_str()) {
            continue;
        }

        if exit_edge.to == destination {
            best_for_exit.insert(exit_edge.id.as_str(), Vec::new());
            continue;
        }

        let mut best_cost: HashMap<(&'pg str, Vec<&'pg str>), (u64, u64)> = HashMap::new();
        let mut history: Vec<HistoryNode<'pg>> = Vec::new();
        let mut pq: BinaryHeap<DijkstraState<'pg>> = BinaryHeap::new();

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

            if !budget.take(&pg.limits) {
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

            for e in pg
                .outgoing_edges(current.node)
                .filter(|e| e.kind == EdgeKind::Local)
            {
                let next_d = current.duration_seconds + e.duration_seconds;
                if next_d > max_seconds {
                    continue;
                }
                let next_m = current.distance_meters + e.distance_meters;

                if !transition_allowed_forward(ft, &history, my_history_index, e) {
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

    best_for_exit
}

// ---------------------------------------------------------------------------
// Scalar helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a [`PreparedGraph`] by validating `g` against `l` and constructing
/// the search index.  This is the expensive step (O(n log n + m log m));
/// call it once and reuse the result across many [`search_prepared`] calls.
pub fn prepare(g: Graph, l: &SearchLimits) -> Result<PreparedGraph, RoutingError> {
    let index = build_owned_index(&g, l)?;
    Ok(PreparedGraph {
        graph: g,
        limits: l.clone(),
        index,
        reachable_cache: RefCell::new(HashMap::new()),
    })
}

/// Execute a route search on an already-prepared graph.
///
/// All per-call index building is skipped; only the request is validated
/// (O(1)) and the search algorithm is run.
pub fn search_prepared(
    pg: &PreparedGraph,
    r: &SearchRequest,
) -> Result<SearchResult, RoutingError> {
    validate_request(pg, r)?;
    let now = utc(&r.pricing_at)?;

    // Resolve the origin.
    let (origin_label, snapped) = match (&r.origin_node_id, &r.origin) {
        (Some(id), None) => {
            if !pg.has_node(id.as_str()) {
                return Err(invalid("unknown origin node"));
            }
            let node = pg.node(id.as_str());
            (
                id.clone(),
                SnappedOrigin {
                    node_id: id.clone(),
                    lat: node.lat,
                    lon: node.lon,
                    distance_meters: 0.0,
                },
            )
        }
        (None, Some(ll)) => match pg.index.snap_grid.nearest(ll.lat, ll.lon, &pg.graph.nodes) {
            Some((d, idx)) => {
                let node = &pg.graph.nodes[idx];
                (
                    node.id.clone(),
                    SnappedOrigin {
                        node_id: node.id.clone(),
                        lat: node.lat,
                        lon: node.lon,
                        distance_meters: d,
                    },
                )
            }
            None => {
                return Ok(SearchResult {
                    request_id: r.request_id.clone(),
                    release_id: r.release_id.clone(),
                    status: "no_candidates".into(),
                    reason: Some("NO_CONNECTION".into()),
                    ranking_mode: "shutoko_time".into(),
                    expanded_states: 0,
                    candidates: Vec::new(),
                })
            }
        },
        _ => return Err(invalid("origin resolution state unreachable")),
    };
    let origin_node_id = origin_label.as_str();

    // Build a node lookup map (borrowed from pg.graph.nodes) once and reuse
    // it for all handoff::select_waypoints calls inside the 'pairs loop below.
    let nodes_map: BTreeMap<&str, &Node> =
        pg.graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut budget = Budget::default();
    let mut candidates = Vec::new();
    let mut candidate_edges = 0;
    let mut pairs: Vec<_> = pg
        .graph
        .billing_pairs
        .iter()
        .filter(|p| p.status == VerificationStatus::Verified)
        .collect();
    pairs.sort_by(|a, b| a.id.cmp(&b.id));
    if pairs.len() > pg.limits.max_pairs {
        budget.truncated = true;
        pairs.truncate(pg.limits.max_pairs);
    }
    let any_pairs = !pairs.is_empty();
    let mut connection = false;
    let mut found_loop = false;
    let mut legal_route = false;
    let mut time_rejected = false;
    let mut handoff_rejected = false;

    let mut verified_pairs: Vec<(&BillingPair, Vec<&Edge>, Vec<&Edge>)> =
        Vec::with_capacity(pairs.len());
    let mut entry_edges: Vec<&Edge> = Vec::with_capacity(pairs.len());
    let mut exit_edges: Vec<&Edge> = Vec::with_capacity(pairs.len());
    for p in pairs {
        let pre = path_pg(pg, &p.entry_to_anchor_edge_ids)?;
        let post = path_pg(pg, &p.anchor_to_exit_edge_ids)?;
        entry_edges.push(pre[0]);
        exit_edges.push(post.last().copied().unwrap());
        verified_pairs.push((p, pre, post));
    }
    entry_edges.sort_by_key(|e| e.id.as_str());
    entry_edges.dedup_by_key(|e| e.id.as_str());
    exit_edges.sort_by_key(|e| e.id.as_str());
    exit_edges.dedup_by_key(|e| e.id.as_str());

    let forward_map = dijkstra_local_forward_pg(
        pg,
        origin_node_id,
        &entry_edges,
        pg.limits.max_local_edges,
        r.max_minutes * 60,
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

    let backward_map = dijkstra_local_backward_pg(
        pg,
        &active_exit_edges,
        origin_node_id,
        pg.limits.max_local_edges,
        r.max_minutes * 60,
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

        // Task C': use the cached reachable set to avoid recomputing Dijkstra
        // for the same (anchor, max_seconds) across multiple search_prepared calls.
        let reachable = cached_reachable_set(pg, p.anchor_node_id.as_str(), r.max_minutes * 60);

        let loops = paths_pg(
            pg,
            &p.anchor_node_id,
            &p.anchor_node_id,
            EdgeKind::Shutoko,
            pg.limits.max_loop_edges,
            r.max_minutes * 60,
            &reachable,
            &mut budget,
        );
        found_loop |= !loops.is_empty();

        for cycle in &loops {
            if !budget.take(&pg.limits) {
                break 'pairs;
            }
            let highway: Vec<_> = pre.iter().chain(cycle).chain(&post).copied().collect();
            let all: Vec<_> = access
                .iter()
                .chain(&highway)
                .chain(returns)
                .copied()
                .collect();
            if !allowed_pg(pg, &all) {
                continue;
            }
            legal_route = true;
            let base = seconds(&all);
            let buffer = 300.max(base.div_ceil(5));
            if base < r.min_minutes * 60 || base + buffer > r.max_minutes * 60 {
                time_rejected = true;
                continue;
            }
            if candidates.len() == pg.limits.beam_width || candidate_edges + all.len() > 20_000 {
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
            let id = std::iter::once(p.id.as_str())
                .chain(ids.iter().map(String::as_str))
                .map(|s| format!("{}:{}", s.len(), s))
                .collect::<String>();

            let mut coordinates: Vec<[f64; 2]> = Vec::with_capacity(all.len() + 1);
            if let Some(first) = all.first() {
                let n = pg.node(first.from.as_str());
                coordinates.push([n.lon, n.lat]);
            }
            for e in &all {
                let n = pg.node(e.to.as_str());
                coordinates.push([n.lon, n.lat]);
            }

            let mut road_names: Vec<String> = Vec::new();
            for e in &highway {
                if let Some(name) = &e.name {
                    if !road_names.iter().any(|n| n == name) {
                        road_names.push(name.clone());
                    }
                }
            }

            let departure = r.origin.clone().unwrap_or(LatLng {
                lat: snapped.lat,
                lon: snapped.lon,
            });
            let waypoints =
                handoff::select_waypoints(&p.anchor_node_id, cycle, exit_edge, &nodes_map);
            let maps_url = match handoff::format_maps_url(&departure, &waypoints) {
                Ok(url) => url,
                Err(()) => {
                    handoff_rejected = true;
                    continue;
                }
            };
            let handoff_payload = Handoff {
                origin: departure.clone(),
                destination: departure,
                waypoints,
                maps_url,
                verification_set_version: None,
            };

            candidates.push(Candidate {
                id,
                release_id: r.release_id.clone(),
                origin: r.origin.clone(),
                origin_node_id: origin_label.clone(),
                snapped_origin: snapped.clone(),
                entry: RampInfo {
                    edge_id: p.entry_id.clone(),
                    name: p.entry_name.clone(),
                },
                exit: RampInfo {
                    edge_id: p.exit_id.clone(),
                    name: p.exit_name.clone(),
                },
                entry_id: p.entry_id.clone(),
                exit_id: p.exit_id.clone(),
                road_names,
                edge_ids: ids,
                geometry: GeoJsonLineString {
                    r#type: "LineString".into(),
                    coordinates,
                },
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
                reasons: Vec::new(),
                warnings: vec![
                    "STATIC_TRAVEL_TIME".into(),
                    "HANDOFF_WAYPOINTS_UNVERIFIED".into(),
                ],
                handoff: handoff_payload,
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
        if selected.iter().any(|s| similar_pg(pg, &c, s)) {
            continue;
        }
        selected.push(c);
        if selected.len() == pg.limits.max_candidates {
            break;
        }
    }
    for (i, c) in selected.iter_mut().enumerate() {
        c.reasons = if i == 0 {
            vec![
                if time_ranking {
                    "BEST_SHUTOKO_TIME".to_string()
                } else {
                    "BEST_TIME_PER_YEN".to_string()
                },
                "ONE_SECTION_TOLL".to_string(),
            ]
        } else {
            vec!["ONE_SECTION_TOLL".to_string()]
        };
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
    } else if handoff_rejected && !time_rejected {
        Some("NO_HANDOFF")
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

fn similar_pg(pg: &PreparedGraph, a: &Candidate, b: &Candidate) -> bool {
    let set = |c: &Candidate| -> BTreeSet<String> {
        c.edge_ids
            .iter()
            .filter(|id| pg.edge(id.as_str()).kind != EdgeKind::Local)
            .cloned()
            .collect()
    };
    let sa = set(a);
    let sb = set(b);
    let shared: u64 = sa
        .intersection(&sb)
        .map(|id| pg.edge(id.as_str()).distance_meters)
        .sum();
    let union: u64 = sa
        .union(&sb)
        .map(|id| pg.edge(id.as_str()).distance_meters)
        .sum();
    u128::from(shared) * 5 >= u128::from(union) * 4
}

/// Search synthetic or prevalidated graphs. Returned routes are experimental
/// and have no Maps handoff.
///
/// This is a thin wrapper over [`prepare`] + [`search_prepared`] that
/// preserves the original signature for backward compatibility.
pub fn search(
    g: &Graph,
    r: &SearchRequest,
    l: &SearchLimits,
) -> Result<SearchResult, RoutingError> {
    let pg = prepare(g.clone(), l)?;
    search_prepared(&pg, r)
}

/// Parse and validate strict JSON, then serialize the search response.
///
/// Internally uses [`prepare`] + [`search_prepared`] to avoid cloning the
/// already-parsed graph.
pub fn search_json(
    graph_json: &str,
    request_json: &str,
    limits_json: &str,
) -> Result<String, RoutingError> {
    // 512 MiB: raised from 64 MiB to accommodate large real-world graphs
    // (metropolitan highway networks can exceed 100k nodes × ~10 edges each).
    const MAX_GRAPH_JSON_BYTES: usize = 512 * 1024 * 1024;
    if graph_json.len() > MAX_GRAPH_JSON_BYTES
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
    // g is moved into prepare() — no extra clone compared with the old path.
    let pg = prepare(g, &l)?;
    serde_json::to_string(&search_prepared(&pg, &r)?)
        .map_err(|_| invalid("result serialization failed"))
}

/// Build a [`PreparedGraph`] from JSON strings.
///
/// Parses `graph_json` and `limits_json`, validates, and constructs the
/// search index.  The returned `PreparedGraph` can be passed to
/// [`search_prepared_json`] (or [`search_prepared`]) for fast repeated search.
pub fn prepare_json(graph_json: &str, limits_json: &str) -> Result<PreparedGraph, RoutingError> {
    const MAX_GRAPH_JSON_BYTES: usize = 512 * 1024 * 1024;
    if graph_json.len() > MAX_GRAPH_JSON_BYTES || limits_json.len() > 4096 {
        return Err(invalid("JSON payload exceeds prototype size limit"));
    }
    let g: Graph = serde_json::from_str(graph_json).map_err(|_| invalid("invalid graph JSON"))?;
    let l: SearchLimits =
        serde_json::from_str(limits_json).map_err(|_| invalid("invalid limits JSON"))?;
    prepare(g, &l)
}

/// Execute a search on a [`PreparedGraph`] using a JSON request string.
///
/// Complements [`prepare_json`]: parse the request, run the search, and
/// serialize the result — all without re-building the graph index.
pub fn search_prepared_json(
    pg: &PreparedGraph,
    request_json: &str,
) -> Result<String, RoutingError> {
    if request_json.len() > 16 * 1024 {
        return Err(invalid("JSON payload exceeds prototype size limit"));
    }
    let r: SearchRequest =
        serde_json::from_str(request_json).map_err(|_| invalid("invalid request JSON"))?;
    serde_json::to_string(&search_prepared(pg, &r)?)
        .map_err(|_| invalid("result serialization failed"))
}
