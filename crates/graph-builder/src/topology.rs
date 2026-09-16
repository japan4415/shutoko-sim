//! Directed graph topology builder from OSM Overpass elements.
//!
//! # Design & Determinism
//! - Ways are segmented into consecutive node pairs connected strictly by shared OSM node ID.
//! - Edges connect strictly by shared OSM node IDs; different levels or geometric crossings
//!   without shared node IDs naturally remain separated without requiring layer/bridge/tunnel tags.
//! - Forward/reverse edges for `oneway` tags are expanded deterministically with unique IDs.
//! - Edge distances use the Haversine formula with Earth radius R = 6,371,000.0 m, rounded to u64.
//! - Durations use static speed models per `EdgeKind` with rounding to u64 seconds.
//! - All IDs, nodes, edges, and forbidden transitions are sorted in deterministic ascending order.

use crate::model::{Edge, EdgeKind, Graph, Node, SnapIndex, SnapNode};
use crate::osm::{OsmElement, OverpassResponse};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Diagnostic report capturing turn restriction and ramp topology statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestrictionReport {
    /// Total relation elements with type=restriction inspected.
    pub total_relations: usize,
    /// Count of no_* turn restrictions applied with via=node.
    pub no_turn_via_node: usize,
    /// Count of only_* turn restrictions applied with via=node.
    pub only_turn_via_node: usize,
    /// Number of distinct forbidden edge pairs generated from only_* restrictions.
    pub only_turn_edge_pairs: usize,
    /// Count of via=way restrictions applied (edge sequences of length >= 3).
    pub via_way: usize,
    /// Number of conditional restrictions skipped (e.g. restriction:conditional).
    pub skipped_conditional: usize,
    /// Number of restriction relations missing a via member.
    pub skipped_no_via: usize,
    /// Number of restrictions referencing ways/nodes outside the extracted graph.
    pub skipped_missing_elements: usize,
    /// Number of via=way restrictions where ways do not connect into a continuous path.
    pub skipped_disconnected: usize,
    /// Number of only_* restrictions with via=way skipped (unsupported for static graph).
    pub skipped_only_via_way: usize,
    /// Number of restrictions with unrecognized restriction values skipped.
    pub skipped_unrecognized: usize,
    /// Internal motorway_link edges dropped (not connecting local streets and Shutoko).
    pub dropped_link_edges: usize,
    /// Ramp edges that could not be classified as Entry/Exit vs. Shutoko because neither
    /// the surface-connection signal nor the OSM node tag secondary signal was available.
    /// These edges are conservatively classified as Shutoko. Regenerate with an updated
    /// OSM extract that includes surface roads for accurate classification.
    pub undecidable_ramp_edges: usize,
    /// Human-readable diagnostic messages from two sources:
    ///
    /// 1. **Classification warnings** — entry/exit ramp decisions that required
    ///    conflict resolution between primary (surface-connection) and secondary
    ///    (OSM node tag) signals, or fell back to the conservative Shutoko default
    ///    because both signals were absent.
    ///
    /// 2. **Restriction notes** — turn restriction relations that were skipped
    ///    (conditional, missing via, outside graph, disconnected, unsupported only_*
    ///    via-way, unrecognised restriction value) or otherwise notable.
    pub notes: Vec<String>,
}

impl RestrictionReport {
    /// Sum of all categorized turn restriction relations.
    pub fn total_accounted(&self) -> usize {
        self.no_turn_via_node
            + self.only_turn_via_node
            + self.via_way
            + self.skipped_conditional
            + self.skipped_no_via
            + self.skipped_missing_elements
            + self.skipped_disconnected
            + self.skipped_only_via_way
            + self.skipped_unrecognized
    }

    /// Verifies that every inspected turn restriction relation is fully accounted for.
    pub fn is_balanced(&self) -> bool {
        self.total_relations == self.total_accounted()
    }
}

/// Estimated nominal flow speed for Shutoko urban expressways (60 km/h).
/// Rationale: Tokyo inner circular route (C1) has a legal speed limit of 50-60 km/h;
/// 60 km/h is used as a static nominal flow estimate for free-flow conditions.
pub const SHUTOKO_SPEED_KMH: f64 = 60.0;

/// Estimated speed for entry and exit ramps (40 km/h).
/// Rationale: Motorway link ramps feature tight curvature and toll gate acceleration/deceleration zones.
pub const RAMP_SPEED_KMH: f64 = 40.0;

/// Estimated speed for urban surface streets / local roads (30 km/h).
/// Rationale: Tokyo surface streets have standard 30-50 km/h speed limits with frequent signalized stops.
pub const LOCAL_SPEED_KMH: f64 = 30.0;

/// Mean Earth radius in meters used for Haversine distance calculations.
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

/// Configuration options for building the topology.
#[derive(Debug, Clone)]
pub struct TopologyConfig {
    pub release_id: String,
    pub vehicle_profile: String,
}

impl Default for TopologyConfig {
    fn default() -> Self {
        Self {
            release_id: "default-release".into(),
            vehicle_profile: "passenger-car-etc".into(),
        }
    }
}

/// Compute the Haversine great-circle distance in meters between two lat/lon points.
/// Returns a deterministic integer distance >= 1 meter.
pub fn haversine_distance_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> u64 {
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();

    let a =
        (d_lat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    let distance = EARTH_RADIUS_METERS * c;

    // Deterministic round; clamp to at least 1 meter to satisfy routing-core constraints
    (distance.round() as u64).max(1)
}

/// Compute travel duration in seconds based on distance and static speed in km/h.
/// Returns a deterministic integer duration >= 1 second.
pub fn duration_seconds(distance_meters: u64, speed_kmh: f64) -> u64 {
    let speed_mps = speed_kmh / 3.6;
    let secs = (distance_meters as f64) / speed_mps;
    (secs.round() as u64).max(1)
}

/// Helper representing an intermediate directed edge candidate before final link classification.
#[derive(Debug, Clone)]
struct IntermediateEdge {
    name: Option<String>,
    id: String,
    from: String,
    to: String,
    distance_meters: u64,
    is_motorway_link: bool,
    tentative_kind: Option<EdgeKind>,
}

/// Determines whether a way's tags identify it as Shutoko mainline.
/// Inspects `operator`, `network`, `ref`, and `name` / `name:ja` tags.
/// The `name` check is required because some OSM way segments for the Shutoko C1
/// mainline carry only a name tag (e.g. "首都高速都心環状線") without `ref` or `operator`.
pub fn is_shutoko_motorway(element: &OsmElement) -> bool {
    if element.get_tag("highway") != Some("motorway") {
        return false;
    }

    if let Some(op) = element.get_tag("operator") {
        let op_lower = op.to_lowercase();
        if op.contains("首都高速")
            || op.contains("首都高")
            || op_lower.contains("metropolitan expressway")
            || op_lower.contains("shuto expressway")
        {
            return true;
        }
    }

    if let Some(net) = element.get_tag("network") {
        let net_lower = net.to_lowercase();
        if net.contains("首都高速")
            || net.contains("首都高")
            || net_lower.contains("metropolitan expressway")
        {
            return true;
        }
    }

    if let Some(r) = element.get_tag("ref") {
        let r_upper = r.to_uppercase();
        // C1 (Inner Circular), C2, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, B, K1..K7, S1..S5, Y
        const SHUTOKO_REFS: &[&str] = &[
            "C1", "C2", "1", "2", "3", "4", "5", "6", "7", "9", "10", "11", "B", "K1", "K2", "K3",
            "K5", "K6", "K7", "S1", "S2", "S5", "Y",
        ];
        for known in SHUTOKO_REFS {
            if r_upper == *known || r_upper.split(';').any(|part| part.trim() == *known) {
                return true;
            }
        }
        if r.contains("首都高") {
            return true;
        }
    }

    // Fall back to name / name:ja: some way segments carry only the Japanese name without
    // operator/network/ref tags. "首都高速" and "首都高" unambiguously identify the Shutoko
    // network in combination with highway=motorway.
    let name = element
        .get_tag("name")
        .or_else(|| element.get_tag("name:ja"))
        .unwrap_or("");
    if name.contains("首都高速") || name.contains("首都高") {
        return true;
    }

    false
}

/// Classifies an OSM `highway` value (and, for `highway=service`, the `service=*` sub-tag)
/// as vehicle-accessible (surface-connection evidence), non-vehicle, or ambiguous.
///
/// Returns `Some(true)` when the way is a road vehicles (cars) can drive on — a ramp
/// descending to such a way is a real street-level Entry or Exit.
/// Returns `Some(false)` when the way is a non-vehicle path (footway, cycleway, steps, …),
/// a clearly private/facility-internal access lane, or part of the motorway network already
/// captured in the graph.
/// Returns `None` for values that are ambiguous or structurally unresolvable without
/// additional context; callers should emit a warning and treat the way conservatively
/// (i.e. not count it as surface-connection evidence).
///
/// This uses OSM's documented structural `highway` and `service` taxonomies (what *type*
/// of road is this?) rather than brittle string-pattern matching on road names.
///
/// # `highway=service` classification rationale
/// `highway=service` broadly covers publicly-accessible alleys, private driveways, parking
/// aisles, JCT management roads, and toll-gate vehicle lanes.  Accepting all service ways
/// as surface-connection evidence would silently misclassify JCT connectors whose endpoints
/// share a management/access road.  The `service=*` sub-tag resolves the ambiguity:
///
/// - `parking_aisle` / `driveway` / `drive-through` / `emergency_access` / `slipway`:
///   private or facility-internal — confirmed NOT street-level evidence → `Some(false)`.
/// - `alley`: publicly-accessible back-lane connecting to the street network → `Some(true)`.
/// - Absent or unrecognised sub-tag: management roads, toll-gate lanes, and other
///   facility-internal roads are common here.  Conservative `None` is returned so the
///   caller emits a warning and excludes the way from surface_nodes rather than silently
///   misclassifying.
///
/// # `highway=road` classification rationale
/// `highway=road` is an OSM placeholder meaning "type unknown to the contributor".  It may
/// ultimately be a public road or a private track; treating it as confirmed evidence would
/// introduce silent false positives.  Conservative `None` is returned.
pub(crate) fn is_vehicle_highway(highway: &str, service: Option<&str>) -> Option<bool> {
    match highway {
        // Motor-vehicle roads — confirmed surface connection evidence
        "trunk" | "trunk_link" | "primary" | "primary_link" | "secondary" | "secondary_link"
        | "tertiary" | "tertiary_link" | "unclassified" | "residential" | "living_street" => {
            Some(true)
        }

        // Service roads: resolve by the service=* sub-tag to distinguish publicly-accessible
        // alleys from private/facility-internal access paths.  JCT management roads, toll-gate
        // vehicle lanes, and parking aisles are commonly tagged highway=service; accepting all
        // service ways without sub-tag discrimination would silently misclassify JCT connectors.
        "service" => match service {
            // Clearly private or facility-internal access — not street-level evidence.
            Some(
                "parking_aisle" | "driveway" | "drive-through" | "emergency_access" | "slipway",
            ) => Some(false),
            // Publicly-accessible alleys connecting to the street network.
            Some("alley") => Some(true),
            // Unknown or absent sub-tag: management roads, toll-gate lanes, and
            // other facility-internal roads are common here.  Return None so the
            // caller emits a warning and conservatively excludes from surface_nodes.
            _ => None,
        },

        // Placeholder / unknown classification: may be any road type.
        // Conservative None prevents silent false positives.
        "road" => None,

        // Non-vehicle paths — share nodes with ramps only incidentally
        "footway" | "path" | "pedestrian" | "cycleway" | "steps" | "bridleway" => Some(false),

        // Already captured in the Shutoko graph — not a "surface" road relative to that graph
        "motorway" | "motorway_link" => Some(false),

        // Ambiguous or uncommon values: caller emits a warning and skips conservatively
        _ => None,
    }
}

/// Parse oneway direction from OSM tags.
/// - "yes", "1", "true" -> Forward only
/// - "-1", "reverse" -> Reverse only
/// - other / absent -> Bidirectional
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnewayDirection {
    ForwardOnly,
    ReverseOnly,
    Bidirectional,
}

pub fn parse_oneway(element: &OsmElement) -> OnewayDirection {
    match element.get_tag("oneway") {
        Some("yes" | "1" | "true") => OnewayDirection::ForwardOnly,
        Some("-1" | "reverse") => OnewayDirection::ReverseOnly,
        _ => OnewayDirection::Bidirectional,
    }
}

/// Build `Graph`, `SnapIndex`, and diagnostic `RestrictionReport` from Overpass API elements.
pub fn build_topology_with_report(
    response: &OverpassResponse,
    config: &TopologyConfig,
) -> Result<(Graph, SnapIndex, RestrictionReport), String> {
    // 1. Index nodes with coordinates
    let mut node_coords: HashMap<i64, (f64, f64)> = HashMap::new();
    for elem in &response.elements {
        if elem.is_node() {
            if let (Some(lat), Some(lon)) = (elem.lat, elem.lon) {
                node_coords.insert(elem.id, (lat, lon));
            }
        }
    }

    // Pre-pass: collect surface_nodes from all non-motorway, non-motorway_link highway ways,
    // and build a node OSM-highway-tag index for the secondary classification signal.
    //
    // "Surface nodes" are nodes that belong to at least one way whose highway tag is
    // neither "motorway" nor "motorway_link".  A ramp endpoint node that is in surface_nodes
    // is connected to the ground-level street network — a definitive signal for Entry/Exit.
    //
    // IMPORTANT: these ways are NOT added to intermediate_edges and do not appear in the
    // final graph.  The output graph's Local-edge count remains 0.
    let mut surface_nodes: HashSet<String> = HashSet::new();
    let mut node_highway_tags: HashMap<i64, String> = HashMap::new();
    let mut ambiguous_highway_warnings: Vec<String> = Vec::new();

    for elem in &response.elements {
        if elem.is_node() {
            // Collect each node's OSM highway tag for the secondary classification signal.
            if let Some(hw) = elem.get_tag("highway") {
                node_highway_tags.insert(elem.id, hw.to_string());
            }
            continue;
        }
        if !elem.is_way() {
            continue;
        }
        let highway = match elem.get_tag("highway") {
            Some(h) => h,
            None => continue,
        };
        match is_vehicle_highway(highway, elem.get_tag("service")) {
            Some(true) => {
                // Vehicle-accessible surface road: record its node IDs as evidence that a
                // ramp endpoint touching this way is at street level (Entry or Exit).
                if let Some(nodes) = &elem.nodes {
                    for &nid in nodes {
                        surface_nodes.insert(format!("n:{}", nid));
                    }
                }
            }
            Some(false) => {
                // Non-vehicle path or motorway-network way: not surface-connection evidence.
                // (footways, cycleways, steps, parking aisles, private driveways, etc. can
                // share a node with a ramp incidentally without implying the ramp descends
                // to street level.)
            }
            None => {
                // Ambiguous or unresolvable highway type: conservatively exclude from
                // surface_nodes and warn.  For highway=service without a recognised
                // service= sub-tag, this prevents management roads and toll-gate lanes
                // from silently contributing false Entry/Exit evidence.
                let detail = if highway == "service" {
                    match elem.get_tag("service") {
                        Some(st) => format!(" (service={st}, sub-tag not a recognised public-road type)"),
                        None => " (no service= sub-tag; management roads and toll-gate lanes are common here)".to_string(),
                    }
                } else if highway == "road" {
                    " (placeholder value — highway type not yet determined)".to_string()
                } else {
                    String::new()
                };
                ambiguous_highway_warnings.push(format!(
                    "Way {}: ambiguous or unrecognised highway type {:?}{} — \
                     not counted as surface-connection evidence (conservative)",
                    elem.id, highway, detail
                ));
            }
        }
    }

    for warn in &ambiguous_highway_warnings {
        eprintln!("Warning: {warn}");
    }

    // True if the OSM extract contains at least one surface (non-motorway/motorway_link)
    // road way.  Distinguishes "node is confirmed not on any surface road" from
    // "no surface data present to evaluate".
    let has_surface_context = !surface_nodes.is_empty();

    if !has_surface_context {
        let ambiguous_note = if ambiguous_highway_warnings.is_empty() {
            " No vehicle-accessible surface road ways (e.g. primary, secondary, residential) \
             were found in the extract."
                .to_string()
        } else {
            format!(
                " {} way(s) had ambiguous or unrecognised highway values and were \
                 conservatively excluded (see warnings above); none contributed \
                 vehicle-accessible surface evidence.",
                ambiguous_highway_warnings.len()
            )
        };
        eprintln!(
            "Warning: OSM extract contains no vehicle-accessible surface road ways \
             that can serve as definitive Entry/Exit evidence.{} \
             Ramp Entry/Exit classification will use OSM node tags only (secondary \
             signal). Edges with no discriminating node tag are conservatively classified as \
             Shutoko. Re-run with an updated OSM extract that includes surface roads for \
             accurate classification.",
            ambiguous_note
        );
    }

    // 2. Extract intermediate edges from ways
    let mut intermediate_edges: Vec<IntermediateEdge> = Vec::new();
    let mut way_edges_by_id: HashMap<i64, Vec<String>> = HashMap::new();

    for elem in &response.elements {
        if !elem.is_way() {
            continue;
        }
        let nodes = match &elem.nodes {
            Some(ns) if ns.len() >= 2 => ns,
            _ => continue,
        };

        let highway = match elem.get_tag("highway") {
            Some(h) => h,
            None => continue,
        };

        let (is_motorway_link, tentative_kind) = if is_shutoko_motorway(elem) {
            (false, Some(EdgeKind::Shutoko))
        } else if highway == "motorway_link" {
            (true, None)
        } else {
            // Surface road ways (all non-motorway, non-motorway_link highway types) are
            // context-only: their nodes are captured in surface_nodes (pre-pass above) and
            // used for Entry/Exit vs. Shutoko classification, but they are NOT added as
            // graph edges — the output graph therefore contains zero Local edges.
            continue;
        };

        let oneway = parse_oneway(elem);

        // OSM way name (prefer `name`, fall back to `name:ja`) becomes the
        // edge's `name` tag in the output graph.
        let way_name: Option<String> = elem
            .get_tag("name")
            .or_else(|| elem.get_tag("name:ja"))
            .map(str::to_string);

        for i in 0..(nodes.len() - 1) {
            let u_id = nodes[i];
            let v_id = nodes[i + 1];

            let (u_lat, u_lon) = match node_coords.get(&u_id) {
                Some(c) => *c,
                None => continue,
            };
            let (v_lat, v_lon) = match node_coords.get(&v_id) {
                Some(c) => *c,
                None => continue,
            };

            let dist = haversine_distance_meters(u_lat, u_lon, v_lat, v_lon);
            let u_node_str = format!("n:{}", u_id);
            let v_node_str = format!("n:{}", v_id);

            // Forward direction
            if oneway == OnewayDirection::ForwardOnly || oneway == OnewayDirection::Bidirectional {
                let edge_id = format!("e:w{}:{}:f", elem.id, i);
                intermediate_edges.push(IntermediateEdge {
                    name: way_name.clone(),
                    id: edge_id.clone(),
                    from: u_node_str.clone(),
                    to: v_node_str.clone(),
                    distance_meters: dist,
                    is_motorway_link,
                    tentative_kind,
                });
                way_edges_by_id.entry(elem.id).or_default().push(edge_id);
            }

            // Reverse direction
            if oneway == OnewayDirection::ReverseOnly || oneway == OnewayDirection::Bidirectional {
                let edge_id = format!("e:w{}:{}:r", elem.id, i);
                intermediate_edges.push(IntermediateEdge {
                    name: way_name.clone(),
                    id: edge_id.clone(),
                    from: v_node_str,
                    to: u_node_str,
                    distance_meters: dist,
                    is_motorway_link,
                    tentative_kind,
                });
                way_edges_by_id.entry(elem.id).or_default().push(edge_id);
            }
        }
    }

    // 3. Classify motorway_link edges based on graph topology alone (no local roads required).
    //
    // Design rationale: the new OSM fetch drops general-surface roads, so `local_nodes` no longer
    // exists. Instead we identify the street-side terminus of each ramp by topology:
    //
    //   Entry edge: a motorway_link edge whose `from` node has **zero** incoming edges from any
    //               other ramp (motorway_link) or mainline (motorway/Shutoko) edge, AND whose
    //               forward link-chain eventually reaches the Shutoko mainline.
    //               The zero-incoming-from condition identifies the "surface side" dead-end of an
    //               entry ramp—exactly the node a driver departs from when they enter the
    //               expressway.
    //
    //   Exit edge:  a motorway_link edge whose `to` node has **zero** outgoing edges from any
    //               ramp or mainline edge, AND whose backward link-chain reaches the Shutoko
    //               mainline.  This is the surface-side dead-end of an exit ramp.
    //
    //   Shutoko:    all other motorway_link edges that are connected to the Shutoko network
    //               (intermediate ramp segments, JCT connectors, etc.)
    //
    //   Dropped:    motorway_link edges that cannot reach the Shutoko mainline in either
    //               direction (isolated fragments not part of the analysed network).

    // 3a. Collect nodes that touch Shutoko mainline edges.
    let mut shutoko_nodes: HashSet<String> = HashSet::new();

    for edge in &intermediate_edges {
        if edge.tentative_kind == Some(EdgeKind::Shutoko) {
            shutoko_nodes.insert(edge.from.clone());
            shutoko_nodes.insert(edge.to.clone());
        }
    }

    // 3b. Count incoming / outgoing edges for each node, considering only ramp and mainline
    //     edges (motorway_link and motorway).  Local-road edges are intentionally excluded so
    //     that a ramp terminus node shared with a surface street is still recognised as a
    //     dead-end from the ramp/motorway graph's perspective.
    let mut ramp_motor_incoming: HashMap<String, usize> = HashMap::new();
    let mut ramp_motor_outgoing: HashMap<String, usize> = HashMap::new();

    for edge in &intermediate_edges {
        if edge.tentative_kind == Some(EdgeKind::Shutoko) || edge.is_motorway_link {
            *ramp_motor_outgoing.entry(edge.from.clone()).or_insert(0) += 1;
            *ramp_motor_incoming.entry(edge.to.clone()).or_insert(0) += 1;
        }
    }

    // 3c. Build link adjacency maps for forward/backward reachability through motorway_link edges.
    let mut link_outgoing: HashMap<String, Vec<String>> = HashMap::new();
    let mut link_incoming: HashMap<String, Vec<String>> = HashMap::new();

    for edge in intermediate_edges.iter().filter(|e| e.is_motorway_link) {
        link_outgoing
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
        link_incoming
            .entry(edge.to.clone())
            .or_default()
            .push(edge.from.clone());
    }

    // 3d. Reachability helpers: traverse motorway_link edges only.
    let reaches_target_forward = |start: &str, target_nodes: &HashSet<String>| -> bool {
        if target_nodes.contains(start) {
            return true;
        }
        let mut visited = HashSet::new();
        let mut queue = vec![start.to_string()];
        visited.insert(start.to_string());
        while let Some(curr) = queue.pop() {
            if let Some(nexts) = link_outgoing.get(&curr) {
                for next in nexts {
                    if target_nodes.contains(next) {
                        return true;
                    }
                    if visited.insert(next.clone()) {
                        queue.push(next.clone());
                    }
                }
            }
        }
        false
    };

    let reaches_target_backward = |start: &str, target_nodes: &HashSet<String>| -> bool {
        if target_nodes.contains(start) {
            return true;
        }
        let mut visited = HashSet::new();
        let mut queue = vec![start.to_string()];
        visited.insert(start.to_string());
        while let Some(curr) = queue.pop() {
            if let Some(prevs) = link_incoming.get(&curr) {
                for prev in prevs {
                    if target_nodes.contains(prev) {
                        return true;
                    }
                    if visited.insert(prev.clone()) {
                        queue.push(prev.clone());
                    }
                }
            }
        }
        false
    };

    // 3e. Build final edges with surface-connection-based classification.
    //
    // Primary signal: does the candidate's street-side endpoint node share a node with any
    // non-motorway, non-motorway_link highway way (surface_nodes membership)?
    //   Some(true)  → real surface connection → Entry or Exit
    //   Some(false) → confirmed no surface connection (surface data IS present) → Shutoko
    //   None        → no surface road data in extract → primary signal unavailable
    //
    // Secondary signal: OSM highway tag on the endpoint node.
    //   highway=traffic_signals → surface intersection → Entry/Exit evidence
    //   highway=motorway_junction → grade-separated junction → Shutoko evidence
    //   other / absent → no additional discriminating information
    //
    // Conflict resolution (primary takes precedence):
    //   (a) Primary=surface AND secondary=motorway_junction → warn, use primary → Entry/Exit
    //   (b) Primary=no-surface AND secondary=traffic_signals → warn, use primary → Shutoko
    //   (c) Primary unavailable AND secondary=traffic_signals → Entry/Exit
    //   (d) Primary unavailable AND secondary=motorway_junction → Shutoko
    //   (e) Primary unavailable AND secondary unavailable → undecidable:
    //       log warning, use conservative default → Shutoko
    //       (JCT connectors misclassified as Entry/Exit are more harmful: a phantom
    //        snap origin causes silent routing failures, while a real ramp defaulted
    //        to Shutoko is simply omitted from the snap index.)
    let mut final_edges: Vec<Edge> = Vec::new();
    let mut final_edge_ids: HashSet<String> = HashSet::new();
    let mut dropped_link_edges = 0usize;
    let mut undecidable_ramp_count = 0usize;
    let mut classification_warnings: Vec<String> = Vec::new();

    for edge in intermediate_edges {
        let kind = if let Some(k) = edge.tentative_kind {
            // Shutoko mainline edges pass through unchanged.
            // (Local edges are no longer created in step 2.)
            k
        } else if edge.is_motorway_link {
            let incoming_at_from = *ramp_motor_incoming.get(&edge.from).unwrap_or(&0);
            let outgoing_from_to = *ramp_motor_outgoing.get(&edge.to).unwrap_or(&0);

            let forward_to_shutoko = reaches_target_forward(&edge.to, &shutoko_nodes);
            let backward_from_shutoko = reaches_target_backward(&edge.from, &shutoko_nodes);

            if incoming_at_from == 0 && forward_to_shutoko {
                // Entry candidate: no ramp/mainline edges flow into the from-node and the
                // forward chain reaches the Shutoko mainline.
                let node_str = &edge.from;
                let on_surface = surface_nodes.contains(node_str.as_str());
                let primary: Option<bool> = if has_surface_context {
                    Some(on_surface)
                } else {
                    None
                };
                let tag_hw: Option<&str> = node_str
                    .strip_prefix("n:")
                    .and_then(|s| s.parse::<i64>().ok())
                    .and_then(|id| node_highway_tags.get(&id))
                    .map(|s| s.as_str());
                let secondary: Option<bool> = match tag_hw {
                    Some("traffic_signals") => Some(true),
                    Some("motorway_junction") => Some(false),
                    _ => None,
                };

                match (primary, secondary) {
                    (Some(true), sec) => {
                        if sec == Some(false) {
                            classification_warnings.push(format!(
                                "entry candidate {}: surface-connection=yes conflicts with \
                                 highway={} (secondary=Shutoko); using surface-connection \
                                 (primary) → Entry",
                                node_str,
                                tag_hw.unwrap_or("?")
                            ));
                        }
                        EdgeKind::Entry
                    }
                    (Some(false), sec) => {
                        if sec == Some(true) {
                            classification_warnings.push(format!(
                                "entry candidate {}: no surface connection conflicts with \
                                 highway=traffic_signals (secondary=Entry); using \
                                 surface-connection (primary) → Shutoko",
                                node_str
                            ));
                        }
                        EdgeKind::Shutoko
                    }
                    (None, Some(true)) => EdgeKind::Entry,
                    (None, Some(false)) => EdgeKind::Shutoko,
                    (None, None) => {
                        // Undecidable: no surface context and no discriminating node tag.
                        // Conservative default: Shutoko (see rationale in step 3e header).
                        undecidable_ramp_count += 1;
                        classification_warnings.push(format!(
                            "entry candidate {}: no surface-road context and no discriminating \
                             OSM node tag → undecidable, conservatively classified as Shutoko",
                            node_str
                        ));
                        EdgeKind::Shutoko
                    }
                }
            } else if outgoing_from_to == 0 && backward_from_shutoko {
                // Exit candidate: no ramp/mainline edges leave the to-node and the backward
                // chain reaches the Shutoko mainline.
                let node_str = &edge.to;
                let on_surface = surface_nodes.contains(node_str.as_str());
                let primary: Option<bool> = if has_surface_context {
                    Some(on_surface)
                } else {
                    None
                };
                let tag_hw: Option<&str> = node_str
                    .strip_prefix("n:")
                    .and_then(|s| s.parse::<i64>().ok())
                    .and_then(|id| node_highway_tags.get(&id))
                    .map(|s| s.as_str());
                let secondary: Option<bool> = match tag_hw {
                    Some("traffic_signals") => Some(true),
                    Some("motorway_junction") => Some(false),
                    _ => None,
                };

                match (primary, secondary) {
                    (Some(true), sec) => {
                        if sec == Some(false) {
                            classification_warnings.push(format!(
                                "exit candidate {}: surface-connection=yes conflicts with \
                                 highway={} (secondary=Shutoko); using surface-connection \
                                 (primary) → Exit",
                                node_str,
                                tag_hw.unwrap_or("?")
                            ));
                        }
                        EdgeKind::Exit
                    }
                    (Some(false), sec) => {
                        if sec == Some(true) {
                            classification_warnings.push(format!(
                                "exit candidate {}: no surface connection conflicts with \
                                 highway=traffic_signals (secondary=Exit); using \
                                 surface-connection (primary) → Shutoko",
                                node_str
                            ));
                        }
                        EdgeKind::Shutoko
                    }
                    (None, Some(true)) => EdgeKind::Exit,
                    (None, Some(false)) => EdgeKind::Shutoko,
                    (None, None) => {
                        undecidable_ramp_count += 1;
                        classification_warnings.push(format!(
                            "exit candidate {}: no surface-road context and no discriminating \
                             OSM node tag → undecidable, conservatively classified as Shutoko",
                            node_str
                        ));
                        EdgeKind::Shutoko
                    }
                }
            } else if forward_to_shutoko || backward_from_shutoko {
                // Intermediate ramp segment or JCT connector connected to Shutoko but not
                // at a street-side dead-end; treat as part of the expressway network.
                EdgeKind::Shutoko
            } else {
                // Motorway_link edge that cannot reach the Shutoko mainline in either
                // direction — isolated fragment, drop it.
                dropped_link_edges += 1;
                continue;
            }
        } else {
            // Unreachable: non-motorway, non-link ways are filtered at the way-parsing
            // stage above.
            continue;
        };

        let speed = match kind {
            EdgeKind::Shutoko => {
                if edge.is_motorway_link {
                    RAMP_SPEED_KMH
                } else {
                    SHUTOKO_SPEED_KMH
                }
            }
            EdgeKind::Entry | EdgeKind::Exit => RAMP_SPEED_KMH,
            // NOTE: Local edges are never created in the way-parsing step above —
            // all non-motorway, non-motorway_link ways are skipped and do not appear
            // in intermediate_edges.  This arm is kept for exhaustiveness to guard
            // against future EdgeKind additions, but is unreachable at runtime.
            EdgeKind::Local => LOCAL_SPEED_KMH,
        };

        let duration = duration_seconds(edge.distance_meters, speed);
        final_edge_ids.insert(edge.id.clone());
        final_edges.push(Edge {
            id: edge.id,
            from: edge.from,
            to: edge.to,
            kind,
            duration_seconds: duration,
            distance_meters: edge.distance_meters,
            name: edge.name,
        });
    }

    // 4. Collect used nodes
    let mut active_node_ids: BTreeSet<String> = BTreeSet::new();
    for edge in &final_edges {
        active_node_ids.insert(edge.from.clone());
        active_node_ids.insert(edge.to.clone());
    }

    // Node IDs are always built as "n:<osm node id>", so coordinates can be
    // recovered from the index. Edges only exist when both endpoints had coords.
    let mut nodes: Vec<Node> = active_node_ids
        .iter()
        .map(|id| {
            let osm_id: i64 = id
                .strip_prefix("n:")
                .and_then(|s| s.parse().ok())
                .expect("node id must be n:<osm node id>");
            let (lat, lon) = *node_coords
                .get(&osm_id)
                .expect("node coordinate must exist for edge endpoint");
            Node {
                id: id.clone(),
                lat,
                lon,
            }
        })
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    // Map final edge IDs to edge references and build outgoing index
    let final_edges_by_id: HashMap<String, &Edge> =
        final_edges.iter().map(|e| (e.id.clone(), e)).collect();
    let mut final_outgoing: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &final_edges {
        final_outgoing
            .entry(edge.from.clone())
            .or_default()
            .push(edge.id.clone());
    }
    for list in final_outgoing.values_mut() {
        list.sort();
    }

    // 5. Parse turn restrictions from relations
    // Emit undecidable-classification warnings before report construction.
    if undecidable_ramp_count > 0 {
        eprintln!(
            "Warning: {} ramp edge(s) could not be classified as Entry/Exit vs. Shutoko \
             (no surface-road context and no discriminating OSM node tag); conservatively \
             classified as Shutoko. Update OSM extract with surface roads via fetch-osm.sh \
             for accurate Entry/Exit classification.",
            undecidable_ramp_count
        );
    }

    let mut forbidden_transitions_set: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut report = RestrictionReport {
        dropped_link_edges,
        undecidable_ramp_edges: undecidable_ramp_count,
        notes: classification_warnings,
        ..Default::default()
    };

    for elem in &response.elements {
        if !elem.is_relation() {
            continue;
        }
        if elem.get_tag("type") != Some("restriction") {
            continue;
        }

        report.total_relations += 1;

        let restriction = match elem.get_tag("restriction") {
            Some(r) => r,
            None => {
                if let Some(cond) = elem.get_tag("restriction:conditional") {
                    report.skipped_conditional += 1;
                    report.notes.push(format!(
                        "relation {}: conditional restriction ({}) skipped (traffic regulation out of scope)",
                        elem.id, cond
                    ));
                } else {
                    report.skipped_missing_elements += 1;
                    report
                        .notes
                        .push(format!("relation {}: missing restriction tag", elem.id));
                }
                continue;
            }
        };

        let is_no_turn = matches!(
            restriction,
            "no_left_turn" | "no_right_turn" | "no_u_turn" | "no_straight_on"
        );
        let is_only_turn = matches!(
            restriction,
            "only_left_turn" | "only_right_turn" | "only_straight_on" | "only_u_turn"
        );

        if !is_no_turn && !is_only_turn {
            report.skipped_unrecognized += 1;
            report.notes.push(format!(
                "relation {}: unrecognized restriction value \"{}\"",
                elem.id, restriction
            ));
            continue;
        }

        let members = match &elem.members {
            Some(m) => m,
            None => {
                report.skipped_no_via += 1;
                continue;
            }
        };

        let mut from_ways: Vec<i64> = Vec::new();
        let mut to_ways: Vec<i64> = Vec::new();
        let mut via_nodes: Vec<i64> = Vec::new();
        let mut via_ways: Vec<i64> = Vec::new();

        for m in members {
            if m.role == "from" && m.member_type == "way" {
                from_ways.push(m.ref_id);
            } else if m.role == "to" && m.member_type == "way" {
                to_ways.push(m.ref_id);
            } else if m.role == "via" {
                if m.member_type == "node" {
                    via_nodes.push(m.ref_id);
                } else if m.member_type == "way" {
                    via_ways.push(m.ref_id);
                }
            }
        }

        if from_ways.is_empty() || to_ways.is_empty() {
            report.skipped_missing_elements += 1;
            report.notes.push(format!(
                "relation {}: missing from or to way member",
                elem.id
            ));
            continue;
        }

        if via_nodes.is_empty() && via_ways.is_empty() {
            report.skipped_no_via += 1;
            report
                .notes
                .push(format!("relation {}: missing via member", elem.id));
            continue;
        }

        let fw = from_ways[0];
        let tw = to_ways[0];

        // 5a. Handle via=node
        if let Some(&vn) = via_nodes.first() {
            let via_node_str = format!("n:{}", vn);

            let fw_edges = match way_edges_by_id.get(&fw) {
                Some(es) => es,
                None => {
                    report.skipped_missing_elements += 1;
                    report.notes.push(format!(
                        "relation {}: from way {} not in graph",
                        elem.id, fw
                    ));
                    continue;
                }
            };
            let tw_edges = match way_edges_by_id.get(&tw) {
                Some(es) => es,
                None => {
                    report.skipped_missing_elements += 1;
                    report
                        .notes
                        .push(format!("relation {}: to way {} not in graph", elem.id, tw));
                    continue;
                }
            };

            let fe_candidates: Vec<&String> = fw_edges
                .iter()
                .filter(|eid| {
                    final_edges_by_id
                        .get(*eid)
                        .is_some_and(|e| e.to == via_node_str)
                })
                .collect();

            let te_candidates: Vec<&String> = tw_edges
                .iter()
                .filter(|eid| {
                    final_edges_by_id
                        .get(*eid)
                        .is_some_and(|e| e.from == via_node_str)
                })
                .collect();

            if fe_candidates.is_empty() || te_candidates.is_empty() {
                report.skipped_missing_elements += 1;
                report.notes.push(format!(
                    "relation {}: no active edges connecting via node {} (fw:{}, tw:{})",
                    elem.id, vn, fw, tw
                ));
                continue;
            }

            if is_no_turn {
                for fe_id in &fe_candidates {
                    for te_id in &te_candidates {
                        forbidden_transitions_set.insert(vec![(*fe_id).clone(), (*te_id).clone()]);
                    }
                }
                report.no_turn_via_node += 1;
            } else if is_only_turn {
                let to_ids: HashSet<&str> = te_candidates.iter().map(|s| s.as_str()).collect();
                let all_outgoing = final_outgoing.get(&via_node_str);
                let mut added_pairs = 0;

                if let Some(outgoing) = all_outgoing {
                    for fe_id in &fe_candidates {
                        for out_id in outgoing {
                            if !to_ids.contains(out_id.as_str())
                                && forbidden_transitions_set
                                    .insert(vec![(*fe_id).clone(), out_id.clone()])
                            {
                                added_pairs += 1;
                            }
                        }
                    }
                }

                report.only_turn_via_node += 1;
                report.only_turn_edge_pairs += added_pairs;
            }
            continue;
        }

        // 5b. Handle via=way
        if !via_ways.is_empty() {
            let fw_edges = match way_edges_by_id.get(&fw) {
                Some(es) => es,
                None => {
                    report.skipped_missing_elements += 1;
                    report.notes.push(format!(
                        "relation {}: from way {} not in graph",
                        elem.id, fw
                    ));
                    continue;
                }
            };
            let tw_edges = match way_edges_by_id.get(&tw) {
                Some(es) => es,
                None => {
                    report.skipped_missing_elements += 1;
                    report
                        .notes
                        .push(format!("relation {}: to way {} not in graph", elem.id, tw));
                    continue;
                }
            };

            let mut all_via_edges = Vec::new();
            let mut missing_via = false;
            for vw in &via_ways {
                match way_edges_by_id.get(vw) {
                    Some(es) => {
                        for eid in es {
                            if final_edge_ids.contains(eid) {
                                all_via_edges.push(eid.clone());
                            }
                        }
                    }
                    None => {
                        missing_via = true;
                        break;
                    }
                }
            }

            if missing_via || all_via_edges.is_empty() {
                report.skipped_missing_elements += 1;
                report.notes.push(format!(
                    "relation {}: via ways not fully present in graph",
                    elem.id
                ));
                continue;
            }

            let to_edge_set: HashSet<&str> = tw_edges
                .iter()
                .filter(|id| final_edge_ids.contains(*id))
                .map(String::as_str)
                .collect();

            if to_edge_set.is_empty() {
                report.skipped_missing_elements += 1;
                continue;
            }

            if is_only_turn {
                report.skipped_only_via_way += 1;
                report.notes.push(format!(
                    "relation {}: only_* turn restriction with via=way is unsupported (omitted from static graph)",
                    elem.id
                ));
                continue;
            }

            let mut found_any_path = false;

            for fe_id in fw_edges {
                if !final_edge_ids.contains(fe_id) {
                    continue;
                }
                let _fe = match final_edges_by_id.get(fe_id) {
                    Some(e) => e,
                    None => continue,
                };

                let mut queue: Vec<Vec<String>> = vec![vec![fe_id.clone()]];
                let mut visited_path_prefixes: HashSet<String> = HashSet::new();

                while let Some(path) = queue.pop() {
                    let last_edge_id = path.last().unwrap();
                    let last_edge = match final_edges_by_id.get(last_edge_id) {
                        Some(e) => e,
                        None => continue,
                    };

                    if to_edge_set.contains(last_edge_id.as_str()) && path.len() >= 3 {
                        if is_no_turn {
                            forbidden_transitions_set.insert(path.clone());
                            found_any_path = true;
                        }
                        continue;
                    }

                    if path.len() > via_ways.len() + 10 {
                        continue;
                    }

                    if let Some(nexts) = final_outgoing.get(&last_edge.to) {
                        for next_id in nexts {
                            if path.contains(next_id) {
                                continue;
                            }
                            let has_via = path.iter().skip(1).any(|id| all_via_edges.contains(id));
                            let is_via = all_via_edges.contains(next_id);
                            let is_to = to_edge_set.contains(next_id.as_str());

                            if is_via || (has_via && is_to) {
                                let mut new_path = path.clone();
                                new_path.push(next_id.clone());
                                let path_key = new_path.join("->");
                                if visited_path_prefixes.insert(path_key) {
                                    queue.push(new_path);
                                }
                            }
                        }
                    }
                }
            }

            if found_any_path {
                report.via_way += 1;
            } else {
                report.skipped_disconnected += 1;
                report.notes.push(format!(
                    "relation {}: disconnected via-way path between from way {} and to way {}",
                    elem.id, fw, tw
                ));
            }
        }
    }

    if report.dropped_link_edges > 0 {
        eprintln!(
            "Info: dropped {} internal motorway_link edges (not connecting local streets and Shutoko)",
            report.dropped_link_edges
        );
    }
    eprintln!(
        "Turn restrictions: {} total relations -> {} no_turn (via=node), {} only_turn (via=node, {} forbidden pairs), {} via_way | skipped: {} conditional, {} no via, {} outside graph, {} disconnected{}{}",
        report.total_relations,
        report.no_turn_via_node,
        report.only_turn_via_node,
        report.only_turn_edge_pairs,
        report.via_way,
        report.skipped_conditional,
        report.skipped_no_via,
        report.skipped_missing_elements,
        report.skipped_disconnected,
        if report.skipped_only_via_way > 0 {
            format!(", {} only via-way", report.skipped_only_via_way)
        } else {
            String::new()
        },
        if report.skipped_unrecognized > 0 {
            format!(", {} unrecognized", report.skipped_unrecognized)
        } else {
            String::new()
        }
    );

    let mut forbidden_transitions: Vec<Vec<String>> =
        forbidden_transitions_set.into_iter().collect();
    forbidden_transitions.sort();

    // Sort edges deterministically by ID
    final_edges.sort_by(|a, b| a.id.cmp(&b.id));

    let graph = Graph {
        schema_version: 2,
        release_id: config.release_id.clone(),
        vehicle_profile: config.vehicle_profile.clone(),
        nodes,
        edges: final_edges,
        billing_pairs: Vec::new(),
        forbidden_transitions,
        ramps: Vec::new(),
        od_tariffs: Vec::new(),
    };

    // 6. Build SnapIndex for Entry ramp origin nodes (schema_version 2).
    //
    // Each Entry edge's `from` node is the street-accessible terminus of an entry ramp —
    // the point a driver stands at when they are about to enter the expressway.  These are
    // the nodes used for geographic snapping of a user's origin to the nearest entry point.
    //
    // A node appears at most once: the Entry condition (zero ramp/mainline incoming) ensures
    // no two distinct Entry edges share the same `from` node, so the snap count equals the
    // Entry edge count.
    let mut snap_nodes: Vec<SnapNode> = Vec::new();
    let entry_from_ids: HashSet<&str> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Entry)
        .map(|e| e.from.as_str())
        .collect();

    for node in &graph.nodes {
        if entry_from_ids.contains(node.id.as_str()) {
            if let Some(num_str) = node.id.strip_prefix("n:") {
                if let Ok(osm_id) = num_str.parse::<i64>() {
                    if let Some(&(lat, lon)) = node_coords.get(&osm_id) {
                        snap_nodes.push(SnapNode {
                            id: node.id.clone(),
                            lat,
                            lon,
                        });
                    }
                }
            }
        }
    }
    snap_nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let snap_index = SnapIndex {
        schema_version: 2,
        release_id: config.release_id.clone(),
        nodes: snap_nodes,
    };

    Ok((graph, snap_index, report))
}

/// Build `Graph` and `SnapIndex` from Overpass API elements.
pub fn build_topology(
    response: &OverpassResponse,
    config: &TopologyConfig,
) -> Result<(Graph, SnapIndex), String> {
    let (graph, snap_index, _report) = build_topology_with_report(response, config)?;
    Ok((graph, snap_index))
}

/// Serialize Graph deterministically with 2-space indentation and trailing newline.
pub fn to_deterministic_json(graph: &Graph) -> Result<String, serde_json::Error> {
    let mut s = serde_json::to_string_pretty(graph)?;
    s.push('\n');
    Ok(s)
}

/// Serialize SnapIndex deterministically with 2-space indentation and trailing newline.
pub fn snap_index_to_deterministic_json(
    snap_index: &SnapIndex,
) -> Result<String, serde_json::Error> {
    let mut s = serde_json::to_string_pretty(snap_index)?;
    s.push('\n');
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_distance() {
        // Tokyo Station (approx 35.681236, 139.767125) to Ginza (approx 35.671989, 139.763965) ~ 1.06 km
        let dist = haversine_distance_meters(35.681236, 139.767125, 35.671989, 139.763965);
        assert!(dist > 1000 && dist < 1200, "Distance was {}", dist);
    }

    #[test]
    fn test_duration_seconds() {
        // 60 km/h = 16.666... m/s. For 1000m, duration is ~60s
        let dur = duration_seconds(1000, 60.0);
        assert_eq!(dur, 60);

        // 40 km/h = 11.111... m/s. For 1000m, duration is ~90s
        let dur_ramp = duration_seconds(1000, 40.0);
        assert_eq!(dur_ramp, 90);

        // 30 km/h = 8.333... m/s. For 1000m, duration is ~120s
        let dur_local = duration_seconds(1000, 30.0);
        assert_eq!(dur_local, 120);
    }

    // -------------------------------------------------------------------------
    // Tests for the surface-connection-based Entry/Exit classification
    // (issue #32: replace brittle name-pattern and distance heuristics with
    // structural OSM signals)
    // -------------------------------------------------------------------------

    /// Helper: build a minimal OverpassResponse from JSON-like element descriptors.
    /// Each element is (type, id, lat, lon, nodes, tags).
    /// For simplicity we use serde_json for the full pipeline.
    fn build_response_from_json(json_str: &str) -> crate::osm::OverpassResponse {
        serde_json::from_str(json_str).expect("test JSON must parse")
    }

    /// With a surface road connecting to the entry from-node, the ramp is classified
    /// as Entry even when no name/distance heuristic is available.
    #[test]
    fn test_surface_connection_entry_classified_as_entry() {
        // Shutoko loop: n10 → n11 → n12 → n10 (motorway C1)
        // Surface road: n1 → n2 (highway=primary)
        // Entry ramp: n2 → n5 → n10 (motorway_link, oneway)
        // n2 is in surface_nodes (part of primary way) → Entry
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":1,"lat":35.6800,"lon":139.7600},
                {"type":"node","id":2,"lat":35.6810,"lon":139.7600},
                {"type":"node","id":5,"lat":35.6815,"lon":139.7620},
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"way","id":1,"nodes":[1,2],"tags":{"highway":"primary","oneway":"yes"}},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":200,"nodes":[2,5,10],"tags":{"highway":"motorway_link","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let kinds: std::collections::HashMap<&str, crate::model::EdgeKind> = graph
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.kind))
            .collect();
        assert_eq!(
            kinds.get("e:w200:0:f"),
            Some(&crate::model::EdgeKind::Entry),
            "entry ramp from-node n:2 is in surface_nodes → Entry"
        );
        assert_eq!(report.undecidable_ramp_edges, 0);
    }

    /// With a surface road connecting to the exit to-node, the ramp is classified
    /// as Exit even without name/distance heuristics.
    #[test]
    fn test_surface_connection_exit_classified_as_exit() {
        // Shutoko loop: n10 → n11 → n12 → n10 (motorway C1)
        // Exit ramp: n11 → n25 → n20 (motorway_link, oneway)
        // Surface road: n20 → n21 (highway=primary)
        // n20 is in surface_nodes → Exit
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"node","id":20,"lat":35.6850,"lon":139.7600},
                {"type":"node","id":21,"lat":35.6860,"lon":139.7600},
                {"type":"node","id":25,"lat":35.6835,"lon":139.7620},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":300,"nodes":[11,25,20],"tags":{"highway":"motorway_link","oneway":"yes"}},
                {"type":"way","id":2,"nodes":[20,21],"tags":{"highway":"primary","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let kinds: std::collections::HashMap<&str, crate::model::EdgeKind> = graph
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.kind))
            .collect();
        assert_eq!(
            kinds.get("e:w300:1:f"),
            Some(&crate::model::EdgeKind::Exit),
            "exit ramp to-node n:20 is in surface_nodes → Exit"
        );
        assert_eq!(report.undecidable_ramp_edges, 0);
    }

    /// Without surface road data, an entry/exit candidate whose endpoint node has no
    /// discriminating OSM tag is classified as Shutoko (conservative default) and
    /// counted as undecidable.
    #[test]
    fn test_no_surface_context_undecidable_defaults_to_shutoko() {
        // No surface roads. Entry candidate n:1 (no highway tag) → undecidable → Shutoko.
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":1,"lat":35.6800,"lon":139.7600},
                {"type":"node","id":5,"lat":35.6815,"lon":139.7620},
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":200,"nodes":[1,5,10],"tags":{"highway":"motorway_link","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let kinds: std::collections::HashMap<&str, crate::model::EdgeKind> = graph
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.kind))
            .collect();
        assert_eq!(
            kinds.get("e:w200:0:f"),
            Some(&crate::model::EdgeKind::Shutoko),
            "no surface data + no OSM tag → undecidable → Shutoko"
        );
        assert!(
            report.undecidable_ramp_edges >= 1,
            "undecidable edges must be counted; got {}",
            report.undecidable_ramp_edges
        );
        assert!(
            report.notes.iter().any(|n| n.contains("undecidable")),
            "undecidable classification must appear in notes"
        );
    }

    /// Without surface road data, the secondary signal highway=traffic_signals on the
    /// entry from-node overrides the undecidable default and classifies as Entry.
    #[test]
    fn test_traffic_signals_secondary_signal_entry() {
        // Node 1 has highway=traffic_signals (surface intersection evidence).
        // No surface road ways → primary signal absent → use secondary → Entry.
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":1,"lat":35.6800,"lon":139.7600,"tags":{"highway":"traffic_signals"}},
                {"type":"node","id":5,"lat":35.6815,"lon":139.7620},
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":200,"nodes":[1,5,10],"tags":{"highway":"motorway_link","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let kinds: std::collections::HashMap<&str, crate::model::EdgeKind> = graph
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.kind))
            .collect();
        assert_eq!(
            kinds.get("e:w200:0:f"),
            Some(&crate::model::EdgeKind::Entry),
            "highway=traffic_signals on from-node → Entry (secondary signal)"
        );
        assert_eq!(report.undecidable_ramp_edges, 0);
    }

    /// Without surface road data, highway=motorway_junction on the entry from-node
    /// overrides the undecidable default and classifies as Shutoko (JCT evidence).
    #[test]
    fn test_motorway_junction_secondary_signal_shutoko() {
        // Node 1 has highway=motorway_junction (JCT evidence).
        // No surface road ways → primary absent → secondary → Shutoko.
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":1,"lat":35.6800,"lon":139.7600,"tags":{"highway":"motorway_junction"}},
                {"type":"node","id":5,"lat":35.6815,"lon":139.7620},
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":200,"nodes":[1,5,10],"tags":{"highway":"motorway_link","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let kinds: std::collections::HashMap<&str, crate::model::EdgeKind> = graph
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.kind))
            .collect();
        assert_eq!(
            kinds.get("e:w200:0:f"),
            Some(&crate::model::EdgeKind::Shutoko),
            "highway=motorway_junction on from-node → Shutoko (secondary signal)"
        );
        assert_eq!(report.undecidable_ramp_edges, 0);
    }

    /// Surface road connection takes precedence over a contradicting motorway_junction tag.
    /// The node is in surface_nodes (primary=true) but also has highway=motorway_junction
    /// (secondary=Shutoko). Primary wins → Entry, with a warning in notes.
    #[test]
    fn test_surface_connection_overrides_motorway_junction_tag() {
        // n:2 is in the primary surface road AND has highway=motorway_junction tag.
        // Primary signal (surface connection) takes precedence → Entry + warning.
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":1,"lat":35.6800,"lon":139.7600},
                {"type":"node","id":2,"lat":35.6810,"lon":139.7600,"tags":{"highway":"motorway_junction","name":"TestJCT"}},
                {"type":"node","id":5,"lat":35.6815,"lon":139.7620},
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"way","id":1,"nodes":[1,2],"tags":{"highway":"primary","oneway":"yes"}},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":200,"nodes":[2,5,10],"tags":{"highway":"motorway_link","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let kinds: std::collections::HashMap<&str, crate::model::EdgeKind> = graph
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.kind))
            .collect();
        assert_eq!(
            kinds.get("e:w200:0:f"),
            Some(&crate::model::EdgeKind::Entry),
            "surface connection (primary) overrides motorway_junction tag (secondary) → Entry"
        );
        assert_eq!(report.undecidable_ramp_edges, 0);
        assert!(
            report.notes.iter().any(|n| n.contains("conflicts with")),
            "contradiction between primary and secondary signals must be noted; notes={:?}",
            report.notes
        );
    }

    /// Surface road data is present but the exit to-node is NOT connected to any surface
    /// road (primary=Some(false)) → confirmed Shutoko. This correctly rejects a JCT
    /// connector whose end-point is entirely within the motorway network.
    #[test]
    fn test_surface_context_present_no_connection_shutoko() {
        // Entry ramp has a surface connection (n:2 in primary way).
        // Exit candidate n:20 is NOT connected to any surface road → Shutoko.
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":1,"lat":35.6800,"lon":139.7600},
                {"type":"node","id":2,"lat":35.6810,"lon":139.7600},
                {"type":"node","id":5,"lat":35.6815,"lon":139.7620},
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"node","id":20,"lat":35.6850,"lon":139.7600},
                {"type":"node","id":25,"lat":35.6835,"lon":139.7620},
                {"type":"way","id":1,"nodes":[1,2],"tags":{"highway":"primary","oneway":"yes"}},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":200,"nodes":[2,5,10],"tags":{"highway":"motorway_link","oneway":"yes"}},
                {"type":"way","id":300,"nodes":[11,25,20],"tags":{"highway":"motorway_link","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let kinds: std::collections::HashMap<&str, crate::model::EdgeKind> = graph
            .edges
            .iter()
            .map(|e| (e.id.as_str(), e.kind))
            .collect();
        // Entry ramp (n:2 in surface_nodes) → Entry
        assert_eq!(
            kinds.get("e:w200:0:f"),
            Some(&crate::model::EdgeKind::Entry)
        );
        // Exit candidate n:20 NOT in surface_nodes (surface context IS present) → Shutoko
        assert_eq!(
            kinds.get("e:w300:1:f"),
            Some(&crate::model::EdgeKind::Shutoko),
            "exit to-node n:20 not in surface_nodes despite surface context being present → Shutoko (JCT)"
        );
        assert_eq!(report.undecidable_ramp_edges, 0);
    }

    /// Verify that surface road ways do NOT produce graph edges — only motorway and
    /// motorway_link ways appear in the output graph.
    #[test]
    fn test_surface_roads_not_added_as_edges() {
        let resp = build_response_from_json(
            r#"{
            "elements": [
                {"type":"node","id":1,"lat":35.6800,"lon":139.7600},
                {"type":"node","id":2,"lat":35.6810,"lon":139.7600},
                {"type":"node","id":3,"lat":35.6815,"lon":139.7620},
                {"type":"node","id":10,"lat":35.6820,"lon":139.7640},
                {"type":"node","id":11,"lat":35.6820,"lon":139.7680},
                {"type":"node","id":12,"lat":35.6840,"lon":139.7660},
                {"type":"way","id":1,"nodes":[1,2],"tags":{"highway":"primary","oneway":"yes"}},
                {"type":"way","id":2,"nodes":[2,3],"tags":{"highway":"secondary","oneway":"yes"}},
                {"type":"way","id":100,"nodes":[10,11],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":101,"nodes":[11,12],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}},
                {"type":"way","id":102,"nodes":[12,10],"tags":{"highway":"motorway","ref":"C1","oneway":"yes"}}
            ]
        }"#,
        );
        let (graph, _snap, _report) =
            build_topology_with_report(&resp, &TopologyConfig::default()).unwrap();
        let edge_ids: std::collections::HashSet<&str> =
            graph.edges.iter().map(|e| e.id.as_str()).collect();
        // Surface road edges must NOT appear in the graph
        assert!(
            !edge_ids.contains("e:w1:0:f"),
            "primary road must not produce a graph edge"
        );
        assert!(
            !edge_ids.contains("e:w2:0:f"),
            "secondary road must not produce a graph edge"
        );
        // Shutoko edges must be present
        assert!(edge_ids.contains("e:w100:0:f"), "Shutoko edge must exist");
        // No Local edges
        let local_count = graph
            .edges
            .iter()
            .filter(|e| e.kind == crate::model::EdgeKind::Local)
            .count();
        assert_eq!(local_count, 0, "Local edges must not be created");
    }
}
