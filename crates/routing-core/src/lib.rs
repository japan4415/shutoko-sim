//! Experimental, bounded routing on explicitly connected directed graphs.
//! This engine does not establish real-world toll eligibility or navigation safety.
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::fmt;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// Routing engine version, reported as `manifest.engineVersion` so that the
/// manifest always records the search-engine version (not the builder's own).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Snap search radius used by the spatial grid for cell-size computation.
/// Kept here so `grid.rs` can import it via `crate::SNAP_RADIUS_METERS`.
pub const SNAP_RADIUS_METERS: f64 = 200.0;

/// Detour factor for Tokyo urban areas. Straight-line distances underestimate
/// actual driving distance due to the dense grid of one-way streets and turns.
/// A value of 1.3 is a conservative estimate for the Tokyo metropolitan area.
const DETOUR_FACTOR: f64 = 1.3;

/// Assumed driving speed for surface streets approaching/leaving the expressway.
/// 30 km/h = 8.333… m/s.
///
/// We use the equirectangular `distance_meters` approximation for the straight-line
/// leg; it is accurate to <0.1 % at Tokyo's latitude for distances under 50 km,
/// which is sufficient for access-time estimation.
const ACCESS_SPEED_MPS: f64 = 30.0 / 3.6;

/// Estimate one-way access or return time in seconds given a straight-line distance.
///
/// Formula: ⌈distance × DETOUR_FACTOR / ACCESS_SPEED_MPS⌉
fn estimated_access_seconds(dist_m: f64) -> u64 {
    ((dist_m * DETOUR_FACTOR) / ACCESS_SPEED_MPS).ceil() as u64
}

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
    /// Retained for graph-file compatibility with older fixtures.
    /// graph-builder no longer emits Local edges; the routing engine never
    /// traverses them.
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
    /// Maximum number of Entry access points to try for coordinate-input searches.
    /// `k_nearest()` returns at most this many Entry from-nodes, sorted by distance
    /// (ascending), with lex-smaller ID as a tie-break.
    ///
    /// `0` means **unlimited**: every Entry access point in the graph is a candidate.
    /// Defaults to `0` (unlimited — all entries).
    ///
    /// C1 has only 16 Entry from-nodes; distance-based truncation never helps and
    /// silently drops valid candidates when the nearest k nodes are not billing-pair
    /// origins.  Set to a positive value only when you explicitly want to restrict
    /// the search to the N nearest entry points.
    pub max_access_entries: usize,
    pub max_pairs: usize,
    pub max_candidates: usize,
    /// Maximum number of nodes allowed in the graph. Defaults to 1,000,000.
    pub max_graph_nodes: usize,
    /// Maximum number of edges allowed in the graph. Defaults to 3,000,000.
    pub max_graph_edges: usize,
    /// Maximum straight-line distance (metres) from the user's coordinate to the
    /// **nearest** Entry access point.  If the nearest entry is farther than this
    /// value, the search immediately returns `NO_CONNECTION` without exploring any
    /// billing pairs.
    ///
    /// `0.0` means **unlimited** (no distance cap applied).
    /// Must be a non-negative finite number; negative values, `NaN`, and `Infinity`
    /// are rejected by `validate()`.
    /// Defaults to `30_000.0` (30 km).
    pub max_access_distance_meters: f64,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_expanded_states: 100_000,
            beam_width: 200,
            max_loop_edges: 2000,
            // 0 = unlimited: all Entry access points are candidates.
            max_access_entries: 0,
            max_pairs: 10,
            max_candidates: 3,
            max_graph_nodes: 1_000_000,
            max_graph_edges: 3_000_000,
            // 30 km: beyond this the engine is outside its operational area.
            max_access_distance_meters: 30_000.0,
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
    /// Node ID of the Entry access point used for this candidate.
    /// Each candidate may use a different entry access point.
    pub node_id: String,
    pub lat: f64,
    pub lon: f64,
    /// Straight-line distance (metres) from the user's origin to this access point.
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
    /// Entry access point for this candidate.  Candidates from the same
    /// coordinate-input search may have different `snapped_origin` values if
    /// they are reached via different Entry access points.
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
    /// Nearest Entry access point to the coordinate origin, reported
    /// independently of whether a candidate was produced (including the
    /// cap-exceeded `NO_CONNECTION` early return).
    ///
    /// `None` for `originNodeId` input (no snapping happens) and when the
    /// graph has no Entry access points at all.
    #[serde(default)]
    pub nearest_access: Option<SnappedOrigin>,
    /// Shortest `plan_seconds` (`base + buffer`) over every legal loop found,
    /// including loops rejected by the requested time window.  It is the
    /// numeric basis for a `TIME_WINDOW` rejection.
    ///
    /// `None` when no legal loop exists (e.g. `NO_CONNECTION`, `NO_LOOP`, or a
    /// search that never reached the loop-enumeration stage).
    #[serde(default)]
    pub min_plan_seconds: Option<u64>,
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
    /// Spatial index over Entry edge from-nodes for fast coordinate → nearest
    /// access-point snapping.
    snap_grid: grid::OwnedSnapGrid,
}

/// A prepared graph: owns the graph data plus pre-built search indices.
///
/// Create with [`prepare`]; then pass to [`search_prepared`] as many times as
/// needed. Building the index is O(n log n + m log m) and happens **once**;
/// each subsequent search skips that cost entirely.
pub struct PreparedGraph {
    /// The owned graph data.
    ///
    /// `pub(crate)` instead of `pub` to prevent external mutation.  External
    /// consumers should use the [`PreparedGraph::graph`] accessor instead.
    /// Because `OwnedIndex` stores positional indices (usize) into
    /// `graph.nodes` / `graph.edges`, any external push/remove would silently
    /// corrupt lookups and the reachable cache.
    pub(crate) graph: Graph,
    /// Search limits used during preparation (re-validated on each search).
    ///
    /// `pub(crate)` to keep `PreparedGraph` opaque outside the crate.
    pub(crate) limits: SearchLimits,
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
    /// Read-only access to the graph data.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Read-only access to the search limits stored at prepare time.
    pub fn limits(&self) -> &SearchLimits {
        &self.limits
    }

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
        // max_access_entries: 0 = unlimited (all entries); positive values are capped at 50.
        || (l.max_access_entries != 0 && l.max_access_entries > 50)
        || l.max_pairs == 0
        || l.max_pairs > 100
        || l.max_candidates == 0
        || l.max_candidates > 3
    {
        return Err(invalid("search limits outside supported bounds"));
    }
    // max_access_distance_meters: must be non-negative and finite.
    // 0.0 = unlimited (no distance cap).
    if !l.max_access_distance_meters.is_finite() || l.max_access_distance_meters < 0.0 {
        return Err(invalid(
            "max_access_distance_meters must be finite and non-negative (0.0 = unlimited)",
        ));
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

    // Build edge_pos, outgoing, and collect Entry from-node IDs for the snap grid.
    // Using a BTreeSet so iteration is in lex order, giving deterministic
    // tie-breaking in OwnedSnapGrid.
    let mut edge_pos: HashMap<String, usize> = HashMap::with_capacity(g.edges.len());
    let mut outgoing: HashMap<String, Vec<usize>> = HashMap::with_capacity(g.nodes.len());
    let mut entry_from_ids: BTreeSet<String> = BTreeSet::new();

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
        if e.kind == EdgeKind::Entry {
            entry_from_ids.insert(e.from.clone());
        }
        outgoing.entry(e.from.clone()).or_default().push(i);
    }

    // Sort outgoing adjacency lists by edge ID for deterministic expansion order.
    for v in outgoing.values_mut() {
        v.sort_by(|&a, &b| g.edges[a].id.cmp(&g.edges[b].id));
    }

    // Build snap grid from Entry from-node indices.
    let entry_from_indices: Vec<usize> = entry_from_ids
        .iter()
        .map(|id| node_pos[id.as_str()])
        .collect();
    let snap_grid = grid::OwnedSnapGrid::build(entry_from_indices, &g.nodes);

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

// Iterative simple-path enumeration. The same global budget includes paths and combinations.
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

    // ---------------------------------------------------------------------------
    // Origin resolution: determine user departure coordinates and access candidates.
    //
    // For `originNodeId` input: the specified node is the sole Entry access candidate
    // (backward-compatible behaviour).
    //
    // For `origin` (coordinate) input:
    //   1. `k_nearest` returns Entry from-nodes sorted by distance (lex-smaller ID as
    //      tie-break).  When `max_access_entries == 0` (unlimited, the default), all
    //      Entry from-nodes in the graph are candidates.
    //   2. If the snap grid is empty (no Entry edges at all), `NO_CONNECTION` is returned.
    //   3. If `max_access_distance_meters > 0.0` and the nearest entry exceeds that
    //      distance, `NO_CONNECTION` is returned (out-of-service-area guard).  The
    //      nearest Entry access point is still reported in `nearestAccess`.
    // ---------------------------------------------------------------------------
    let (origin_ll, access_node_indices, nearest_access): (
        LatLng,
        BTreeSet<usize>,
        Option<SnappedOrigin>,
    ) = match (&r.origin_node_id, &r.origin) {
        (Some(id), None) => {
            if !pg.has_node(id.as_str()) {
                return Err(invalid("unknown origin node"));
            }
            let node = pg.node(id.as_str());
            let origin_ll = LatLng {
                lat: node.lat,
                lon: node.lon,
            };
            let idx = pg.index.node_pos[id.as_str()];
            (origin_ll, std::iter::once(idx).collect(), None)
        }
        (None, Some(ll)) => {
            // max_access_entries == 0 means "unlimited": pass usize::MAX so that
            // k_nearest returns every entry in the graph.
            let k = if pg.limits.max_access_entries == 0 {
                usize::MAX
            } else {
                pg.limits.max_access_entries
            };
            let results = pg
                .index
                .snap_grid
                .k_nearest(ll.lat, ll.lon, k, &pg.graph.nodes);
            if results.is_empty() {
                // Snap grid is empty — no Entry edges in the graph.
                return Ok(SearchResult {
                    request_id: r.request_id.clone(),
                    release_id: r.release_id.clone(),
                    status: "no_candidates".into(),
                    reason: Some("NO_CONNECTION".into()),
                    ranking_mode: "shutoko_time".into(),
                    expanded_states: 0,
                    candidates: Vec::new(),
                    nearest_access: None,
                    min_plan_seconds: None,
                });
            }
            // Nearest Entry access point, independent of the distance cap below.
            let (nearest_dist, nearest_idx) = results[0];
            let nearest_node = &pg.graph.nodes[nearest_idx];
            let nearest_access = Some(SnappedOrigin {
                node_id: nearest_node.id.clone(),
                lat: nearest_node.lat,
                lon: nearest_node.lon,
                distance_meters: nearest_dist,
            });
            // Distance sanity cap: if even the nearest Entry access point exceeds the
            // allowed maximum, the origin is outside the operational area.
            // 0.0 means unlimited (no cap applied).
            if pg.limits.max_access_distance_meters > 0.0
                && nearest_dist > pg.limits.max_access_distance_meters
            {
                return Ok(SearchResult {
                    request_id: r.request_id.clone(),
                    release_id: r.release_id.clone(),
                    status: "no_candidates".into(),
                    reason: Some("NO_CONNECTION".into()),
                    ranking_mode: "shutoko_time".into(),
                    expanded_states: 0,
                    candidates: Vec::new(),
                    nearest_access,
                    min_plan_seconds: None,
                });
            }
            let indices = results.into_iter().map(|(_, idx)| idx).collect();
            (ll.clone(), indices, nearest_access)
        }
        _ => return Err(invalid("origin resolution state unreachable")),
    };

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
    // Shortest plan_seconds among legal loops, including loops the requested
    // time window rejects. Reported as `minPlanSeconds`.
    let mut min_plan_seconds: Option<u64> = None;

    let mut verified_pairs: Vec<(&BillingPair, Vec<&Edge>, Vec<&Edge>)> =
        Vec::with_capacity(pairs.len());
    for p in pairs {
        let pre = path_pg(pg, &p.entry_to_anchor_edge_ids)?;
        let post = path_pg(pg, &p.anchor_to_exit_edge_ids)?;
        verified_pairs.push((p, pre, post));
    }

    'pairs: for (p, pre, post) in verified_pairs {
        let entry_edge = pre[0];
        let exit_edge = post.last().unwrap();

        // Only explore this billing pair if its Entry from-node is an accessible
        // candidate (i.e. it was returned by k_nearest or is the specified node).
        let entry_from_idx = pg.index.node_pos[entry_edge.from.as_str()];
        if !access_node_indices.contains(&entry_from_idx) {
            continue;
        }
        connection = true;

        let access_node = &pg.graph.nodes[entry_from_idx];
        // Straight-line distance from user's departure to the Entry access point.
        // Using equirectangular approximation (distance_meters): accurate <0.1%
        // within 50 km at Tokyo's latitude — sufficient for access-time estimation.
        let access_dist = distance_meters(
            origin_ll.lat,
            origin_ll.lon,
            access_node.lat,
            access_node.lon,
        );
        let access_secs = estimated_access_seconds(access_dist);

        let exit_to_node = pg.node(exit_edge.to.as_str());
        // Return: straight-line distance from the expressway exit node to the
        // user's departure point.
        let return_dist = distance_meters(
            exit_to_node.lat,
            exit_to_node.lon,
            origin_ll.lat,
            origin_ll.lon,
        );
        let return_secs = estimated_access_seconds(return_dist);

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
            if !allowed_pg(pg, &highway) {
                continue;
            }
            legal_route = true;
            let base = access_secs + seconds(&highway) + return_secs;
            let buffer = 300.max(base.div_ceil(5));
            let plan_seconds = base + buffer;
            min_plan_seconds = Some(min_plan_seconds.map_or(plan_seconds, |m| m.min(plan_seconds)));
            if base < r.min_minutes * 60 || base + buffer > r.max_minutes * 60 {
                time_rejected = true;
                continue;
            }
            if candidates.len() == pg.limits.beam_width || candidate_edges + highway.len() > 20_000
            {
                budget.truncated = true;
                break 'pairs;
            }
            candidate_edges += highway.len();
            let price = p.prices.iter().find(|v| {
                utc(&v.effective_from).is_ok_and(|from| from <= now)
                    && v.effective_to
                        .as_deref()
                        .is_none_or(|to| utc(to).is_ok_and(|to| now < to))
            });
            let ids = edge_ids(&highway);
            let id = std::iter::once(p.id.as_str())
                .chain(ids.iter().map(String::as_str))
                .map(|s| format!("{}:{}", s.len(), s))
                .collect::<String>();

            let mut coordinates: Vec<[f64; 2]> = Vec::with_capacity(highway.len() + 1);
            if let Some(first) = highway.first() {
                let n = pg.node(first.from.as_str());
                coordinates.push([n.lon, n.lat]);
            }
            for e in &highway {
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

            // Candidate's snapped_origin: the Entry access point for this candidate.
            // Each candidate may have a different access point when coordinate input
            // is used with multiple nearby entries (k_nearest).
            // distance_meters is the straight-line distance from the user's origin.
            let snapped_origin = SnappedOrigin {
                node_id: entry_edge.from.clone(),
                lat: access_node.lat,
                lon: access_node.lon,
                distance_meters: access_dist,
            };

            let departure = r.origin.clone().unwrap_or_else(|| origin_ll.clone());
            let waypoints =
                handoff::select_waypoints(entry_edge.from.as_str(), cycle, exit_edge, |id| {
                    pg.index.node_pos.get(id).map(|&i| {
                        let n = &pg.graph.nodes[i];
                        LatLng {
                            lat: n.lat,
                            lon: n.lon,
                        }
                    })
                });
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
                origin_node_id: entry_edge.from.clone(),
                snapped_origin,
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
                    access_seconds: access_secs,
                    shutoko_seconds: seconds(&highway),
                    return_seconds: return_secs,
                    base_seconds: base,
                    buffer_seconds: buffer,
                    plan_seconds: base + buffer,
                },
                distance_meters: meters(&highway),
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
        nearest_access,
        min_plan_seconds,
    })
}

fn similar_pg(pg: &PreparedGraph, a: &Candidate, b: &Candidate) -> bool {
    // All edges in the candidate are now highway edges (no Local edges);
    // the former EdgeKind::Local filter is removed as it was a no-op.
    let set = |c: &Candidate| -> BTreeSet<String> { c.edge_ids.iter().cloned().collect() };
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
