//! Google Maps URLs generation and waypoint selection for navigation handoff.
//!
//! URL generation follows standard Google Maps URLs format (<= 2,048 chars) without
//! pulling external heavy URL crates. Waypoint selection uses a provisional 3-point rule
//! (entry access point, loop midpoint, exit node) subject to physical verification in #8.

use crate::{Edge, LatLng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_MAPS_URL_LENGTH: usize = 2048;
pub const MAX_MAPS_WAYPOINTS: usize = 3;
pub const URL_BUILDER_VERSION: &str = "google-maps-split/v1";
pub const DEVICE_VERIFICATION_PENDING: &str = "device_verification_pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapsHandoffLegRole {
    SurfaceAccess,
    LoopTransfer,
    SurfaceReturn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapsHandoffLeg {
    pub role: MapsHandoffLegRole,
    pub origin: LatLng,
    pub destination: LatLng,
    pub waypoints: Vec<LatLng>,
    pub maps_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapsHandoffLegWire {
    pub role: MapsHandoffLegRole,
    pub maps_url: String,
    pub url_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SplitMapsHandoff {
    pub builder_version: String,
    pub legs: Vec<MapsHandoffLeg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapsHandoffError {
    EmptyMandatoryLap,
    MissingNode,
    InvalidCoordinate,
    TooManyWaypoints,
    UrlTooLong,
    InvalidBuilderVersion,
    InvalidLegCount,
    InvalidLegOrder,
    MapsUrlMismatch,
    DeviceVerificationBindingMismatch,
}

pub fn maps_url_sha256(maps_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(maps_url.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn valid_lat_lng(point: &LatLng) -> bool {
    point.lat.is_finite()
        && point.lon.is_finite()
        && (-90.0..=90.0).contains(&point.lat)
        && (-180.0..=180.0).contains(&point.lon)
}

fn append_waypoints(url: &mut String, waypoints: &[LatLng]) {
    if waypoints.is_empty() {
        return;
    }
    url.push_str("&waypoints=");
    for (index, waypoint) in waypoints.iter().enumerate() {
        if index > 0 {
            url.push_str("%7C");
        }
        url.push_str(&format!("{:.6},{:.6}", waypoint.lat, waypoint.lon));
    }
}

pub fn format_maps_leg_url(
    origin: &LatLng,
    destination: &LatLng,
    waypoints: &[LatLng],
) -> Result<String, MapsHandoffError> {
    if waypoints.len() > MAX_MAPS_WAYPOINTS {
        return Err(MapsHandoffError::TooManyWaypoints);
    }
    if !valid_lat_lng(origin)
        || !valid_lat_lng(destination)
        || waypoints.iter().any(|waypoint| !valid_lat_lng(waypoint))
    {
        return Err(MapsHandoffError::InvalidCoordinate);
    }
    let mut url = format!(
        "https://www.google.com/maps/dir/?api=1&origin={:.6},{:.6}&destination={:.6},{:.6}&travelmode=driving",
        origin.lat, origin.lon, destination.lat, destination.lon
    );
    append_waypoints(&mut url, waypoints);
    if url.len() > MAX_MAPS_URL_LENGTH {
        return Err(MapsHandoffError::UrlTooLong);
    }
    Ok(url)
}

fn midpoint_node_id<'a>(mandatory_lap_edges: &[&'a Edge]) -> Result<&'a str, MapsHandoffError> {
    let first = mandatory_lap_edges
        .first()
        .ok_or(MapsHandoffError::EmptyMandatoryLap)?;
    let total_distance = mandatory_lap_edges.iter().fold(0_u128, |total, edge| {
        total + u128::from(edge.distance_meters)
    });
    let target_distance = total_distance / 2;
    let mut accumulated_distance = 0_u128;
    let mut best_node_id = first.to.as_str();
    let mut best_distance = u128::MAX;
    for edge in mandatory_lap_edges {
        accumulated_distance += u128::from(edge.distance_meters);
        let distance = accumulated_distance.abs_diff(target_distance);
        if distance < best_distance {
            best_distance = distance;
            best_node_id = edge.to.as_str();
        }
    }
    Ok(best_node_id)
}

pub fn build_split_maps_handoff<F>(
    origin: &LatLng,
    entry_access_node_id: &str,
    merge_node_id: &str,
    mandatory_lap_edges: &[&Edge],
    branch_node_id: &str,
    exit_access_node_id: &str,
    node_latlng: F,
) -> Result<SplitMapsHandoff, MapsHandoffError>
where
    F: Fn(&str) -> Option<LatLng>,
{
    let midpoint_node_id = midpoint_node_id(mandatory_lap_edges)?;
    let entry_access = node_latlng(entry_access_node_id).ok_or(MapsHandoffError::MissingNode)?;
    let merge = node_latlng(merge_node_id).ok_or(MapsHandoffError::MissingNode)?;
    let midpoint = node_latlng(midpoint_node_id).ok_or(MapsHandoffError::MissingNode)?;
    let branch = node_latlng(branch_node_id).ok_or(MapsHandoffError::MissingNode)?;
    let exit_access = node_latlng(exit_access_node_id).ok_or(MapsHandoffError::MissingNode)?;
    let loop_waypoints = vec![merge, midpoint];
    let entry_access_origin = entry_access.clone();
    let branch_origin = branch.clone();
    let surface_access_url = format_maps_leg_url(origin, &entry_access, &[])?;
    let loop_transfer_url = format_maps_leg_url(&entry_access, &branch, &loop_waypoints)?;
    let surface_return_waypoints = vec![exit_access];
    let surface_return_url = format_maps_leg_url(&branch, origin, &surface_return_waypoints)?;
    let legs = vec![
        MapsHandoffLeg {
            role: MapsHandoffLegRole::SurfaceAccess,
            origin: origin.clone(),
            destination: entry_access,
            waypoints: Vec::new(),
            maps_url: surface_access_url,
        },
        MapsHandoffLeg {
            role: MapsHandoffLegRole::LoopTransfer,
            origin: entry_access_origin,
            destination: branch,
            waypoints: loop_waypoints,
            maps_url: loop_transfer_url,
        },
        MapsHandoffLeg {
            role: MapsHandoffLegRole::SurfaceReturn,
            origin: branch_origin,
            destination: origin.clone(),
            waypoints: surface_return_waypoints,
            maps_url: surface_return_url,
        },
    ];
    let handoff = SplitMapsHandoff {
        builder_version: URL_BUILDER_VERSION.to_owned(),
        legs,
    };
    handoff.validate()?;
    Ok(handoff)
}

impl SplitMapsHandoff {
    pub fn validate(&self) -> Result<(), MapsHandoffError> {
        if self.builder_version != URL_BUILDER_VERSION {
            return Err(MapsHandoffError::InvalidBuilderVersion);
        }
        let expected_roles = [
            MapsHandoffLegRole::SurfaceAccess,
            MapsHandoffLegRole::LoopTransfer,
            MapsHandoffLegRole::SurfaceReturn,
        ];
        if self.legs.len() != expected_roles.len() {
            return Err(MapsHandoffError::InvalidLegCount);
        }
        for (leg, expected_role) in self.legs.iter().zip(expected_roles) {
            if leg.role != expected_role {
                return Err(MapsHandoffError::InvalidLegOrder);
            }
            let expected_url = format_maps_leg_url(&leg.origin, &leg.destination, &leg.waypoints)?;
            if leg.maps_url != expected_url {
                return Err(MapsHandoffError::MapsUrlMismatch);
            }
        }
        Ok(())
    }

    pub fn wire_legs(&self) -> Result<Vec<MapsHandoffLegWire>, MapsHandoffError> {
        self.validate()?;
        Ok(self
            .legs
            .iter()
            .map(|leg| MapsHandoffLegWire {
                role: leg.role,
                maps_url: leg.maps_url.clone(),
                url_sha256: maps_url_sha256(&leg.maps_url),
            })
            .collect())
    }
}

/// Select provisional waypoints for Google Maps handoff to prevent short-circuiting.
///
/// Provisional rules:
/// 1. Entry access point: the `from`-node of the Entry edge (where the user enters
///    the expressway network from the surface streets).
/// 2. Midpoint node along the loop (node closest to 50% accumulated loop distance).
/// 3. Exit node: the `to`-node of the Exit edge (where the user leaves the expressway).
///
/// The current-location-to-entry-access-point leg is handled by Google Maps itself;
/// we do not add the user's origin as a waypoint.
///
/// `node_latlng` is called to resolve a node ID to its coordinates.  Returns
/// `None` for unknown IDs (which are then silently dropped from the result).
/// Passing a closure avoids building a temporary `BTreeMap` on every call when
/// the caller already owns an indexed graph structure.
///
/// Returns at most 3 distinct waypoints.
pub fn select_waypoints<F>(
    entry_access_node_id: &str,
    cycle_edges: &[&Edge],
    exit_edge: &Edge,
    node_latlng: F,
) -> Vec<LatLng>
where
    F: Fn(&str) -> Option<LatLng>,
{
    let mut selected_node_ids: Vec<&str> = Vec::with_capacity(3);

    // 1. Entry access point: Entry edge from-node (start of expressway section).
    selected_node_ids.push(entry_access_node_id);

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

    // 3. Exit node: Exit edge to-node (end of expressway section).
    let exit_node_id = exit_edge.to.as_str();
    if !selected_node_ids.contains(&exit_node_id) {
        selected_node_ids.push(exit_node_id);
    }

    selected_node_ids.truncate(3);

    selected_node_ids
        .into_iter()
        .filter_map(node_latlng)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EdgeKind;

    fn edge(from: &str, to: &str, distance_meters: u64) -> Edge {
        Edge {
            id: format!("edge:{from}:{to}"),
            from: from.to_owned(),
            to: to.to_owned(),
            kind: EdgeKind::Shutoko,
            duration_seconds: 60,
            distance_meters,
            name: None,
        }
    }

    fn split_fixture() -> SplitMapsHandoff {
        let merge = edge("m", "mid", 10_000);
        let rest = edge("mid", "b", 10_000);
        build_split_maps_handoff(
            &LatLng {
                lat: 35.0,
                lon: 139.0,
            },
            "entry",
            "m",
            &[&merge, &rest],
            "b",
            "exit",
            |node_id| {
                let (lat, lon) = match node_id {
                    "entry" => (35.1, 139.1),
                    "m" => (35.2, 139.2),
                    "mid" => (35.3, 139.3),
                    "b" => (35.4, 139.4),
                    "exit" => (35.5, 139.5),
                    _ => return None,
                };
                Some(LatLng { lat, lon })
            },
        )
        .unwrap()
    }

    #[test]
    fn builds_three_ordered_bounded_google_maps_leg_urls() {
        let handoff = split_fixture();
        assert_eq!(handoff.builder_version, URL_BUILDER_VERSION);
        assert_eq!(
            handoff.legs.iter().map(|leg| leg.role).collect::<Vec<_>>(),
            vec![
                MapsHandoffLegRole::SurfaceAccess,
                MapsHandoffLegRole::LoopTransfer,
                MapsHandoffLegRole::SurfaceReturn,
            ]
        );
        assert_eq!(
            handoff.legs[0].origin,
            LatLng {
                lat: 35.0,
                lon: 139.0
            }
        );
        assert_eq!(
            handoff.legs[0].destination,
            LatLng {
                lat: 35.1,
                lon: 139.1
            }
        );
        assert_eq!(
            handoff.legs[1].origin,
            LatLng {
                lat: 35.1,
                lon: 139.1
            }
        );
        assert_eq!(
            handoff.legs[1].destination,
            LatLng {
                lat: 35.4,
                lon: 139.4
            }
        );
        assert_eq!(
            handoff.legs[1].waypoints,
            vec![
                LatLng {
                    lat: 35.2,
                    lon: 139.2
                },
                LatLng {
                    lat: 35.3,
                    lon: 139.3
                }
            ]
        );
        assert_eq!(
            handoff.legs[2].origin,
            LatLng {
                lat: 35.4,
                lon: 139.4
            }
        );
        assert_eq!(
            handoff.legs[2].destination,
            LatLng {
                lat: 35.0,
                lon: 139.0
            }
        );
        assert_eq!(
            handoff.legs[2].waypoints,
            vec![LatLng {
                lat: 35.5,
                lon: 139.5
            }]
        );
        assert_eq!(
            handoff.legs[0].maps_url,
            "https://www.google.com/maps/dir/?api=1&origin=35.000000,139.000000&destination=35.100000,139.100000&travelmode=driving"
        );
        assert_eq!(
            handoff.legs[1].maps_url,
            "https://www.google.com/maps/dir/?api=1&origin=35.100000,139.100000&destination=35.400000,139.400000&travelmode=driving&waypoints=35.200000,139.200000%7C35.300000,139.300000"
        );
        assert_eq!(
            handoff.legs[2].maps_url,
            "https://www.google.com/maps/dir/?api=1&origin=35.400000,139.400000&destination=35.000000,139.000000&travelmode=driving&waypoints=35.500000,139.500000"
        );
        for leg in &handoff.legs {
            assert!(leg.waypoints.len() <= MAX_MAPS_WAYPOINTS);
            assert!(leg.maps_url.len() <= MAX_MAPS_URL_LENGTH);
            assert!(!leg.maps_url.contains("nav=1"));
            assert!(!leg.maps_url.contains("launch=navigate"));
            assert!(!leg.maps_url.contains("dir_action=n"));
        }
    }

    #[test]
    fn converts_built_legs_to_hashed_gate_wire() {
        let wire = split_fixture().wire_legs().unwrap();
        assert_eq!(wire.len(), 3);
        assert_eq!(wire[0].role, MapsHandoffLegRole::SurfaceAccess);
        assert_eq!(wire[0].url_sha256.len(), 64);
        assert_eq!(wire[0].url_sha256, maps_url_sha256(&wire[0].maps_url));
        let value = serde_json::to_value(&wire).unwrap();
        assert_eq!(value[0]["role"], "surface_access");
        assert_eq!(value[0]["mapsUrl"], wire[0].maps_url);
        assert_eq!(value[0]["urlSha256"], wire[0].url_sha256);

        let mut tampered = split_fixture();
        tampered.legs[0].maps_url.push('0');
        assert_eq!(tampered.wire_legs(), Err(MapsHandoffError::MapsUrlMismatch));
    }

    #[test]
    fn rejects_too_many_waypoints_and_invalid_coordinates() {
        let point = LatLng {
            lat: 35.0,
            lon: 139.0,
        };
        let maximum_waypoints = vec![point.clone(); MAX_MAPS_WAYPOINTS];
        let url = format_maps_leg_url(&point, &point, &maximum_waypoints).unwrap();
        assert!(url.len() <= MAX_MAPS_URL_LENGTH);
        assert_eq!(url.matches("%7C").count(), MAX_MAPS_WAYPOINTS - 1);
        assert_eq!(
            format_maps_leg_url(&point, &point, &vec![point.clone(); MAX_MAPS_WAYPOINTS + 1],),
            Err(MapsHandoffError::TooManyWaypoints)
        );
        let invalid = LatLng {
            lat: 91.0,
            lon: 139.0,
        };
        assert_eq!(
            format_maps_leg_url(&point, &invalid, &[]),
            Err(MapsHandoffError::InvalidCoordinate)
        );
    }

    #[test]
    fn rejects_empty_lap_and_missing_node() {
        let point = LatLng {
            lat: 35.0,
            lon: 139.0,
        };
        assert_eq!(
            build_split_maps_handoff(&point, "entry", "m", &[], "b", "exit", |_| None),
            Err(MapsHandoffError::EmptyMandatoryLap)
        );
        let lap = edge("m", "b", 1_000);
        assert_eq!(
            build_split_maps_handoff(&point, "entry", "m", &[&lap], "b", "exit", |_| None),
            Err(MapsHandoffError::MissingNode)
        );
    }
}
