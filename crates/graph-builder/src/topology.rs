//! Directed graph topology builder from OSM Overpass elements.
//!
//! # Design & Determinism
//! - Ways are segmented into consecutive node pairs connected strictly by shared OSM node ID.
//! - Non-overlapping layers (bridge, tunnel, layer) that cross geometrically without sharing
//!   a node ID are NEVER connected.
//! - Forward/reverse edges for `oneway` tags are expanded deterministically with unique IDs.
//! - Edge distances use the Haversine formula with Earth radius R = 6,371,000.0 m, rounded to u64.
//! - Durations use static speed models per `EdgeKind` with rounding to u64 seconds.
//! - All IDs, nodes, edges, and forbidden transitions are sorted in deterministic ascending order.

use crate::model::{Edge, EdgeKind, Graph, Node, SnapIndex, SnapNode};
use crate::osm::{OsmElement, OverpassResponse};
use std::collections::{BTreeSet, HashMap, HashSet};

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
    id: String,
    from: String,
    to: String,
    distance_meters: u64,
    is_motorway_link: bool,
    tentative_kind: Option<EdgeKind>,
}

/// Determines whether a way's tags identify it as Shutoko mainline.
/// Inspects `operator`, `network`, and `ref` tags.
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

    false
}

/// Checks if highway tag indicates a surface street / local road.
pub fn is_local_highway(highway_val: &str) -> bool {
    matches!(
        highway_val,
        "trunk" | "primary" | "secondary" | "tertiary" | "unclassified" | "residential"
    )
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

/// Build `Graph` and `SnapIndex` from Overpass API elements.
pub fn build_topology(
    response: &OverpassResponse,
    config: &TopologyConfig,
) -> Result<(Graph, SnapIndex), String> {
    // 1. Index nodes with coordinates
    let mut node_coords: HashMap<i64, (f64, f64)> = HashMap::new();
    for elem in &response.elements {
        if elem.is_node() {
            if let (Some(lat), Some(lon)) = (elem.lat, elem.lon) {
                node_coords.insert(elem.id, (lat, lon));
            }
        }
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
        } else if is_local_highway(highway) {
            (false, Some(EdgeKind::Local))
        } else {
            // Not a recognized roadway type
            continue;
        };

        let oneway = parse_oneway(elem);

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

    // 3. Classify motorway_link edges into Entry or Exit based on connectivity
    // Identify nodes that touch Shutoko or Local edges
    let mut shutoko_nodes: HashSet<String> = HashSet::new();
    let mut local_nodes: HashSet<String> = HashSet::new();

    for edge in &intermediate_edges {
        match edge.tentative_kind {
            Some(EdgeKind::Shutoko) => {
                shutoko_nodes.insert(edge.from.clone());
                shutoko_nodes.insert(edge.to.clone());
            }
            Some(EdgeKind::Local) => {
                local_nodes.insert(edge.from.clone());
                local_nodes.insert(edge.to.clone());
            }
            _ => {}
        }
    }

    // Graph of link edges for reachability analysis
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

    // Reachability helpers across link edges
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

    // Build final edges
    let mut final_edges: Vec<Edge> = Vec::new();
    let mut final_edge_ids: HashSet<String> = HashSet::new();

    for edge in intermediate_edges {
        let kind = if let Some(k) = edge.tentative_kind {
            k
        } else if edge.is_motorway_link {
            let forward_to_shutoko = reaches_target_forward(&edge.to, &shutoko_nodes);
            let backward_from_local = reaches_target_backward(&edge.from, &local_nodes);

            let forward_to_local = reaches_target_forward(&edge.to, &local_nodes);
            let backward_from_shutoko = reaches_target_backward(&edge.from, &shutoko_nodes);

            if forward_to_shutoko && backward_from_local && !backward_from_shutoko {
                EdgeKind::Entry
            } else if forward_to_local && backward_from_shutoko && !backward_from_local {
                EdgeKind::Exit
            } else {
                // Not a distinct entry/exit between local and Shutoko
                continue;
            }
        } else {
            continue;
        };

        let speed = match kind {
            EdgeKind::Shutoko => SHUTOKO_SPEED_KMH,
            EdgeKind::Entry | EdgeKind::Exit => RAMP_SPEED_KMH,
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
        });
    }

    // 4. Collect used nodes
    let mut active_node_ids: BTreeSet<String> = BTreeSet::new();
    for edge in &final_edges {
        active_node_ids.insert(edge.from.clone());
        active_node_ids.insert(edge.to.clone());
    }

    let mut nodes: Vec<Node> = active_node_ids
        .iter()
        .map(|id| Node { id: id.clone() })
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    // 5. Parse turn restrictions from relations
    let mut forbidden_transitions_set: BTreeSet<Vec<String>> = BTreeSet::new();

    // Map for fast lookup of edges by (way_id, from, to)
    for elem in &response.elements {
        if !elem.is_relation() {
            continue;
        }
        if elem.get_tag("type") != Some("restriction") {
            continue;
        }

        let restriction = match elem.get_tag("restriction") {
            Some(r) => r,
            None => continue,
        };

        if !matches!(
            restriction,
            "no_left_turn" | "no_right_turn" | "no_u_turn" | "no_straight_on"
        ) {
            continue;
        }

        let members = match &elem.members {
            Some(m) => m,
            None => continue,
        };

        let mut from_way: Option<i64> = None;
        let mut to_way: Option<i64> = None;
        let mut via_node: Option<i64> = None;

        for m in members {
            if m.role == "from" && m.member_type == "way" {
                from_way = Some(m.ref_id);
            } else if m.role == "to" && m.member_type == "way" {
                to_way = Some(m.ref_id);
            } else if m.role == "via" && m.member_type == "node" {
                via_node = Some(m.ref_id);
            }
        }

        if let (Some(fw), Some(tw), Some(vn)) = (from_way, to_way, via_node) {
            let via_node_str = format!("n:{}", vn);

            // Find an edge in fw ending at vn, and an edge in tw starting at vn
            let fw_edges = match way_edges_by_id.get(&fw) {
                Some(es) => es,
                None => continue,
            };
            let tw_edges = match way_edges_by_id.get(&tw) {
                Some(es) => es,
                None => continue,
            };

            for fe_id in fw_edges {
                if !final_edge_ids.contains(fe_id) {
                    continue;
                }
                let fe = final_edges.iter().find(|e| e.id == *fe_id).unwrap();
                if fe.to != via_node_str {
                    continue;
                }

                for te_id in tw_edges {
                    if !final_edge_ids.contains(te_id) {
                        continue;
                    }
                    let te = final_edges.iter().find(|e| e.id == *te_id).unwrap();
                    if te.from != via_node_str {
                        continue;
                    }

                    forbidden_transitions_set.insert(vec![fe_id.clone(), te_id.clone()]);
                }
            }
        }
    }

    let mut forbidden_transitions: Vec<Vec<String>> =
        forbidden_transitions_set.into_iter().collect();
    forbidden_transitions.sort();

    // Sort edges deterministically by ID
    final_edges.sort_by(|a, b| a.id.cmp(&b.id));

    let graph = Graph {
        schema_version: 1,
        release_id: config.release_id.clone(),
        vehicle_profile: config.vehicle_profile.clone(),
        nodes,
        edges: final_edges,
        billing_pairs: Vec::new(),
        forbidden_transitions,
    };

    // 6. Build SnapIndex for local road nodes
    let mut snap_nodes: Vec<SnapNode> = Vec::new();
    let local_node_ids: HashSet<&str> = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Local)
        .flat_map(|e| vec![e.from.as_str(), e.to.as_str()])
        .collect();

    for node in &graph.nodes {
        if local_node_ids.contains(node.id.as_str()) {
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
        schema_version: 1,
        release_id: config.release_id.clone(),
        nodes: snap_nodes,
    };

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
}
