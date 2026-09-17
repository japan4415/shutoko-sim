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

/// Upper bound (minutes) on the planning time the product UI can ever accept.
///
/// Mirrors `SearchRequest::max_minutes`'s validation limit (240) and the UI's
/// `max` attribute.  It is used to bound the *diagnostic* loop enumeration that
/// feeds `minPlanSeconds`: because `planSeconds >= loop time`, every legal loop
/// whose plan fits inside the product cap is enumerated by this bound, so the
/// UI can decide "can any window up to 240 minutes ever work?" without the
/// requested window silently pruning the evidence.
pub const MAX_PRODUCT_MINUTES: u64 = 240;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RampKind {
    GeneralEntry,
    GeneralExit,
    BoundaryIn,
    BoundaryOut,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ramp {
    pub id: String,
    pub facility_id: String,
    pub name: String,
    pub route: String,
    pub direction: String,
    pub kind: RampKind,
    pub edge_id: String,
    pub node_id: String,
    pub mainline_node_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restrictions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OdTariff {
    pub entry_ramp_id: String,
    pub exit_ramp_id: String,
    pub billing_distance_meters: u64,
    pub amount_yen: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_to: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ramps: Vec<Ramp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub od_tariffs: Vec<OdTariff>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_ramp_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_ramp_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_distance_meters: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_ramp_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_ramp_id: Option<String>,
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
    /// Minimum mainline loop distance (metres) required for a cycle to be valid.
    /// Excludes small JCT connectors, ramps, and spiral loops (e.g. Ohashi JCT ~1.1km).
    /// Defaults to 5,000m (5.0 km).
    pub min_loop_meters: u64,
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
            min_loop_meters: 5_000,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_distance_meters: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toll_source: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
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
    /// search that never reached the loop-enumeration stage) **and also when a
    /// resource limit cut the loop enumeration short** (beam width, expanded
    /// state budget, or the billing-pair cap).  In that case the minimum is not
    /// provable, so this stays `null`: the UI must not claim "even four hours
    /// cannot work" nor "the shortest loop takes N minutes" without proof
    /// (review TEST-01-FINAL / V1 / V2).
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

/// Compute standard ETC toll for ordinary vehicles (普通車) on Metropolitan Expressway (首都高速).
///
/// Implements the official distance-based tariff (effective 2022-04-01):
/// - Lower bound: 300 JPY (distance <= 4,300m).
/// - Formula: (150 JPY base + 29.52 JPY/km * distance_km) * 1.10 (consumption tax),
///   rounded to the nearest 10 JPY.
/// - Upper bound: 1,950 JPY (for ordinary passenger cars).
pub fn calculate_etc_toll_yen(distance_meters: u64) -> u64 {
    if distance_meters <= 4_300 {
        return 300;
    }
    let km = distance_meters as f64 / 1000.0;
    let base_with_tax = (150.0 + 29.52 * km) * 1.10;
    let rounded = (base_with_tax / 10.0).round() as u64 * 10;
    rounded.clamp(300, 1950)
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
    /// Maps node ID → sorted list of incoming Shutoko/connector edge indices.
    /// The lists use edge-ID order so reverse shortest-path searches are stable.
    incoming: HashMap<String, Vec<usize>>,
    /// Spatial index over Entry edge from-nodes for fast coordinate → nearest
    /// access-point snapping.
    snap_grid: grid::OwnedSnapGrid,
    /// Maps ramp ID → index into `graph.ramps`.
    ramp_by_id: HashMap<String, usize>,
    /// Maps edge ID → index into `graph.ramps`.
    ramp_by_edge: HashMap<String, usize>,
    /// Maps (entry_ramp_id, exit_ramp_id) → index into `graph.od_tariffs`.
    od_tariff_map: HashMap<(String, String), usize>,
    /// GeneralEntry ramp indices by access node ID, sorted by ramp ID.
    general_entries_by_node: HashMap<String, Vec<usize>>,
    /// GeneralExit ramp indices by facility ID, sorted by ramp ID.
    general_exits_by_facility: HashMap<String, Vec<usize>>,
    /// Distinct exit ramp indices referenced by Verified billing pairs, sorted by ramp ID.
    verified_pair_exit_ramps: Vec<usize>,
    /// Verified billing pair indices by entry ramp ID, sorted by pair ID.
    verified_pairs_by_entry_ramp: HashMap<String, Vec<usize>>,
    /// Strongly-connected component for every graph node. Components are built
    /// over Shutoko edges only; Entry and Exit connectors never make a cycle.
    component_by_node: Vec<usize>,
    /// Whether each component contains a directed cycle.
    component_has_cycle: Vec<bool>,
    /// Deterministic topology-derived representative anchors for every cyclic
    /// component (at most 32 per component, evenly spaced in node-ID order).
    cycle_catalog_anchors: Vec<usize>,
}

#[derive(Clone)]
struct CachedCycles {
    edge_indices: Vec<Vec<usize>>,
    expanded_states: usize,
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
    /// Topology-derived deterministic cycle catalogue, keyed by anchor node.
    /// The charged state count is cached too, keeping repeated responses byte
    /// deterministic (including `expandedStates`).
    cycle_cache: RefCell<HashMap<usize, CachedCycles>>,
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

/// Deterministic iterative Kosaraju decomposition over mainline edges.
///
/// Node and adjacency traversal use canonical IDs, so component membership and
/// every downstream cycle choice are independent of hash-map iteration order.
fn shutoko_components(
    g: &Graph,
    node_pos: &HashMap<String, usize>,
    outgoing: &HashMap<String, Vec<usize>>,
    incoming: &HashMap<String, Vec<usize>>,
) -> (Vec<usize>, Vec<bool>) {
    let mut node_order: Vec<usize> = (0..g.nodes.len()).collect();
    node_order.sort_by(|&a, &b| g.nodes[a].id.cmp(&g.nodes[b].id));

    let mut visited = vec![false; g.nodes.len()];
    let mut finish = Vec::with_capacity(g.nodes.len());
    for &root in &node_order {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut stack = vec![(root, 0usize)];
        while let Some((node_idx, next_pos)) = stack.last_mut() {
            let edges = outgoing
                .get(g.nodes[*node_idx].id.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            while *next_pos < edges.len() && g.edges[edges[*next_pos]].kind != EdgeKind::Shutoko {
                *next_pos += 1;
            }
            if *next_pos == edges.len() {
                finish.push(*node_idx);
                stack.pop();
                continue;
            }
            let edge_idx = edges[*next_pos];
            *next_pos += 1;
            let to = node_pos[g.edges[edge_idx].to.as_str()];
            if !visited[to] {
                visited[to] = true;
                stack.push((to, 0));
            }
        }
    }

    let mut component_by_node = vec![usize::MAX; g.nodes.len()];
    let mut component_has_cycle = Vec::new();
    for &root in finish.iter().rev() {
        if component_by_node[root] != usize::MAX {
            continue;
        }
        let component = component_has_cycle.len();
        let mut stack = vec![root];
        component_by_node[root] = component;
        let mut size = 0usize;
        let mut self_loop = false;
        while let Some(node_idx) = stack.pop() {
            size += 1;
            let node_id = g.nodes[node_idx].id.as_str();
            for &edge_idx in incoming.get(node_id).map(Vec::as_slice).unwrap_or(&[]) {
                let edge = &g.edges[edge_idx];
                if edge.kind != EdgeKind::Shutoko {
                    continue;
                }
                let from = node_pos[edge.from.as_str()];
                self_loop |= from == node_idx;
                if component_by_node[from] == usize::MAX {
                    component_by_node[from] = component;
                    stack.push(from);
                }
            }
        }
        component_has_cycle.push(size > 1 || self_loop);
    }
    (component_by_node, component_has_cycle)
}

/// Validate the graph structure and limits, then build the [`OwnedIndex`].
/// This is the expensive O(n log n + m log m) step; it happens once in [`prepare`].
fn build_owned_index(g: &Graph, l: &SearchLimits) -> Result<OwnedIndex, RoutingError> {
    if (g.schema_version != 2 && g.schema_version != 3)
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
        || l.max_loop_edges > 5000
        // max_access_entries: 0 = unlimited (all entries); positive values are capped at 50.
        || (l.max_access_entries != 0 && l.max_access_entries > 50)
        || l.max_pairs == 0
        || l.max_pairs > 1000
        || l.max_candidates == 0
        || l.max_candidates > 3
        || l.min_loop_meters > 50_000
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
    let mut incoming: HashMap<String, Vec<usize>> = HashMap::with_capacity(g.nodes.len());
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
        incoming.entry(e.to.clone()).or_default().push(i);
    }

    // Sort outgoing adjacency lists by edge ID for deterministic expansion order.
    for v in outgoing.values_mut() {
        v.sort_by(|&a, &b| g.edges[a].id.cmp(&g.edges[b].id));
    }
    for v in incoming.values_mut() {
        v.sort_by(|&a, &b| g.edges[a].id.cmp(&g.edges[b].id));
    }

    let (component_by_node, component_has_cycle) =
        shutoko_components(g, &node_pos, &outgoing, &incoming);
    let mut canonical_nodes: Vec<usize> = (0..g.nodes.len()).collect();
    canonical_nodes.sort_by(|&a, &b| g.nodes[a].id.cmp(&g.nodes[b].id));
    let mut component_nodes = vec![Vec::new(); component_has_cycle.len()];
    for &node in &canonical_nodes {
        component_nodes[component_by_node[node]].push(node);
    }
    let mut cycle_catalog_anchors = Vec::new();
    for (component, nodes) in component_nodes.iter().enumerate() {
        if !component_has_cycle[component] {
            continue;
        }
        if nodes.len() <= 32 {
            cycle_catalog_anchors.extend(nodes.iter().copied());
        } else {
            for sample in 0..32usize {
                let position = sample * (nodes.len() - 1) / 31;
                cycle_catalog_anchors.push(nodes[position]);
            }
        }
    }

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

    // Validate and index ramps (if present)
    let mut ramp_by_id: HashMap<String, usize> = HashMap::with_capacity(g.ramps.len());
    let mut ramp_by_edge: HashMap<String, usize> = HashMap::with_capacity(g.ramps.len());
    let mut general_entry_from_ids = BTreeSet::new();
    let mut general_entries_by_node: HashMap<String, Vec<usize>> = HashMap::new();
    let mut general_exits_by_facility: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, r) in g.ramps.iter().enumerate() {
        if r.id.is_empty() || r.id.len() > 256 || ramp_by_id.insert(r.id.clone(), i).is_some() {
            return Err(invalid("duplicate, oversized, or empty ramp id"));
        }
        let Some(&edge_idx) = edge_pos.get(r.edge_id.as_str()) else {
            return Err(invalid("ramp references unknown edge id"));
        };
        let edge = &g.edges[edge_idx];
        if !node_pos.contains_key(r.node_id.as_str())
            || !node_pos.contains_key(r.mainline_node_id.as_str())
        {
            return Err(invalid("ramp references unknown node id"));
        }
        let valid_binding = match r.kind {
            RampKind::GeneralEntry | RampKind::BoundaryIn => {
                edge.kind == EdgeKind::Entry
                    && r.node_id == edge.from
                    && r.mainline_node_id == edge.to
            }
            RampKind::GeneralExit | RampKind::BoundaryOut => {
                edge.kind == EdgeKind::Exit
                    && r.mainline_node_id == edge.from
                    && r.node_id == edge.to
            }
        };
        if !valid_binding {
            return Err(invalid("ramp kind or directed graph binding mismatch"));
        }
        if r.kind == RampKind::GeneralEntry {
            general_entry_from_ids.insert(edge.from.clone());
            general_entries_by_node
                .entry(r.node_id.clone())
                .or_default()
                .push(i);
        } else if r.kind == RampKind::GeneralExit {
            general_exits_by_facility
                .entry(r.facility_id.clone())
                .or_default()
                .push(i);
        }
        ramp_by_edge.insert(r.edge_id.clone(), i);
    }
    for list in general_entries_by_node.values_mut() {
        list.sort_by(|&a, &b| g.ramps[a].id.cmp(&g.ramps[b].id));
    }
    for list in general_exits_by_facility.values_mut() {
        list.sort_by(|&a, &b| g.ramps[a].id.cmp(&g.ramps[b].id));
    }

    let mut verified_pairs_by_entry_ramp: HashMap<String, Vec<usize>> = HashMap::new();
    let mut verified_pair_exit_ramp_set: BTreeSet<String> = BTreeSet::new();
    for (i, p) in g.billing_pairs.iter().enumerate() {
        if p.status == VerificationStatus::Verified {
            let entry_ramp_id = p.entry_ramp_id.as_deref().or_else(|| {
                ramp_by_edge
                    .get(&p.entry_id)
                    .map(|&idx| g.ramps[idx].id.as_str())
            });
            if let Some(er_id) = entry_ramp_id {
                verified_pairs_by_entry_ramp
                    .entry(er_id.to_string())
                    .or_default()
                    .push(i);
            }
            let exit_ramp_id = p.exit_ramp_id.as_deref().or_else(|| {
                ramp_by_edge
                    .get(&p.exit_id)
                    .map(|&idx| g.ramps[idx].id.as_str())
            });
            if let Some(xr_id) = exit_ramp_id {
                verified_pair_exit_ramp_set.insert(xr_id.to_string());
            }
        }
    }
    for list in verified_pairs_by_entry_ramp.values_mut() {
        list.sort_by(|&a, &b| g.billing_pairs[a].id.cmp(&g.billing_pairs[b].id));
    }
    let mut verified_pair_exit_ramps: Vec<usize> = Vec::new();
    for xr_id in verified_pair_exit_ramp_set {
        if let Some(&idx) = ramp_by_id.get(&xr_id) {
            verified_pair_exit_ramps.push(idx);
        }
    }

    // Canonical ramps are authoritative when present: only verified-bound
    // general entries become public coordinate snap targets. Legacy/synthetic
    // graphs without Ramp records retain Entry-edge snapping compatibility.
    let snap_ids = if g.ramps.is_empty() {
        &entry_from_ids
    } else {
        &general_entry_from_ids
    };
    let entry_from_indices: Vec<usize> = snap_ids.iter().map(|id| node_pos[id.as_str()]).collect();
    let snap_grid = grid::OwnedSnapGrid::build(entry_from_indices, &g.nodes);

    // Validate and index od_tariffs (if present)
    let mut od_tariff_map: HashMap<(String, String), usize> =
        HashMap::with_capacity(g.od_tariffs.len());
    for (i, t) in g.od_tariffs.iter().enumerate() {
        if t.entry_ramp_id.is_empty() || t.exit_ramp_id.is_empty() {
            return Err(invalid("empty ramp id in od_tariff"));
        }
        let key = (t.entry_ramp_id.clone(), t.exit_ramp_id.clone());
        od_tariff_map.insert(key, i);
    }

    Ok(OwnedIndex {
        node_pos,
        edge_pos,
        outgoing,
        incoming,
        snap_grid,
        ramp_by_id,
        ramp_by_edge,
        od_tariff_map,
        general_entries_by_node,
        general_exits_by_facility,
        verified_pair_exit_ramps,
        verified_pairs_by_entry_ramp,
        component_by_node,
        component_has_cycle,
        cycle_catalog_anchors,
    })
}

/// Validate a search request against the prepared graph (fast, O(1) checks).
fn validate_request(pg: &PreparedGraph, r: &SearchRequest) -> Result<(), RoutingError> {
    if pg.graph.release_id != r.release_id || pg.graph.vehicle_profile != r.vehicle_profile {
        return Err(invalid("incompatible release or vehicle profile"));
    }
    match (&r.origin_node_id, &r.origin, &r.entry_ramp_id) {
        (Some(_), Some(_), _) => {
            return Err(invalid("either origin or originNodeId must be provided"));
        }
        (None, None, None) => {
            return Err(invalid("either origin or originNodeId must be provided"));
        }
        (Some(node_id), None, _) => {
            if node_id.is_empty() || node_id.len() > 256 {
                return Err(invalid("invalid origin node id"));
            }
        }
        (None, Some(origin), _) => {
            if !origin.lat.is_finite()
                || !origin.lon.is_finite()
                || !(-90.0..=90.0).contains(&origin.lat)
                || !(-180.0..=180.0).contains(&origin.lon)
            {
                return Err(invalid("coordinates out of range or non-finite"));
            }
        }
        (None, None, Some(entry_ramp)) => {
            if entry_ramp.is_empty() || entry_ramp.len() > 256 {
                return Err(invalid("invalid entry ramp id"));
            }
        }
    }
    if let Some(ref entry_ramp) = r.entry_ramp_id {
        if entry_ramp.is_empty() || entry_ramp.len() > 256 {
            return Err(invalid("invalid entry ramp id"));
        }
        let Some(&idx) = pg.index.ramp_by_id.get(entry_ramp) else {
            return Err(invalid("unknown or unsupported entry ramp id"));
        };
        let ramp = &pg.graph.ramps[idx];
        if ramp.kind != RampKind::GeneralEntry
            || pg.edge(ramp.edge_id.as_str()).kind != EdgeKind::Entry
        {
            return Err(invalid("entry ramp is not a routable general entry"));
        }
    }
    if let Some(ref exit_ramp) = r.exit_ramp_id {
        if exit_ramp.is_empty() || exit_ramp.len() > 256 {
            return Err(invalid("invalid exit ramp id"));
        }
        let Some(&idx) = pg.index.ramp_by_id.get(exit_ramp) else {
            return Err(invalid("unknown or unsupported exit ramp id"));
        };
        let ramp = &pg.graph.ramps[idx];
        if ramp.kind != RampKind::GeneralExit
            || pg.edge(ramp.edge_id.as_str()).kind != EdgeKind::Exit
        {
            return Err(invalid("exit ramp is not a routable general exit"));
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

    fn charge(&mut self, amount: usize, l: &SearchLimits) -> bool {
        if amount > l.max_expanded_states.saturating_sub(self.expanded) {
            self.expanded = l.max_expanded_states;
            self.truncated = true;
            false
        } else {
            self.expanded += amount;
            true
        }
    }
}

/// Reverse Dijkstra tree pointing every mainline node toward `target`.
/// Equal-cost paths use the lexicographically smaller first edge.
///
/// `allowed` optionally restricts the search to the forward-reachable set of a
/// single entry (`forward_dist[node] != u64::MAX`). That set is closed under
/// outgoing Shutoko edges by construction, so every path that starts inside it
/// stays inside it: restricting the reverse search therefore cannot change any
/// `dist[a]` for an anchor `a` in the set, it only avoids settling nodes that no
/// candidate anchor can ever reach. This is the bounded per-request work that
/// keeps the shared expanded-state budget from being consumed by nearby entry
/// tiers whose forward component is a small stub (Issue #57 review V5-01).
fn reverse_shortest_tree(
    pg: &PreparedGraph,
    target: usize,
    allowed: Option<&[u64]>,
) -> (Vec<u64>, Vec<Option<usize>>, usize) {
    use std::cmp::Reverse;
    let mut dist = vec![u64::MAX; pg.graph.nodes.len()];
    let mut next_edge = vec![None; pg.graph.nodes.len()];
    // The entry's forward set is closed under outgoing edges, so if the target
    // itself is outside it no node inside it can reach the target.
    if allowed.is_some_and(|forward| forward[target] == u64::MAX) {
        return (dist, next_edge, 0);
    }
    let mut heap = BinaryHeap::new();
    dist[target] = 0;
    heap.push(Reverse((0u64, target)));
    let mut expanded = 0usize;
    while let Some(Reverse((cost, node_idx))) = heap.pop() {
        if cost != dist[node_idx] {
            continue;
        }
        expanded += 1;
        let node_id = pg.graph.nodes[node_idx].id.as_str();
        for &edge_idx in pg
            .index
            .incoming
            .get(node_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let edge = &pg.graph.edges[edge_idx];
            if edge.kind != EdgeKind::Shutoko {
                continue;
            }
            let from = pg.index.node_pos[edge.from.as_str()];
            if allowed.is_some_and(|forward| forward[from] == u64::MAX) {
                continue;
            }
            let new_cost = cost.saturating_add(edge.duration_seconds);
            let replace = new_cost < dist[from]
                || (new_cost == dist[from]
                    && next_edge[from].is_none_or(|old: usize| edge.id < pg.graph.edges[old].id));
            if replace {
                dist[from] = new_cost;
                next_edge[from] = Some(edge_idx);
                heap.push(Reverse((new_cost, from)));
            }
        }
    }
    (dist, next_edge, expanded)
}

fn topology_cycles_at(pg: &PreparedGraph, anchor: usize, budget: &mut Budget) -> Vec<Vec<usize>> {
    let cached = pg.cycle_cache.borrow().get(&anchor).cloned();
    let cached = cached.unwrap_or_else(|| {
        let (dist, next_edge, expanded_states) = reverse_shortest_tree(pg, anchor, None);
        let anchor_id = pg.graph.nodes[anchor].id.as_str();
        let mut cycles = Vec::new();
        for edge in pg
            .outgoing_edges(anchor_id)
            .filter(|edge| edge.kind == EdgeKind::Shutoko)
        {
            let first_idx = pg.index.edge_pos[edge.id.as_str()];
            let mut cycle = vec![first_idx];
            let mut at = pg.index.node_pos[edge.to.as_str()];
            let mut seen = BTreeSet::new();
            seen.insert(anchor);
            while at != anchor {
                if !seen.insert(at) || dist[at] == u64::MAX {
                    cycle.clear();
                    break;
                }
                let Some(next) = next_edge[at] else {
                    cycle.clear();
                    break;
                };
                cycle.push(next);
                at = pg.index.node_pos[pg.graph.edges[next].to.as_str()];
            }
            if !cycle.is_empty() {
                let refs: Vec<&Edge> = cycle.iter().map(|&i| &pg.graph.edges[i]).collect();
                if allowed_pg(pg, &refs) {
                    cycles.push(cycle);
                }
            }
        }
        cycles.sort_by(|a, b| {
            let a_seconds: u64 = a.iter().map(|&i| pg.graph.edges[i].duration_seconds).sum();
            let b_seconds: u64 = b.iter().map(|&i| pg.graph.edges[i].duration_seconds).sum();
            a_seconds.cmp(&b_seconds).then_with(|| {
                a.iter()
                    .map(|&i| pg.graph.edges[i].id.as_str())
                    .cmp(b.iter().map(|&i| pg.graph.edges[i].id.as_str()))
            })
        });
        let value = CachedCycles {
            edge_indices: cycles,
            expanded_states,
        };
        pg.cycle_cache.borrow_mut().insert(anchor, value.clone());
        value
    });
    if !budget.charge(cached.expanded_states, &pg.limits) {
        return Vec::new();
    }
    cached.edge_indices
}

fn indexed_cycle_refs<'pg>(
    pg: &'pg PreparedGraph,
    start: &str,
    depth: usize,
    max_seconds: u64,
    budget: &mut Budget,
) -> Vec<Vec<&'pg Edge>> {
    let anchor = pg.index.node_pos[start];
    let mut cycles = Vec::new();
    for indices in topology_cycles_at(pg, anchor, budget) {
        let refs: Vec<&Edge> = indices.iter().map(|&i| &pg.graph.edges[i]).collect();
        if refs.len() <= depth
            && seconds(&refs) <= max_seconds
            && meters(&refs) >= pg.limits.min_loop_meters
        {
            cycles.push(refs);
        }
    }
    cycles
}

/// Forward Dijkstra tree from `start`, restricted to mainline edges.
fn forward_shortest_tree(
    pg: &PreparedGraph,
    start: usize,
) -> (Vec<u64>, Vec<Option<usize>>, usize) {
    use std::cmp::Reverse;
    let mut dist = vec![u64::MAX; pg.graph.nodes.len()];
    let mut previous_edge = vec![None; pg.graph.nodes.len()];
    let mut heap = BinaryHeap::new();
    dist[start] = 0;
    heap.push(Reverse((0u64, start)));
    let mut expanded = 0usize;
    while let Some(Reverse((cost, node_idx))) = heap.pop() {
        if cost != dist[node_idx] {
            continue;
        }
        expanded += 1;
        let node_id = pg.graph.nodes[node_idx].id.as_str();
        for &edge_idx in pg
            .index
            .outgoing
            .get(node_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let edge = &pg.graph.edges[edge_idx];
            if edge.kind != EdgeKind::Shutoko {
                continue;
            }
            let to = pg.index.node_pos[edge.to.as_str()];
            let new_cost = cost.saturating_add(edge.duration_seconds);
            let replace = new_cost < dist[to]
                || (new_cost == dist[to]
                    && previous_edge[to].is_none_or(|old: usize| edge.id < pg.graph.edges[old].id));
            if replace {
                dist[to] = new_cost;
                previous_edge[to] = Some(edge_idx);
                heap.push(Reverse((new_cost, to)));
            }
        }
    }
    (dist, previous_edge, expanded)
}

fn reconstruct_forward(
    pg: &PreparedGraph,
    previous_edge: &[Option<usize>],
    start: usize,
    target: usize,
) -> Option<Vec<usize>> {
    let mut at = target;
    let mut reversed = Vec::new();
    while at != start {
        let edge_idx = previous_edge[at]?;
        reversed.push(edge_idx);
        at = pg.index.node_pos[pg.graph.edges[edge_idx].from.as_str()];
    }
    reversed.reverse();
    Some(reversed)
}

fn reconstruct_reverse(
    pg: &PreparedGraph,
    next_edge: &[Option<usize>],
    start: usize,
    target: usize,
) -> Option<Vec<usize>> {
    let mut at = start;
    let mut path = Vec::new();
    while at != target {
        let edge_idx = next_edge[at]?;
        path.push(edge_idx);
        at = pg.index.node_pos[pg.graph.edges[edge_idx].to.as_str()];
    }
    Some(path)
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
    // On the full 22k-node graph, enumerating every simple path inside the
    // large SCC is exponential. A reverse shortest-path tree gives one
    // deterministic, admissible return path for each first edge instead.
    // Small contract fixtures retain exhaustive enumeration semantics.
    if pg.graph.edges.len() > 5_000 && start == end && kind == EdgeKind::Shutoko {
        return indexed_cycle_refs(pg, start, depth, max_seconds, budget);
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
                    if start == end {
                        let total_meters: u64 = candidate.iter().map(|e| e.distance_meters).sum();
                        if total_meters < pg.limits.min_loop_meters {
                            // Micro-loop / spiral connector rejected
                            continue;
                        }
                    }
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
    let pg = PreparedGraph {
        graph: g,
        limits: l.clone(),
        index,
        reachable_cache: RefCell::new(HashMap::new()),
        cycle_cache: RefCell::new(HashMap::new()),
    };
    // Cycle construction belongs to preparation, not to an individual query.
    // Prime the bounded catalogue once and zero its per-search charge.
    let anchors = pg.index.cycle_catalog_anchors.clone();
    for anchor in anchors {
        let mut preparation_budget = Budget::default();
        let _ = topology_cycles_at(&pg, anchor, &mut preparation_budget);
        if let Some(cached) = pg.cycle_cache.borrow_mut().get_mut(&anchor) {
            cached.expanded_states = 0;
        }
    }
    Ok(pg)
}

struct DynamicOdOutcome {
    candidate: Option<Candidate>,
    min_plan_seconds: Option<u64>,
    found_cycle: bool,
    legal_route: bool,
    time_rejected: bool,
    handoff_rejected: bool,
}

#[allow(clippy::too_many_arguments)]
fn evaluate_dynamic_od(
    pg: &PreparedGraph,
    r: &SearchRequest,
    now: OffsetDateTime,
    origin_ll: &LatLng,
    entry_ramp: &Ramp,
    exit_ramp: &Ramp,
    forward_dist: &[u64],
    previous_edge: &[Option<usize>],
    budget: &mut Budget,
    is_explicit: bool,
) -> Result<DynamicOdOutcome, RoutingError> {
    let entry_edge = pg.edge(entry_ramp.edge_id.as_str());
    let exit_edge = pg.edge(exit_ramp.edge_id.as_str());
    let entry_mainline = pg.index.node_pos[entry_ramp.mainline_node_id.as_str()];
    let exit_mainline = pg.index.node_pos[exit_ramp.mainline_node_id.as_str()];

    let (reverse_dist, next_edge, reverse_expanded) =
        reverse_shortest_tree(pg, exit_mainline, Some(forward_dist));
    if !budget.charge(reverse_expanded, &pg.limits) {
        return Ok(DynamicOdOutcome {
            candidate: None,
            min_plan_seconds: None,
            found_cycle: false,
            legal_route: false,
            time_rejected: false,
            handoff_rejected: false,
        });
    }

    let mut anchors: Vec<usize> = pg
        .index
        .cycle_catalog_anchors
        .iter()
        .copied()
        .filter(|&node| {
            forward_dist[node] != u64::MAX
                && reverse_dist[node] != u64::MAX
                && pg.index.component_has_cycle[pg.index.component_by_node[node]]
        })
        .collect();
    anchors.sort_by(|&a, &b| {
        forward_dist[a]
            .saturating_add(reverse_dist[a])
            .cmp(&forward_dist[b].saturating_add(reverse_dist[b]))
            .then_with(|| pg.graph.nodes[a].id.cmp(&pg.graph.nodes[b].id))
    });

    let entry_access = pg.node(entry_ramp.node_id.as_str());
    let exit_access = pg.node(exit_ramp.node_id.as_str());
    let access_dist = distance_meters(
        origin_ll.lat,
        origin_ll.lon,
        entry_access.lat,
        entry_access.lon,
    );
    let access_secs = estimated_access_seconds(access_dist);
    let return_dist = distance_meters(
        exit_access.lat,
        exit_access.lon,
        origin_ll.lat,
        origin_ll.lon,
    );
    let return_secs = estimated_access_seconds(return_dist);
    let mut min_plan_seconds = None;
    let mut found_cycle = false;
    let mut legal_route = false;
    let mut time_rejected = false;
    let mut handoff_rejected = false;
    let mut candidate = None;

    for anchor in anchors {
        let Some(approach) = reconstruct_forward(pg, previous_edge, entry_mainline, anchor) else {
            continue;
        };
        let Some(egress) = reconstruct_reverse(pg, &next_edge, anchor, exit_mainline) else {
            continue;
        };
        for cycle in topology_cycles_at(pg, anchor, budget) {
            if budget.truncated {
                break;
            }
            let loop_refs: Vec<&Edge> = cycle.iter().map(|&i| &pg.graph.edges[i]).collect();
            if loop_refs.len() > pg.limits.max_loop_edges
                || meters(&loop_refs) < pg.limits.min_loop_meters
            {
                continue;
            }
            found_cycle = true;
            let mut route_indices =
                Vec::with_capacity(2 + approach.len() + cycle.len() + egress.len());
            route_indices.push(pg.index.edge_pos[entry_edge.id.as_str()]);
            route_indices.extend(&approach);
            route_indices.extend(&cycle);
            route_indices.extend(&egress);
            route_indices.push(pg.index.edge_pos[exit_edge.id.as_str()]);
            let highway: Vec<&Edge> = route_indices.iter().map(|&i| &pg.graph.edges[i]).collect();
            if highway.len() > 20_000 || !allowed_pg(pg, &highway) {
                continue;
            }
            legal_route = true;
            let base = access_secs + seconds(&highway) + return_secs;
            let buffer = 300.max(base.div_ceil(5));
            let plan = base + buffer;
            min_plan_seconds = Some(min_plan_seconds.map_or(plan, |old: u64| old.min(plan)));
            if base < r.min_minutes * 60 || plan > r.max_minutes * 60 {
                time_rejected = true;
                continue;
            }
            if candidate.is_some() {
                continue;
            }

            let billing_pair = pg.graph.billing_pairs.iter().find(|pair| {
                pair.status == VerificationStatus::Verified
                    && (pair.entry_ramp_id.as_deref() == Some(entry_ramp.id.as_str())
                        || pair.entry_id == entry_edge.id)
                    && (pair.exit_ramp_id.as_deref() == Some(exit_ramp.id.as_str())
                        || pair.exit_id == exit_edge.id)
            });
            let tariff = pg
                .index
                .od_tariff_map
                .get(&(entry_ramp.id.clone(), exit_ramp.id.clone()))
                .map(|&idx| &pg.graph.od_tariffs[idx])
                .filter(|tariff| {
                    tariff
                        .effective_from
                        .as_deref()
                        .is_none_or(|from| utc(from).is_ok_and(|from| from <= now))
                        && tariff
                            .effective_to
                            .as_deref()
                            .is_none_or(|to| utc(to).is_ok_and(|to| now < to))
                });
            let table_price = billing_pair.and_then(|pair| {
                pair.prices.iter().find(|price| {
                    utc(&price.effective_from).is_ok_and(|from| from <= now)
                        && price
                            .effective_to
                            .as_deref()
                            .is_none_or(|to| utc(to).is_ok_and(|to| now < to))
                })
            });
            let (amount, effective_from, effective_to, billing_distance, source) =
                if let Some(price) = table_price {
                    (
                        Some(price.amount_yen),
                        Some(price.effective_from.clone()),
                        price.effective_to.clone(),
                        tariff
                            .map(|value| value.billing_distance_meters)
                            .or_else(|| billing_pair.and_then(|pair| pair.billing_distance_meters)),
                        Some("table".to_string()),
                    )
                } else if let Some(value) = tariff {
                    (
                        value.amount_yen,
                        value.effective_from.clone(),
                        value.effective_to.clone(),
                        Some(value.billing_distance_meters),
                        Some("od_tariff".to_string()),
                    )
                } else {
                    (None, None, None, None, None)
                };

            let ids = edge_ids(&highway);
            let candidate_id = std::iter::once(entry_ramp.id.as_str())
                .chain(std::iter::once(exit_ramp.id.as_str()))
                .chain(ids.iter().map(String::as_str))
                .map(|value| format!("{}:{}", value.len(), value))
                .collect::<String>();
            let mut coordinates = Vec::with_capacity(highway.len() + 1);
            let first = pg.node(highway[0].from.as_str());
            coordinates.push([first.lon, first.lat]);
            for edge in &highway {
                let node = pg.node(edge.to.as_str());
                coordinates.push([node.lon, node.lat]);
            }
            let mut road_names = Vec::new();
            for edge in &highway {
                if let Some(name) = &edge.name {
                    if !road_names.contains(name) {
                        road_names.push(name.clone());
                    }
                }
            }
            let departure = r.origin.clone().unwrap_or_else(|| origin_ll.clone());
            let waypoints =
                handoff::select_waypoints(entry_edge.from.as_str(), &loop_refs, exit_edge, |id| {
                    pg.index.node_pos.get(id).map(|&idx| {
                        let node = &pg.graph.nodes[idx];
                        LatLng {
                            lat: node.lat,
                            lon: node.lon,
                        }
                    })
                });
            let maps_url = match handoff::format_maps_url(&departure, &waypoints) {
                Ok(url) => url,
                Err(()) => {
                    if is_explicit {
                        return Err(invalid(
                            "explicit route handoff URL exceeds supported length",
                        ));
                    }
                    handoff_rejected = true;
                    continue;
                }
            };
            let ranking_reason = if amount.is_some() {
                "BEST_TIME_PER_YEN"
            } else {
                "BEST_SHUTOKO_TIME"
            };
            candidate = Some(Candidate {
                id: candidate_id,
                release_id: r.release_id.clone(),
                origin: r.origin.clone(),
                origin_node_id: entry_edge.from.clone(),
                snapped_origin: SnappedOrigin {
                    node_id: entry_edge.from.clone(),
                    lat: entry_access.lat,
                    lon: entry_access.lon,
                    distance_meters: access_dist,
                },
                entry: RampInfo {
                    edge_id: entry_edge.id.clone(),
                    name: Some(entry_ramp.name.clone()),
                    ramp_id: Some(entry_ramp.id.clone()),
                    route: Some(entry_ramp.route.clone()),
                    direction: Some(entry_ramp.direction.clone()),
                },
                exit: RampInfo {
                    edge_id: exit_edge.id.clone(),
                    name: Some(exit_ramp.name.clone()),
                    ramp_id: Some(exit_ramp.id.clone()),
                    route: Some(exit_ramp.route.clone()),
                    direction: Some(exit_ramp.direction.clone()),
                },
                entry_id: entry_edge.id.clone(),
                exit_id: exit_edge.id.clone(),
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
                    plan_seconds: plan,
                },
                distance_meters: meters(&highway),
                shutoko_distance_meters: meters(&highway),
                toll: Toll {
                    billing_pair_id: billing_pair.map_or_else(
                        || format!("od:{}:{}", entry_ramp.id, exit_ramp.id),
                        |pair| pair.id.clone(),
                    ),
                    charged_section_count: 1,
                    amount_yen: amount,
                    pricing_at: r.pricing_at.clone(),
                    effective_from,
                    effective_to,
                    billing_distance_meters: billing_distance,
                    toll_source: source,
                },
                r#loop: Loop {
                    anchor_node_id: pg.graph.nodes[anchor].id.clone(),
                    edge_ids: edge_ids(&loop_refs),
                    duration_seconds: seconds(&loop_refs),
                    distance_meters: meters(&loop_refs),
                    validated: true,
                },
                reasons: if is_explicit {
                    vec![ranking_reason.into(), "EXPLICIT_OD".into()]
                } else {
                    Vec::new()
                },
                warnings: vec![
                    "STATIC_TRAVEL_TIME".into(),
                    "HANDOFF_WAYPOINTS_UNVERIFIED".into(),
                ],
                handoff: Handoff {
                    origin: departure.clone(),
                    destination: departure,
                    waypoints,
                    maps_url,
                    verification_set_version: None,
                },
            });
        }
        if budget.truncated {
            break;
        }
    }

    Ok(DynamicOdOutcome {
        candidate,
        min_plan_seconds,
        found_cycle,
        legal_route,
        time_rejected,
        handoff_rejected,
    })
}

fn explicit_pair_search(
    pg: &PreparedGraph,
    r: &SearchRequest,
    now: OffsetDateTime,
    origin_ll: &LatLng,
    nearest_access: Option<SnappedOrigin>,
) -> Result<SearchResult, RoutingError> {
    let entry_id = r.entry_ramp_id.as_deref().expect("validated entry ramp");
    let exit_id = r.exit_ramp_id.as_deref().expect("validated exit ramp");
    let entry_ramp = &pg.graph.ramps[pg.index.ramp_by_id[entry_id]];
    let exit_ramp = &pg.graph.ramps[pg.index.ramp_by_id[exit_id]];
    let entry_mainline = pg.index.node_pos[entry_ramp.mainline_node_id.as_str()];

    let mut budget = Budget::default();
    let (forward_dist, previous_edge, forward_expanded) = forward_shortest_tree(pg, entry_mainline);
    if !budget.charge(forward_expanded, &pg.limits) {
        return Ok(SearchResult {
            request_id: r.request_id.clone(),
            release_id: r.release_id.clone(),
            status: "truncated".into(),
            reason: Some("SEARCH_LIMIT".into()),
            ranking_mode: "shutoko_time".into(),
            expanded_states: budget.expanded,
            candidates: Vec::new(),
            nearest_access,
            min_plan_seconds: None,
        });
    }

    let outcome = evaluate_dynamic_od(
        pg,
        r,
        now,
        origin_ll,
        entry_ramp,
        exit_ramp,
        &forward_dist,
        &previous_edge,
        &mut budget,
        true,
    )?;

    if budget.truncated {
        return Ok(SearchResult {
            request_id: r.request_id.clone(),
            release_id: r.release_id.clone(),
            status: "truncated".into(),
            reason: Some("SEARCH_LIMIT".into()),
            ranking_mode: "shutoko_time".into(),
            expanded_states: budget.expanded,
            candidates: Vec::new(),
            nearest_access,
            min_plan_seconds: None,
        });
    }

    let candidates: Vec<Candidate> = outcome.candidate.into_iter().collect();
    let (status, reason) = if !candidates.is_empty() {
        ("ok", None)
    } else if outcome.found_cycle && outcome.min_plan_seconds.is_some() {
        ("no_candidates", Some("TIME_WINDOW"))
    } else {
        ("no_candidates", Some("NO_LOOP"))
    };
    let ranking_mode = if candidates
        .iter()
        .all(|value| value.toll.amount_yen.is_some())
        && !candidates.is_empty()
    {
        "time_per_yen"
    } else {
        "shutoko_time"
    };
    Ok(SearchResult {
        request_id: r.request_id.clone(),
        release_id: r.release_id.clone(),
        status: status.into(),
        reason: reason.map(str::to_owned),
        ranking_mode: ranking_mode.into(),
        expanded_states: budget.expanded,
        candidates,
        nearest_access,
        min_plan_seconds: outcome.min_plan_seconds,
    })
}

fn coordinate_tier_search(
    pg: &PreparedGraph,
    r: &SearchRequest,
    now: OffsetDateTime,
    origin_ll: &LatLng,
    access_node_indices: &BTreeSet<usize>,
    nearest_access: Option<SnappedOrigin>,
) -> Result<SearchResult, RoutingError> {
    struct EntryTier<'a> {
        ramp: &'a Ramp,
        access_dist: f64,
        access_secs: u64,
    }

    let mut entry_tiers = Vec::new();
    for &node_idx in access_node_indices {
        let node = &pg.graph.nodes[node_idx];
        let access_dist = distance_meters(origin_ll.lat, origin_ll.lon, node.lat, node.lon);
        if pg.limits.max_access_distance_meters > 0.0
            && access_dist > pg.limits.max_access_distance_meters
        {
            continue;
        }
        let access_secs = estimated_access_seconds(access_dist);
        if let Some(ramp_indices) = pg.index.general_entries_by_node.get(&node.id) {
            for &r_idx in ramp_indices {
                let ramp = &pg.graph.ramps[r_idx];
                if let Some(ref req_entry) = r.entry_ramp_id {
                    if ramp.id != *req_entry {
                        continue;
                    }
                }
                entry_tiers.push(EntryTier {
                    ramp,
                    access_dist,
                    access_secs,
                });
            }
        }
    }

    if entry_tiers.is_empty() {
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

    entry_tiers.sort_by(|a, b| {
        a.access_dist
            .total_cmp(&b.access_dist)
            .then_with(|| a.access_secs.cmp(&b.access_secs))
            .then_with(|| a.ramp.id.cmp(&b.ramp.id))
    });

    let mut budget = Budget::default();
    let mut diagnostic_budget = Budget::default();
    let mut global_min_plan_seconds: Option<u64> = None;
    let mut global_legal_route = false;
    let mut global_time_rejected = false;
    let mut global_handoff_rejected = false;
    // The nearest access tier owns the diagnostic. When it has been fully
    // evaluated and exposes at least one legal loop, the user's window — not a
    // farther entry — is the reason no candidate fits, so the search stops and
    // reports `TIME_WINDOW`/`NO_HANDOFF` with the proven `minPlanSeconds`.
    // Deeper fall-through is reserved for a nearest entry that cannot form any
    // legal loop at all (structural dead end), which keeps the nearest-entry
    // priority while restoring the coordinate recovery path (review V5-01/V5-03
    // product requirement). A truncated tier never terminates this way: the
    // fail-closed `SEARCH_LIMIT` path is unchanged.
    for (tier_index, tier) in entry_tiers.into_iter().enumerate() {
        let is_nearest_tier = tier_index == 0;
        if budget.truncated {
            return Ok(SearchResult {
                request_id: r.request_id.clone(),
                release_id: r.release_id.clone(),
                status: "truncated".into(),
                reason: Some("SEARCH_LIMIT".into()),
                ranking_mode: "shutoko_time".into(),
                expanded_states: budget.expanded,
                candidates: Vec::new(),
                nearest_access,
                min_plan_seconds: None,
            });
        }

        let verified_pairs: Vec<usize> = pg
            .index
            .verified_pairs_by_entry_ramp
            .get(&tier.ramp.id)
            .map(|indices| {
                indices
                    .iter()
                    .copied()
                    .filter(|&p_idx| {
                        let p = &pg.graph.billing_pairs[p_idx];
                        if let Some(ref req_exit) = r.exit_ramp_id {
                            let matches_pair = p.exit_ramp_id.as_deref() == Some(req_exit.as_str());
                            let matches_exit_edge = p.exit_id == *req_exit;
                            let matches_ramp = pg
                                .index
                                .ramp_by_id
                                .get(req_exit)
                                .is_some_and(|&idx| pg.graph.ramps[idx].edge_id == p.exit_id);
                            if !matches_pair && !matches_exit_edge && !matches_ramp {
                                return false;
                            }
                        }
                        true
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !verified_pairs.is_empty() {
            // Verified Tier
            if verified_pairs.len() > pg.limits.max_pairs {
                budget.truncated = true;
                return Ok(SearchResult {
                    request_id: r.request_id.clone(),
                    release_id: r.release_id.clone(),
                    status: "truncated".into(),
                    reason: Some("SEARCH_LIMIT".into()),
                    ranking_mode: "time_per_yen".into(),
                    expanded_states: budget.expanded,
                    candidates: Vec::new(),
                    nearest_access,
                    min_plan_seconds: None,
                });
            }

            let mut tier_candidates: Vec<Candidate> = Vec::new();
            let mut tier_min_plan: Option<u64> = None;
            let mut tier_candidate_edges = 0usize;

            'verified: for &p_idx in &verified_pairs {
                let p = &pg.graph.billing_pairs[p_idx];
                let pre = path_pg(pg, &p.entry_to_anchor_edge_ids)?;
                let post = path_pg(pg, &p.anchor_to_exit_edge_ids)?;
                let entry_edge = pre[0];
                let exit_edge = post.last().unwrap();
                let access_dist = tier.access_dist;
                let access_secs = tier.access_secs;
                let exit_to_node = pg.node(exit_edge.to.as_str());
                let return_dist = distance_meters(
                    exit_to_node.lat,
                    exit_to_node.lon,
                    origin_ll.lat,
                    origin_ll.lon,
                );
                let return_secs = estimated_access_seconds(return_dist);
                let reachable =
                    cached_reachable_set(pg, p.anchor_node_id.as_str(), r.max_minutes * 60);
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
                if budget.truncated {
                    // The pair enumeration is incomplete, but any candidate that
                    // was already fully validated inside this tier stays valid.
                    // Keep it and report truncation instead of discarding it
                    // (Issue #57 review V5-03).
                    break 'verified;
                }

                for cycle in &loops {
                    if !budget.take(&pg.limits) {
                        break 'verified;
                    }
                    let highway: Vec<_> = pre.iter().chain(cycle).chain(&post).copied().collect();
                    if !allowed_pg(pg, &highway) {
                        continue;
                    }
                    global_legal_route = true;
                    let base = access_secs + seconds(&highway) + return_secs;
                    let buffer = 300.max(base.div_ceil(5));
                    let plan_seconds = base + buffer;
                    tier_min_plan =
                        Some(tier_min_plan.map_or(plan_seconds, |m| m.min(plan_seconds)));
                    if base < r.min_minutes * 60 || base + buffer > r.max_minutes * 60 {
                        global_time_rejected = true;
                        continue;
                    }
                    if tier_candidates.len() == pg.limits.beam_width
                        || tier_candidate_edges + highway.len() > 20_000
                    {
                        // Same-tier storage limit: keep the candidates that were
                        // already validated and stop this tier (V5-03).
                        budget.truncated = true;
                        break 'verified;
                    }
                    tier_candidate_edges += highway.len();
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

                    let access_node = pg.node(&entry_edge.from);
                    let snapped_origin = SnappedOrigin {
                        node_id: entry_edge.from.clone(),
                        lat: access_node.lat,
                        lon: access_node.lon,
                        distance_meters: access_dist,
                    };

                    let departure = r.origin.clone().unwrap_or_else(|| origin_ll.clone());
                    let waypoints = handoff::select_waypoints(
                        entry_edge.from.as_str(),
                        cycle,
                        exit_edge,
                        |id| {
                            pg.index.node_pos.get(id).map(|&i| {
                                let n = &pg.graph.nodes[i];
                                LatLng {
                                    lat: n.lat,
                                    lon: n.lon,
                                }
                            })
                        },
                    );
                    let maps_url = match handoff::format_maps_url(&departure, &waypoints) {
                        Ok(url) => url,
                        Err(()) => {
                            global_handoff_rejected = true;
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

                    // Display names follow the authoritative BillingPair model:
                    // the pair's own entry/exit name wins, then the Ramp ledger.
                    let entry_ramp_info = RampInfo {
                        edge_id: p.entry_id.clone(),
                        name: p
                            .entry_name
                            .clone()
                            .or_else(|| Some(tier.ramp.name.clone())),
                        ramp_id: Some(tier.ramp.id.clone()),
                        route: Some(tier.ramp.route.clone()),
                        direction: Some(tier.ramp.direction.clone()),
                    };
                    let exit_ramp = pg
                        .index
                        .ramp_by_id
                        .get(p.exit_ramp_id.as_deref().unwrap_or_default())
                        .map(|&idx| &pg.graph.ramps[idx])
                        .or_else(|| {
                            pg.index
                                .ramp_by_edge
                                .get(&p.exit_id)
                                .map(|&idx| &pg.graph.ramps[idx])
                        });
                    let exit_ramp_info = RampInfo {
                        edge_id: p.exit_id.clone(),
                        name: p
                            .exit_name
                            .clone()
                            .or_else(|| exit_ramp.map(|r| r.name.clone())),
                        ramp_id: exit_ramp
                            .map(|r| r.id.clone())
                            .or_else(|| p.exit_ramp_id.clone()),
                        route: exit_ramp.map(|r| r.route.clone()),
                        direction: exit_ramp.map(|r| r.direction.clone()),
                    };

                    let od_tariff = match (
                        entry_ramp_info.ramp_id.as_deref(),
                        exit_ramp_info.ramp_id.as_deref(),
                    ) {
                        (Some(e_id), Some(x_id)) => pg
                            .index
                            .od_tariff_map
                            .get(&(e_id.to_string(), x_id.to_string()))
                            .map(|&idx| &pg.graph.od_tariffs[idx]),
                        _ => None,
                    };

                    let (toll_amount, toll_from, toll_to, toll_distance, toll_source) =
                        if let Some(price) = price {
                            (
                                Some(price.amount_yen),
                                Some(price.effective_from.clone()),
                                price.effective_to.clone(),
                                od_tariff
                                    .map(|t| t.billing_distance_meters)
                                    .or(p.billing_distance_meters),
                                Some("table".to_string()),
                            )
                        } else if !p.prices.is_empty() {
                            (
                                None,
                                None,
                                None,
                                od_tariff
                                    .map(|t| t.billing_distance_meters)
                                    .or(p.billing_distance_meters),
                                None,
                            )
                        } else if let Some(tariff) = od_tariff.filter(|t| {
                            t.effective_from
                                .as_deref()
                                .is_none_or(|from| utc(from).is_ok_and(|from| from <= now))
                                && t.effective_to
                                    .as_deref()
                                    .is_none_or(|to| utc(to).is_ok_and(|to| now < to))
                        }) {
                            (
                                tariff.amount_yen.or_else(|| {
                                    Some(calculate_etc_toll_yen(tariff.billing_distance_meters))
                                }),
                                tariff.effective_from.clone(),
                                tariff.effective_to.clone(),
                                Some(tariff.billing_distance_meters),
                                Some("od_tariff".to_string()),
                            )
                        } else if let Some(dist) = p.billing_distance_meters {
                            (
                                Some(calculate_etc_toll_yen(dist)),
                                None,
                                None,
                                Some(dist),
                                Some("calculated".to_string()),
                            )
                        } else {
                            (None, None, None, None, None)
                        };

                    tier_candidates.push(Candidate {
                        id,
                        release_id: r.release_id.clone(),
                        origin: r.origin.clone(),
                        origin_node_id: entry_edge.from.clone(),
                        snapped_origin,
                        entry: entry_ramp_info,
                        exit: exit_ramp_info,
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
                            amount_yen: toll_amount,
                            pricing_at: r.pricing_at.clone(),
                            effective_from: toll_from,
                            effective_to: toll_to,
                            billing_distance_meters: toll_distance,
                            toll_source,
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

                if r.max_minutes < MAX_PRODUCT_MINUTES {
                    let diagnostic_seconds = MAX_PRODUCT_MINUTES * 60;
                    let reachable =
                        cached_reachable_set(pg, p.anchor_node_id.as_str(), diagnostic_seconds);
                    let diagnostic_loops = paths_pg(
                        pg,
                        &p.anchor_node_id,
                        &p.anchor_node_id,
                        EdgeKind::Shutoko,
                        pg.limits.max_loop_edges,
                        diagnostic_seconds,
                        &reachable,
                        &mut diagnostic_budget,
                    );
                    for cycle in &diagnostic_loops {
                        if !diagnostic_budget.take(&pg.limits) {
                            break;
                        }
                        let highway: Vec<_> =
                            pre.iter().chain(cycle).chain(&post).copied().collect();
                        if !allowed_pg(pg, &highway) {
                            continue;
                        }
                        let base = access_secs + seconds(&highway) + return_secs;
                        let buffer = 300.max(base.div_ceil(5));
                        let plan_seconds = base + buffer;
                        tier_min_plan =
                            Some(tier_min_plan.map_or(plan_seconds, |m| m.min(plan_seconds)));
                        global_legal_route = true;
                        if base < r.min_minutes * 60 || base + buffer > r.max_minutes * 60 {
                            global_time_rejected = true;
                        }
                    }
                }
            }

            if !tier_candidates.is_empty() {
                tier_candidates.sort_by(|a, b| {
                    let ratio = ((b.duration.shutoko_seconds as u128)
                        * (a.toll.amount_yen.unwrap_or(1) as u128))
                        .cmp(
                            &((a.duration.shutoko_seconds as u128)
                                * (b.toll.amount_yen.unwrap_or(1) as u128)),
                        );
                    ratio
                        .then_with(|| b.duration.shutoko_seconds.cmp(&a.duration.shutoko_seconds))
                        .then_with(|| {
                            (a.duration.access_seconds + a.duration.return_seconds)
                                .cmp(&(b.duration.access_seconds + b.duration.return_seconds))
                        })
                        .then_with(|| a.id.cmp(&b.id))
                });
                let mut selected: Vec<Candidate> = Vec::new();
                for c in tier_candidates {
                    if selected.iter().any(|s| similar_pg(pg, &c, s)) {
                        continue;
                    }
                    selected.push(c);
                    if selected.len() == pg.limits.max_candidates {
                        break;
                    }
                }
                let time_ranking = selected.iter().any(|c| c.toll.amount_yen.is_none());
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
                let min_p = if budget.truncated || diagnostic_budget.truncated {
                    None
                } else {
                    global_min_plan_seconds.map_or(tier_min_plan, |g| {
                        Some(tier_min_plan.map_or(g, |t| g.min(t)))
                    })
                };
                return Ok(SearchResult {
                    request_id: r.request_id.clone(),
                    release_id: r.release_id.clone(),
                    status: if budget.truncated { "truncated" } else { "ok" }.into(),
                    reason: if budget.truncated {
                        Some("SEARCH_LIMIT".into())
                    } else {
                        None
                    },
                    ranking_mode: if time_ranking {
                        "shutoko_time".into()
                    } else {
                        "time_per_yen".into()
                    },
                    expanded_states: budget.expanded,
                    candidates: selected,
                    nearest_access,
                    min_plan_seconds: min_p,
                });
            }

            global_min_plan_seconds = global_min_plan_seconds.map_or(tier_min_plan, |g| {
                Some(tier_min_plan.map_or(g, |t| g.min(t)))
            });
            if is_nearest_tier && global_legal_route && !budget.truncated {
                break;
            }
            continue;
        }

        // Dynamic Tier
        let mut exits: Vec<&Ramp> = Vec::new();
        if let Some(same_fac) = pg
            .index
            .general_exits_by_facility
            .get(&tier.ramp.facility_id)
        {
            for &xr_idx in same_fac {
                let xr = &pg.graph.ramps[xr_idx];
                if xr.edge_id == tier.ramp.edge_id
                    || xr.mainline_node_id == tier.ramp.mainline_node_id
                {
                    continue;
                }
                if let Some(ref req_exit) = r.exit_ramp_id {
                    if xr.id != *req_exit {
                        continue;
                    }
                }
                exits.push(xr);
            }
        }
        exits.sort_by(|a, b| a.id.cmp(&b.id));
        if exits.len() > 2 {
            exits.truncate(2);
        }

        if exits.is_empty() {
            for &xr_idx in &pg.index.verified_pair_exit_ramps {
                let xr = &pg.graph.ramps[xr_idx];
                if xr.edge_id == tier.ramp.edge_id
                    || xr.mainline_node_id == tier.ramp.mainline_node_id
                {
                    continue;
                }
                if let Some(ref req_exit) = r.exit_ramp_id {
                    if xr.id != *req_exit {
                        continue;
                    }
                }
                exits.push(xr);
            }
            exits.sort_by(|a, b| a.id.cmp(&b.id));
            if exits.len() > 2 {
                exits.truncate(2);
            }
        }

        if exits.is_empty() {
            continue;
        }

        let entry_mainline = pg.index.node_pos[tier.ramp.mainline_node_id.as_str()];
        let (forward_dist, previous_edge, forward_expanded) =
            forward_shortest_tree(pg, entry_mainline);
        if !budget.charge(forward_expanded, &pg.limits) {
            return Ok(SearchResult {
                request_id: r.request_id.clone(),
                release_id: r.release_id.clone(),
                status: "truncated".into(),
                reason: Some("SEARCH_LIMIT".into()),
                ranking_mode: "shutoko_time".into(),
                expanded_states: budget.expanded,
                candidates: Vec::new(),
                nearest_access,
                min_plan_seconds: None,
            });
        }

        let mut tier_candidates: Vec<Candidate> = Vec::new();
        let mut tier_min_plan: Option<u64> = None;

        for exit_ramp in exits {
            let outcome = evaluate_dynamic_od(
                pg,
                r,
                now,
                origin_ll,
                tier.ramp,
                exit_ramp,
                &forward_dist,
                &previous_edge,
                &mut budget,
                false,
            )?;
            if budget.truncated {
                // The tier is incomplete: stop evaluating further exits but keep
                // any candidates that were fully validated for earlier exits.
                // Falling through to a farther entry is not allowed.
                break;
            }
            global_legal_route |= outcome.legal_route;
            global_time_rejected |= outcome.time_rejected;
            global_handoff_rejected |= outcome.handoff_rejected;
            tier_min_plan = tier_min_plan.map_or(outcome.min_plan_seconds, |m| {
                Some(outcome.min_plan_seconds.map_or(m, |t| m.min(t)))
            });
            if let Some(c) = outcome.candidate {
                tier_candidates.push(c);
            }
        }

        if !tier_candidates.is_empty() {
            let (priced, unknown): (Vec<_>, Vec<_>) = tier_candidates
                .into_iter()
                .partition(|c| c.toll.amount_yen.is_some());
            let mut cohort = if !priced.is_empty() { priced } else { unknown };
            let time_ranking = cohort.iter().any(|c| c.toll.amount_yen.is_none());
            cohort.sort_by(|a, b| {
                let ratio = if time_ranking {
                    std::cmp::Ordering::Equal
                } else {
                    ((b.duration.shutoko_seconds as u128)
                        * (a.toll.amount_yen.unwrap_or(1) as u128))
                        .cmp(
                            &((a.duration.shutoko_seconds as u128)
                                * (b.toll.amount_yen.unwrap_or(1) as u128)),
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
            for c in cohort {
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
            let ranking_mode = if time_ranking {
                "shutoko_time"
            } else {
                "time_per_yen"
            };
            let min_p = if budget.truncated || diagnostic_budget.truncated {
                None
            } else {
                global_min_plan_seconds.map_or(tier_min_plan, |g| {
                    Some(tier_min_plan.map_or(g, |t| g.min(t)))
                })
            };
            return Ok(SearchResult {
                request_id: r.request_id.clone(),
                release_id: r.release_id.clone(),
                status: if budget.truncated { "truncated" } else { "ok" }.into(),
                reason: if budget.truncated {
                    Some("SEARCH_LIMIT".into())
                } else {
                    None
                },
                ranking_mode: ranking_mode.into(),
                expanded_states: budget.expanded,
                candidates: selected,
                nearest_access,
                min_plan_seconds: min_p,
            });
        }

        global_min_plan_seconds = global_min_plan_seconds.map_or(tier_min_plan, |g| {
            Some(tier_min_plan.map_or(g, |t| g.min(t)))
        });
        if is_nearest_tier && global_legal_route && !budget.truncated {
            break;
        }
    }

    let reason = if budget.truncated {
        Some("SEARCH_LIMIT")
    } else if !global_legal_route {
        Some("NO_LOOP")
    } else if global_handoff_rejected && !global_time_rejected {
        Some("NO_HANDOFF")
    } else {
        Some("TIME_WINDOW")
    };
    let status = if budget.truncated {
        "truncated"
    } else {
        "no_candidates"
    };
    let min_p = if budget.truncated || diagnostic_budget.truncated {
        None
    } else {
        global_min_plan_seconds
    };
    Ok(SearchResult {
        request_id: r.request_id.clone(),
        release_id: r.release_id.clone(),
        status: status.into(),
        reason: reason.map(str::to_owned),
        ranking_mode: "shutoko_time".into(),
        expanded_states: budget.expanded,
        candidates: Vec::new(),
        nearest_access,
        min_plan_seconds: min_p,
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
    ) = match (&r.origin_node_id, &r.origin, &r.entry_ramp_id) {
        (Some(id), None, _) => {
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
        (None, Some(ll), _) => {
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
        (None, None, Some(entry_ramp_id)) => {
            let ramp = if let Some(&idx) = pg.index.ramp_by_id.get(entry_ramp_id) {
                &pg.graph.ramps[idx]
            } else {
                return Err(invalid("unknown entry ramp id"));
            };
            if !pg.has_node(ramp.node_id.as_str()) {
                return Err(invalid("ramp node not in graph"));
            }
            let node = pg.node(ramp.node_id.as_str());
            let origin_ll = LatLng {
                lat: node.lat,
                lon: node.lon,
            };
            let idx = pg.index.node_pos[ramp.node_id.as_str()];
            (origin_ll, std::iter::once(idx).collect(), None)
        }
        _ => return Err(invalid("origin resolution state unreachable")),
    };

    if r.entry_ramp_id.is_some() && r.exit_ramp_id.is_some() {
        return explicit_pair_search(pg, r, now, &origin_ll, nearest_access);
    }

    if r.origin.is_some() && !pg.graph.ramps.is_empty() {
        return coordinate_tier_search(
            pg,
            r,
            now,
            &origin_ll,
            &access_node_indices,
            nearest_access,
        );
    }

    let mut budget = Budget::default();
    let mut candidates = Vec::new();
    let mut candidate_edges = 0;
    let any_pairs = pg
        .graph
        .billing_pairs
        .iter()
        .any(|p| p.status == VerificationStatus::Verified);

    let mut pairs: Vec<_> = pg
        .graph
        .billing_pairs
        .iter()
        .filter(|p| p.status == VerificationStatus::Verified)
        .filter(|p| {
            if let Some(ref req_entry) = r.entry_ramp_id {
                let matches_pair = p.entry_ramp_id.as_deref() == Some(req_entry.as_str());
                let matches_entry_edge = p.entry_id == *req_entry;
                let matches_ramp = pg
                    .index
                    .ramp_by_id
                    .get(req_entry)
                    .is_some_and(|&idx| pg.graph.ramps[idx].edge_id == p.entry_id);
                if !matches_pair && !matches_entry_edge && !matches_ramp {
                    return false;
                }
            }
            if let Some(ref req_exit) = r.exit_ramp_id {
                let matches_pair = p.exit_ramp_id.as_deref() == Some(req_exit.as_str());
                let matches_exit_edge = p.exit_id == *req_exit;
                let matches_ramp = pg
                    .index
                    .ramp_by_id
                    .get(req_exit)
                    .is_some_and(|&idx| pg.graph.ramps[idx].edge_id == p.exit_id);
                if !matches_pair && !matches_exit_edge && !matches_ramp {
                    return false;
                }
            }
            if let Some(&edge_idx) = pg.index.edge_pos.get(&p.entry_id) {
                let from_node = &pg.graph.edges[edge_idx].from;
                if let Some(&node_idx) = pg.index.node_pos.get(from_node) {
                    if !access_node_indices.contains(&node_idx) {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
            true
        })
        .collect();

    pairs.sort_by(|a, b| {
        if r.origin.is_some() {
            let edge_a = &pg.graph.edges[pg.index.edge_pos[&a.entry_id]];
            let edge_b = &pg.graph.edges[pg.index.edge_pos[&b.entry_id]];
            let node_a = pg.node(&edge_a.from);
            let node_b = pg.node(&edge_b.from);
            let dist_a = distance_meters(origin_ll.lat, origin_ll.lon, node_a.lat, node_a.lon);
            let dist_b = distance_meters(origin_ll.lat, origin_ll.lon, node_b.lat, node_b.lon);
            dist_a.total_cmp(&dist_b).then_with(|| a.id.cmp(&b.id))
        } else {
            a.id.cmp(&b.id)
        }
    });

    if pairs.len() > pg.limits.max_pairs {
        budget.truncated = true;
        pairs.truncate(pg.limits.max_pairs);
    }
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

    // Separate budget for the diagnostic (product-cap) enumeration so that the
    // extra loops it explores can never consume the candidate-search budget or
    // trigger `SEARCH_LIMIT` truncation.
    let mut diagnostic_budget = Budget::default();

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

            let entry_ramp = p
                .entry_ramp_id
                .as_deref()
                .and_then(|id| pg.index.ramp_by_id.get(id).map(|&idx| &pg.graph.ramps[idx]))
                .or_else(|| {
                    pg.index
                        .ramp_by_edge
                        .get(&p.entry_id)
                        .map(|&idx| &pg.graph.ramps[idx])
                });
            let exit_ramp = p
                .exit_ramp_id
                .as_deref()
                .and_then(|id| pg.index.ramp_by_id.get(id).map(|&idx| &pg.graph.ramps[idx]))
                .or_else(|| {
                    pg.index
                        .ramp_by_edge
                        .get(&p.exit_id)
                        .map(|&idx| &pg.graph.ramps[idx])
                });

            let entry_info = RampInfo {
                edge_id: p.entry_id.clone(),
                name: p
                    .entry_name
                    .clone()
                    .or_else(|| entry_ramp.map(|r| r.name.clone())),
                ramp_id: entry_ramp
                    .map(|r| r.id.clone())
                    .or_else(|| p.entry_ramp_id.clone()),
                route: entry_ramp.map(|r| r.route.clone()),
                direction: entry_ramp.map(|r| r.direction.clone()),
            };
            let exit_info = RampInfo {
                edge_id: p.exit_id.clone(),
                name: p
                    .exit_name
                    .clone()
                    .or_else(|| exit_ramp.map(|r| r.name.clone())),
                ramp_id: exit_ramp
                    .map(|r| r.id.clone())
                    .or_else(|| p.exit_ramp_id.clone()),
                route: exit_ramp.map(|r| r.route.clone()),
                direction: exit_ramp.map(|r| r.direction.clone()),
            };

            let od_tariff = match (entry_info.ramp_id.as_deref(), exit_info.ramp_id.as_deref()) {
                (Some(e_id), Some(x_id)) => pg
                    .index
                    .od_tariff_map
                    .get(&(e_id.to_string(), x_id.to_string()))
                    .map(|&idx| &pg.graph.od_tariffs[idx]),
                _ => None,
            };

            let (toll_amount, toll_from, toll_to, toll_distance, toll_source) =
                if let Some(price) = price {
                    (
                        Some(price.amount_yen),
                        Some(price.effective_from.clone()),
                        price.effective_to.clone(),
                        od_tariff
                            .map(|t| t.billing_distance_meters)
                            .or(p.billing_distance_meters),
                        Some("table".to_string()),
                    )
                } else if !p.prices.is_empty() {
                    (
                        None,
                        None,
                        None,
                        od_tariff
                            .map(|t| t.billing_distance_meters)
                            .or(p.billing_distance_meters),
                        None,
                    )
                } else if let Some(tariff) = od_tariff.filter(|t| {
                    t.effective_from
                        .as_deref()
                        .is_none_or(|from| utc(from).is_ok_and(|from| from <= now))
                        && t.effective_to
                            .as_deref()
                            .is_none_or(|to| utc(to).is_ok_and(|to| now < to))
                }) {
                    (
                        tariff.amount_yen.or_else(|| {
                            Some(calculate_etc_toll_yen(tariff.billing_distance_meters))
                        }),
                        tariff.effective_from.clone(),
                        tariff.effective_to.clone(),
                        Some(tariff.billing_distance_meters),
                        Some("od_tariff".to_string()),
                    )
                } else if let Some(dist) = p.billing_distance_meters {
                    (
                        Some(calculate_etc_toll_yen(dist)),
                        None,
                        None,
                        Some(dist),
                        Some("calculated".to_string()),
                    )
                } else {
                    (None, None, None, None, None)
                };

            candidates.push(Candidate {
                id,
                release_id: r.release_id.clone(),
                origin: r.origin.clone(),
                origin_node_id: entry_edge.from.clone(),
                snapped_origin,
                entry: entry_info,
                exit: exit_info,
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
                    amount_yen: toll_amount,
                    pricing_at: r.pricing_at.clone(),
                    effective_from: toll_from,
                    effective_to: toll_to,
                    billing_distance_meters: toll_distance,
                    toll_source,
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

        // Diagnostic extension: the candidate enumeration above is bounded by
        // the *requested* max window, so a legal loop longer than
        // `max_minutes` is pruned and can never reach `minPlanSeconds`.  The UI
        // compares `minPlanSeconds` against the 240-minute product cap to
        // decide whether widening the window can ever help, so re-enumerate up
        // to the product cap with a separate budget.  Loops beyond the request
        // window are always time-rejected, so this only feeds the diagnostic
        // and never changes candidate generation.
        if r.max_minutes < MAX_PRODUCT_MINUTES {
            let diagnostic_seconds = MAX_PRODUCT_MINUTES * 60;
            let reachable = cached_reachable_set(pg, p.anchor_node_id.as_str(), diagnostic_seconds);
            let diagnostic_loops = paths_pg(
                pg,
                &p.anchor_node_id,
                &p.anchor_node_id,
                EdgeKind::Shutoko,
                pg.limits.max_loop_edges,
                diagnostic_seconds,
                &reachable,
                &mut diagnostic_budget,
            );
            for cycle in &diagnostic_loops {
                if !diagnostic_budget.take(&pg.limits) {
                    break;
                }
                let highway: Vec<_> = pre.iter().chain(cycle).chain(&post).copied().collect();
                if !allowed_pg(pg, &highway) {
                    continue;
                }
                let base = access_secs + seconds(&highway) + return_secs;
                let buffer = 300.max(base.div_ceil(5));
                let plan_seconds = base + buffer;
                min_plan_seconds =
                    Some(min_plan_seconds.map_or(plan_seconds, |m| m.min(plan_seconds)));
                found_loop = true;
                legal_route = true;
                // Only a loop that the *requested* window rejects justifies
                // `TIME_WINDOW`.  The diagnostic pass re-enumerates up to the
                // 240-minute product cap, so it also sees loops the requested
                // window would have accepted; marking those as rejected would
                // turn `NO_HANDOFF` into `TIME_WINDOW` and tell the user to
                // widen a window that is not the problem (review V1).
                if base < r.min_minutes * 60 || base + buffer > r.max_minutes * 60 {
                    time_rejected = true;
                }
            }
        }
    }

    // The product-cap decision ("no window up to 240 minutes can work") is only
    // sound when the enumeration that produced `min_plan_seconds` was complete:
    // because `plan_seconds >= loop seconds`, every legal loop that fits the cap
    // is enumerated, so an enumerated minimum above the cap proves that none
    // exists.  When a resource limit (beam width, expanded-state budget, or the
    // billing-pair cap) cut the enumeration short that proof is unavailable and
    // the number is only the minimum over the enumerated subset (it may exceed
    // the true minimum).  Report `null` = "not proven", so the UI asserts
    // neither "shortest" nor "unreachable" without evidence
    // (review TEST-01-FINAL / V1 / V2).
    if budget.truncated || diagnostic_budget.truncated {
        min_plan_seconds = None;
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
