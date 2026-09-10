//! Google Maps URLs generation and waypoint selection for navigation handoff.
//!
//! URL generation follows standard Google Maps URLs format (<= 2,048 chars) without
//! pulling external heavy URL crates. Waypoint selection uses a provisional 3-point rule
//! (entry merge anchor, loop midpoint, exit branch) subject to physical verification in #8.

use crate::{Edge, LatLng, Node};
use std::collections::BTreeMap;

/// Maximum allowed character length for generated Google Maps URLs.
pub const MAX_MAPS_URL_LENGTH: usize = 2048;

/// Select provisional waypoints for Google Maps handoff to prevent short-circuiting.
///
/// Provisional rules:
/// 1. Mainline node immediately after entry merge (anchor node).
/// 2. Midpoint node along the loop (node closest to 50% accumulated loop distance).
/// 3. Mainline node immediately before exit divergence (from-node of exit edge).
///
/// Returns at most 3 distinct waypoints.
pub fn select_waypoints(
    anchor_node_id: &str,
    cycle_edges: &[&Edge],
    exit_edge: &Edge,
    nodes_by_id: &BTreeMap<&str, &Node>,
) -> Vec<LatLng> {
    let mut selected_node_ids: Vec<&str> = Vec::with_capacity(3);

    // 1. Entry merge mainline anchor node
    selected_node_ids.push(anchor_node_id);

    // 2. Loop distance midpoint node
    if !cycle_edges.is_empty() {
        let total_loop_dist: u64 = cycle_edges.iter().map(|e| e.distance_meters).sum();
        let target_dist = total_loop_dist as f64 * 0.5;

        let mut accumulated_dist: u64 = 0;
        let mut best_midpoint_node_id: Option<&str> = None;
        let mut min_diff = f64::MAX;

        for edge in cycle_edges {
            accumulated_dist += edge.distance_meters;
            let diff = (accumulated_dist as f64 - target_dist).abs();
            if diff < min_diff {
                min_diff = diff;
                best_midpoint_node_id = Some(edge.to.as_str());
            }
        }

        if let Some(mid_id) = best_midpoint_node_id {
            if !selected_node_ids.contains(&mid_id) {
                selected_node_ids.push(mid_id);
            }
        }
    }

    // 3. Mainline node immediately before exit branch
    let exit_branch_node_id = exit_edge.from.as_str();
    if !selected_node_ids.contains(&exit_branch_node_id) {
        selected_node_ids.push(exit_branch_node_id);
    }

    selected_node_ids.truncate(3);

    selected_node_ids
        .into_iter()
        .filter_map(|id| {
            nodes_by_id.get(id).map(|n| LatLng {
                lat: n.lat,
                lon: n.lon,
            })
        })
        .collect()
}

/// Format a Google Maps direction URL for round-trip driving navigation with waypoints.
///
/// Format:
/// `https://www.google.com/maps/dir/?api=1&origin=<lat>,<lon>&destination=<lat>,<lon>&travelmode=driving&waypoints=<lat>,<lon>%7C...`
///
/// Coordinates are formatted with 6 decimal places.
/// Returns Err(()) if formatted URL exceeds MAX_MAPS_URL_LENGTH (2,048 chars).
#[allow(clippy::result_unit_err)]
pub fn format_maps_url(origin: &LatLng, waypoints: &[LatLng]) -> Result<String, ()> {
    let mut url = format!(
        "https://www.google.com/maps/dir/?api=1&origin={:.6},{:.6}&destination={:.6},{:.6}&travelmode=driving",
        origin.lat, origin.lon, origin.lat, origin.lon
    );

    if !waypoints.is_empty() {
        url.push_str("&waypoints=");
        for (i, wp) in waypoints.iter().enumerate() {
            if i > 0 {
                url.push_str("%7C");
            }
            url.push_str(&format!("{:.6},{:.6}", wp.lat, wp.lon));
        }
    }

    if url.len() > MAX_MAPS_URL_LENGTH {
        return Err(());
    }

    Ok(url)
}
