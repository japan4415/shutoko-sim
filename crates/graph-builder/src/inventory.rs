//! Canonical ramp inventory, OSM bindings, and OD tariff integration.
//!
//! Provides validation and binding of:
//! - `data/ramp-inventory.json` (canonical population of all Shutoko ramps)
//! - `data/osm-ramp-bindings.json` (explicit OSM way/node bindings)
//! - `data/od-tariffs.json` (official ETC OD tariffs and distance rules)

use crate::model::{EdgeKind, Graph, OdTariff, Ramp, RampKind};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// An item in the canonical ramp inventory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalRampInventoryItem {
    pub ramp_id: String,
    pub facility_id: String,
    pub facility_name: String,
    pub route: String,
    pub direction: String,
    pub kind: RampKind,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub restrictions: Vec<String>,
    pub status: String,
    pub source: String,
    pub source_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restriction_status: Option<String>,
}

/// The root structure of `data/ramp-inventory.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RampInventoryFile {
    pub version: u32,
    pub source: String,
    pub source_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinate_source: Option<String>,
    pub description: String,
    pub ramps: Vec<CanonicalRampInventoryItem>,
}

/// An OSM ramp binding entry in `data/osm-ramp-bindings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsmRampBinding {
    pub ramp_id: String,
    pub osm_way_id: i64,
    pub osm_node_id: i64,
    pub motorway_node_id: i64,
    pub direction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// The root structure of `data/osm-ramp-bindings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsmRampBindingsFile {
    pub version: u32,
    pub source_date: String,
    pub bindings: Vec<OsmRampBinding>,
}

fn default_fixed_fee() -> u64 {
    150
}

fn default_tax_rate() -> f64 {
    1.10
}

/// Distance-based toll calculation rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffRules {
    pub vehicle_profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_date: Option<String>,
    #[serde(default = "default_fixed_fee")]
    pub fixed_fee_yen: u64,
    #[serde(default = "default_tax_rate")]
    pub tax_rate: f64,
    pub min_toll_yen: u64,
    pub max_toll_yen: u64,
    pub min_distance_meters: u64,
    pub base_rate_per_km_yen: f64,
    pub rounding_yen: u64,
}

/// The root structure of `data/od-tariffs.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OdTariffsFile {
    pub version: u32,
    pub source: String,
    pub source_date: String,
    pub rules: TariffRules,
    pub verified_od_pairs: Vec<OdTariff>,
}

/// An entry in the `ramps.json` release artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RampArtifactEntry {
    pub id: String,
    pub facility_id: String,
    pub name: String,
    pub route: String,
    pub direction: String,
    pub kind: RampKind,
    pub lat: f64,
    pub lon: f64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restrictions: Vec<String>,
    pub bound: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mainline_node_id: Option<String>,
}

/// Release artifact `ramps.json` published alongside `graph.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RampsArtifact {
    pub schema_version: u32,
    pub release_id: String,
    pub source_date: String,
    pub total_ramps: usize,
    pub bound_ramps: usize,
    pub ramps: Vec<RampArtifactEntry>,
}

/// Validates the canonical ramp inventory for uniqueness, structural integrity,
/// and valid coordinates.
pub fn validate_ramp_inventory(inv: &RampInventoryFile) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut seen_ramp_ids = HashSet::new();
    let mut seen_facility_route_dir_kind = HashSet::new();

    if inv.version == 0 {
        errors.push("ramp inventory version must be >= 1".into());
    }
    if inv.ramps.is_empty() {
        errors.push("ramp inventory cannot be empty".into());
    }

    for (i, r) in inv.ramps.iter().enumerate() {
        if r.ramp_id.is_empty() {
            errors.push(format!("ramp[{}] has empty ramp_id", i));
        } else if !seen_ramp_ids.insert(r.ramp_id.clone()) {
            errors.push(format!("duplicate ramp_id: {}", r.ramp_id));
        }

        let key = (
            r.facility_id.clone(),
            r.route.clone(),
            r.direction.clone(),
            r.kind,
        );
        if !seen_facility_route_dir_kind.insert(key) {
            errors.push(format!(
                "duplicate facility/route/direction/kind: ({}, {}, {}, {:?}) for ramp {}",
                r.facility_id, r.route, r.direction, r.kind, r.ramp_id
            ));
        }

        if r.facility_name.is_empty() {
            errors.push(format!("ramp {} has empty facility_name", r.ramp_id));
        }
        if r.route.is_empty() {
            errors.push(format!("ramp {} has empty route", r.ramp_id));
        }
        if r.direction.is_empty() {
            errors.push(format!("ramp {} has empty direction", r.ramp_id));
        }
        if r.source.is_empty() {
            errors.push(format!("ramp {} has empty source", r.ramp_id));
        }
        if r.source_date.is_empty() {
            errors.push(format!("ramp {} has empty source_date", r.ramp_id));
        }
        if let Some(ref cs) = r.coordinate_status {
            if !matches!(cs.as_str(), "derived" | "verified" | "unknown") {
                errors.push(format!(
                    "ramp {} has unrecognized coordinate_status '{}'",
                    r.ramp_id, cs
                ));
            }
        }
        if let Some(ref rs) = r.restriction_status {
            if !matches!(rs.as_str(), "verified" | "unverified" | "unknown") {
                errors.push(format!(
                    "ramp {} has unrecognized restriction_status '{}'",
                    r.ramp_id, rs
                ));
            }
        }

        // Tokyo/Kanagawa/Saitama coordinate bounds roughly 35.0..=36.2 lat, 139.0..=140.5 lon
        if !(34.5..=36.5).contains(&r.lat) || !(139.0..=140.5).contains(&r.lon) {
            errors.push(format!(
                "ramp {} coordinates ({}, {}) outside Kanto region bounds",
                r.ramp_id, r.lat, r.lon
            ));
        }

        if !matches!(r.status.as_str(), "active" | "closed" | "planned") {
            errors.push(format!(
                "ramp {} has unrecognized status '{}'",
                r.ramp_id, r.status
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates OSM bindings against the canonical inventory.
pub fn validate_osm_ramp_bindings(
    bindings: &OsmRampBindingsFile,
    inv: &RampInventoryFile,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let inventory_by_ramp_id: HashMap<&str, &CanonicalRampInventoryItem> =
        inv.ramps.iter().map(|r| (r.ramp_id.as_str(), r)).collect();
    let mut bound_ramp_ids = HashSet::new();

    for (i, b) in bindings.bindings.iter().enumerate() {
        match inventory_by_ramp_id.get(b.ramp_id.as_str()) {
            None => errors.push(format!(
                "binding[{}] references unknown ramp_id '{}'",
                i, b.ramp_id
            )),
            Some(ramp) if ramp.direction != b.direction => errors.push(format!(
                "binding for '{}' has direction '{}' but inventory requires '{}'",
                b.ramp_id, b.direction, ramp.direction
            )),
            Some(_) => {}
        }
        if b.osm_way_id <= 0 {
            errors.push(format!(
                "binding for '{}' has invalid osm_way_id {}",
                b.ramp_id, b.osm_way_id
            ));
        }
        if b.osm_node_id <= 0 {
            errors.push(format!(
                "binding for '{}' has invalid osm_node_id {}",
                b.ramp_id, b.osm_node_id
            ));
        }
        if b.motorway_node_id <= 0 {
            errors.push(format!(
                "binding for '{}' has invalid motorway_node_id {}",
                b.ramp_id, b.motorway_node_id
            ));
        }
        // Forbid consecutive placeholder IDs (e.g. osmNodeId = osmWayId + 1)
        if b.osm_node_id == b.osm_way_id + 1 || b.motorway_node_id == b.osm_way_id + 2 {
            errors.push(format!(
                "binding for '{}' uses forbidden consecutive placeholder IDs (way={}, node={}, motorway={})",
                b.ramp_id, b.osm_way_id, b.osm_node_id, b.motorway_node_id
            ));
        }

        if !bound_ramp_ids.insert(b.ramp_id.as_str()) {
            errors.push(format!(
                "duplicate binding for ramp '{}'; expected one",
                b.ramp_id
            ));
        }
    }

    // Only active general ramps are required to be user-selectable. Boundary
    // connectors and closed/planned ramps remain distinct inventory records,
    // but must not be forced into the public entry/exit selection set.
    for r in &inv.ramps {
        if r.status == "active"
            && matches!(r.kind, RampKind::GeneralEntry | RampKind::GeneralExit)
            && !bound_ramp_ids.contains(r.ramp_id.as_str())
        {
            errors.push(format!(
                "active general ramp '{}' has no OSM binding in bindings file",
                r.ramp_id
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates that every OSM binding references real elements within the Overpass response:
/// 1. `osm_way_id` exists in the OSM ways.
/// 2. `osm_node_id` exists and is a member of `osm_way_id.nodes`.
/// 3. `motorway_node_id` exists in the OSM nodes.
pub fn validate_osm_ramp_bindings_against_osm(
    bindings: &OsmRampBindingsFile,
    osm_resp: &crate::osm::OverpassResponse,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut way_map: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut node_set: HashSet<i64> = HashSet::new();

    for elem in &osm_resp.elements {
        if elem.is_way() {
            if let Some(nodes) = elem.nodes.as_ref() {
                way_map.insert(elem.id, nodes.clone());
            }
        } else if elem.is_node() {
            node_set.insert(elem.id);
        }
    }

    for b in &bindings.bindings {
        match way_map.get(&b.osm_way_id) {
            None => {
                errors.push(format!(
                    "binding for '{}' references non-existent osmWayId {}",
                    b.ramp_id, b.osm_way_id
                ));
            }
            Some(nodes) => {
                if !nodes.contains(&b.osm_node_id) {
                    errors.push(format!(
                        "binding for '{}': osmNodeId {} is not a member of osmWayId {} nodes",
                        b.ramp_id, b.osm_node_id, b.osm_way_id
                    ));
                }
            }
        }

        if !node_set.contains(&b.motorway_node_id) {
            errors.push(format!(
                "binding for '{}' references non-existent motorwayNodeId {}",
                b.ramp_id, b.motorway_node_id
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates OD tariffs against the canonical inventory.
pub fn validate_od_tariffs(
    tariffs: &OdTariffsFile,
    inv: &RampInventoryFile,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let entry_ramp_ids: HashSet<&str> = inv
        .ramps
        .iter()
        .filter(|r| matches!(r.kind, RampKind::GeneralEntry | RampKind::BoundaryIn))
        .map(|r| r.ramp_id.as_str())
        .collect();
    let exit_ramp_ids: HashSet<&str> = inv
        .ramps
        .iter()
        .filter(|r| matches!(r.kind, RampKind::GeneralExit | RampKind::BoundaryOut))
        .map(|r| r.ramp_id.as_str())
        .collect();

    for (i, pair) in tariffs.verified_od_pairs.iter().enumerate() {
        if !entry_ramp_ids.contains(pair.entry_ramp_id.as_str()) {
            errors.push(format!(
                "tariff[{}] entry_ramp_id '{}' is not a valid entry ramp in inventory",
                i, pair.entry_ramp_id
            ));
        }
        if !exit_ramp_ids.contains(pair.exit_ramp_id.as_str()) {
            errors.push(format!(
                "tariff[{}] exit_ramp_id '{}' is not a valid exit ramp in inventory",
                i, pair.exit_ramp_id
            ));
        }
        if pair.billing_distance_meters == 0 {
            errors.push(format!(
                "tariff[{}] ({} -> {}) has zero billing_distance_meters",
                i, pair.entry_ramp_id, pair.exit_ramp_id
            ));
        }
        if let Some(amt) = pair.amount_yen {
            if !(300..=1950).contains(&amt) {
                errors.push(format!(
                    "tariff[{}] ({} -> {}) amount_yen {} outside 300..=1950 range",
                    i, pair.entry_ramp_id, pair.exit_ramp_id, amt
                ));
            }
            if amt % 10 != 0 {
                errors.push(format!(
                    "tariff[{}] ({} -> {}) amount_yen {} not rounded to 10 yen",
                    i, pair.entry_ramp_id, pair.exit_ramp_id, amt
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Binds ramps to graph edges and produces:
/// 1. `Vec<Ramp>` for `graph.ramps` (only those bound to edges in the graph).
/// 2. `Vec<RampArtifactEntry>` for `ramps.json` (all canonical ramps).
/// 3. Informational messages for unbound ramps outside the graph coverage.
pub fn bind_ramps_to_graph(
    graph: &Graph,
    inv: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
) -> (Vec<Ramp>, Vec<RampArtifactEntry>, Vec<String>) {
    let mut bound_ramps = Vec::new();
    let mut artifact_entries = Vec::new();
    let mut unbound_notes = Vec::new();

    // Map ramp_id -> binding
    let binding_map: HashMap<&str, &OsmRampBinding> = bindings
        .bindings
        .iter()
        .map(|b| (b.ramp_id.as_str(), b))
        .collect();

    // Pre-index graph edges by way ID
    // Edge IDs have format "e:w{way_id}:{idx}:{dir}"
    let mut edges_by_way: HashMap<i64, Vec<&crate::model::Edge>> = HashMap::new();
    for e in &graph.edges {
        let parts: Vec<&str> = e.id.split(':').collect();
        if parts.len() >= 2 && parts[1].starts_with('w') {
            if let Ok(wid) = parts[1][1..].parse::<i64>() {
                edges_by_way.entry(wid).or_default().push(e);
            }
        }
    }

    for item in &inv.ramps {
        let binding = binding_map.get(item.ramp_id.as_str()).copied();

        let mut matched_edge: Option<&crate::model::Edge> = None;
        let is_active_general = item.status == "active"
            && matches!(item.kind, RampKind::GeneralEntry | RampKind::GeneralExit);
        if is_active_general {
            if let Some(b) = binding {
                if let Some(candidate_edges) = edges_by_way.get(&b.osm_way_id) {
                    let is_entry = item.kind == RampKind::GeneralEntry;
                    let target_kind = if is_entry {
                        EdgeKind::Entry
                    } else {
                        EdgeKind::Exit
                    };
                    let ground_node = format!("n:{}", b.osm_node_id);
                    let motorway_node = format!("n:{}", b.motorway_node_id);
                    let (expected_from, expected_to) = if is_entry {
                        (ground_node.as_str(), motorway_node.as_str())
                    } else {
                        (motorway_node.as_str(), ground_node.as_str())
                    };

                    // Bind the exact directed OSM segment named by the binding.
                    // Selecting the first edge of a way can silently attach multiple
                    // facilities to the wrong end of a multi-segment ramp.
                    matched_edge = candidate_edges
                        .iter()
                        .find(|e| {
                            e.kind == target_kind && e.from == expected_from && e.to == expected_to
                        })
                        .copied();
                }
            }
        }

        if let (Some(b), Some(edge)) = (binding, matched_edge) {
            let is_entry = item.kind == RampKind::GeneralEntry;
            let node_id = if is_entry {
                edge.from.clone()
            } else {
                edge.to.clone()
            };
            let motorway_node_str = format!("n:{}", b.motorway_node_id);
            let mainline_node_id = if graph.nodes.iter().any(|n| n.id == motorway_node_str) {
                motorway_node_str
            } else if is_entry {
                edge.to.clone()
            } else {
                edge.from.clone()
            };

            let ramp = Ramp {
                id: item.ramp_id.clone(),
                facility_id: item.facility_id.clone(),
                name: item.facility_name.clone(),
                route: item.route.clone(),
                direction: item.direction.clone(),
                kind: item.kind,
                edge_id: edge.id.clone(),
                node_id: node_id.clone(),
                mainline_node_id: mainline_node_id.clone(),
                restrictions: item.restrictions.clone(),
            };
            bound_ramps.push(ramp);

            artifact_entries.push(RampArtifactEntry {
                id: item.ramp_id.clone(),
                facility_id: item.facility_id.clone(),
                name: item.facility_name.clone(),
                route: item.route.clone(),
                direction: item.direction.clone(),
                kind: item.kind,
                lat: item.lat,
                lon: item.lon,
                status: item.status.clone(),
                restrictions: item.restrictions.clone(),
                bound: true,
                edge_id: Some(edge.id.clone()),
                node_id: Some(node_id),
                mainline_node_id: Some(mainline_node_id),
            });
        } else {
            unbound_notes.push(format!(
                "unbound-ramp:{}:{} ({}) outside graph extract",
                item.route, item.ramp_id, item.facility_name
            ));
            artifact_entries.push(RampArtifactEntry {
                id: item.ramp_id.clone(),
                facility_id: item.facility_id.clone(),
                name: item.facility_name.clone(),
                route: item.route.clone(),
                direction: item.direction.clone(),
                kind: item.kind,
                lat: item.lat,
                lon: item.lon,
                status: item.status.clone(),
                restrictions: item.restrictions.clone(),
                bound: false,
                edge_id: None,
                node_id: None,
                mainline_node_id: None,
            });
        }
    }

    (bound_ramps, artifact_entries, unbound_notes)
}

/// Applies OD tariffs and annotates existing billing pairs in graph.
pub fn apply_od_tariffs_to_graph(graph: &mut Graph, tariffs: &OdTariffsFile) {
    graph.od_tariffs = tariffs.verified_od_pairs.clone();

    // Map (entry_ramp_id, exit_ramp_id) -> billing_distance_meters
    let tariff_dist: HashMap<(&str, &str), u64> = tariffs
        .verified_od_pairs
        .iter()
        .map(|t| {
            (
                (t.entry_ramp_id.as_str(), t.exit_ramp_id.as_str()),
                t.billing_distance_meters,
            )
        })
        .collect();

    // Map edge_id -> ramp_id from graph.ramps
    let edge_to_ramp: HashMap<&str, &str> = graph
        .ramps
        .iter()
        .map(|r| (r.edge_id.as_str(), r.id.as_str()))
        .collect();

    for p in &mut graph.billing_pairs {
        if p.entry_ramp_id.is_none() {
            if let Some(&rid) = edge_to_ramp.get(p.entry_id.as_str()) {
                p.entry_ramp_id = Some(rid.to_string());
            }
        }
        if p.exit_ramp_id.is_none() {
            if let Some(&rid) = edge_to_ramp.get(p.exit_id.as_str()) {
                p.exit_ramp_id = Some(rid.to_string());
            }
        }
        if p.billing_distance_meters.is_none() {
            if let (Some(e_rid), Some(x_rid)) = (&p.entry_ramp_id, &p.exit_ramp_id) {
                if let Some(&dist) = tariff_dist.get(&(e_rid.as_str(), x_rid.as_str())) {
                    p.billing_distance_meters = Some(dist);
                }
            }
        }
    }
}

/// Serializes `RampsArtifact` deterministically.
pub fn ramps_artifact_to_deterministic_json(
    artifact: &RampsArtifact,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(artifact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn find_data_file(relative: &str) -> std::path::PathBuf {
        let p1 = Path::new(relative);
        if p1.exists() {
            return p1.to_path_buf();
        }
        let p2 = Path::new("../../").join(relative);
        if p2.exists() {
            return p2;
        }
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let p3 = manifest_dir.join("../../").join(relative);
        if p3.exists() {
            return p3;
        }
        panic!("data file '{}' not found in test search paths", relative);
    }

    #[test]
    fn test_validate_real_inventory_file() {
        let real_path = find_data_file("data/ramp-inventory.json");
        let content = fs::read_to_string(&real_path).expect("read ramp-inventory.json");
        let inv: RampInventoryFile = serde_json::from_str(&content).expect("parse ramp-inventory");
        let res = validate_ramp_inventory(&inv);
        assert!(res.is_ok(), "ramp inventory validation failed: {:?}", res);

        // Load official population snapshot to cross-check diff (no self-sufficient test)
        let snap_path = find_data_file("data/official-population-snapshot.json");
        let snap_content =
            fs::read_to_string(&snap_path).expect("read official-population-snapshot.json");
        let snap_val: serde_json::Value =
            serde_json::from_str(&snap_content).expect("parse snapshot");

        let snap_entries = snap_val["generalEntries"]
            .as_array()
            .expect("generalEntries array");
        let snap_exits = snap_val["generalExits"]
            .as_array()
            .expect("generalExits array");
        let snap_summary = &snap_val["summary"];

        assert_eq!(
            snap_entries.len(),
            snap_summary["totalGeneralEntries"].as_u64().unwrap() as usize
        );
        assert_eq!(
            snap_exits.len(),
            snap_summary["totalGeneralExits"].as_u64().unwrap() as usize
        );

        // Verify that every active entry and exit in official snapshot has a matching active ramp in inventory
        let active_entries: Vec<_> = inv
            .ramps
            .iter()
            .filter(|r| r.kind == RampKind::GeneralEntry && r.status == "active")
            .collect();
        let active_exits: Vec<_> = inv
            .ramps
            .iter()
            .filter(|r| r.kind == RampKind::GeneralExit && r.status == "active")
            .collect();

        assert_eq!(
            active_entries.len(),
            snap_entries.len(),
            "active general entries count must exactly match official population snapshot"
        );
        assert_eq!(
            active_exits.len(),
            snap_exits.len(),
            "active general exits count must exactly match official population snapshot"
        );

        let active_entry_routes_dirs: HashSet<_> = active_entries
            .iter()
            .map(|r| {
                (
                    r.facility_name.as_str(),
                    r.route.as_str(),
                    r.direction.as_str(),
                )
            })
            .collect();
        for se in snap_entries {
            let name = se["facilityName"].as_str().unwrap();
            let route = se["route"].as_str().unwrap();
            let dir = se["direction"].as_str().unwrap();
            assert!(
                active_entry_routes_dirs.contains(&(name, route, dir)),
                "official snapshot entry {} ({}, {}) missing in active inventory",
                name,
                route,
                dir
            );
        }

        let active_exit_routes_dirs: HashSet<_> = active_exits
            .iter()
            .map(|r| {
                (
                    r.facility_name.as_str(),
                    r.route.as_str(),
                    r.direction.as_str(),
                )
            })
            .collect();
        for se in snap_exits {
            let name = se["facilityName"].as_str().unwrap();
            let route = se["route"].as_str().unwrap();
            let dir = se["direction"].as_str().unwrap();
            assert!(
                active_exit_routes_dirs.contains(&(name, route, dir)),
                "official snapshot exit {} ({}, {}) missing in active inventory",
                name,
                route,
                dir
            );
        }

        // Verify closed historical ramps (Gofukubashi, Edobashi)
        let closed_ramps: Vec<_> = inv.ramps.iter().filter(|r| r.status == "closed").collect();
        assert_eq!(closed_ramps.len(), 4, "expected 4 closed historical ramps");

        // Verify boundary connections
        let boundary_in = inv
            .ramps
            .iter()
            .filter(|r| r.kind == RampKind::BoundaryIn)
            .count();
        let boundary_out = inv
            .ramps
            .iter()
            .filter(|r| r.kind == RampKind::BoundaryOut)
            .count();
        assert_eq!(boundary_in, 12, "expected 12 boundary in ramps");
        assert_eq!(boundary_out, 12, "expected 12 boundary out ramps");

        // Verify total canonical ramps
        assert_eq!(
            inv.ramps.len(),
            399,
            "expected 399 total canonical ramps (371 active general + 24 boundary + 4 closed)"
        );

        // Verify uniqueness of ramp_id
        let mut seen_ids = HashSet::new();
        for r in &inv.ramps {
            assert!(
                seen_ids.insert(&r.ramp_id),
                "duplicate ramp_id: {}",
                r.ramp_id
            );
        }

        // Verify provenance separation:
        // - source must be non-empty official URL
        // - coordinateSource must be OSM
        // - coordinateStatus must be derived
        for r in &inv.ramps {
            assert!(!r.source.is_empty(), "ramp {} missing source", r.ramp_id);
            assert!(
                r.source.starts_with("https://search.shutoko.jp/")
                    || r.source.starts_with("https://www.shutoko.jp/"),
                "ramp {} source must point to official Shutoko domain",
                r.ramp_id
            );
            assert!(
                r.coordinate_source.as_deref().unwrap_or("").contains("osm")
                    || r.coordinate_source
                        .as_deref()
                        .unwrap_or("")
                        .contains("openstreetmap"),
                "ramp {} coordinateSource should reference OSM",
                r.ramp_id
            );
            assert_eq!(
                r.coordinate_status.as_deref(),
                Some("derived"),
                "ramp {} coordinateStatus should be 'derived'",
                r.ramp_id
            );
        }
    }

    #[test]
    fn test_validate_real_bindings_file() {
        let inv_path = find_data_file("data/ramp-inventory.json");
        let bin_path = find_data_file("data/osm-ramp-bindings.json");
        let inv_str = fs::read_to_string(inv_path).unwrap();
        let bin_str = fs::read_to_string(bin_path).unwrap();
        let inv: RampInventoryFile = serde_json::from_str(&inv_str).unwrap();
        let bindings: OsmRampBindingsFile = serde_json::from_str(&bin_str).unwrap();

        let res = validate_osm_ramp_bindings(&bindings, &inv);
        assert!(res.is_ok(), "bindings validation failed: {:?}", res);
        assert_eq!(
            bindings.bindings.len(),
            371,
            "only active general ramps should have OSM bindings"
        );

        // Validate that no placeholder IDs exist (e.g. osmNodeId == osmWayId + 1)
        for b in &bindings.bindings {
            assert_ne!(
                b.osm_node_id,
                b.osm_way_id + 1,
                "placeholder ID detected for ramp {}: osmNodeId {} == osmWayId {} + 1",
                b.ramp_id,
                b.osm_node_id,
                b.osm_way_id
            );
        }

        // Validate against real full-network OSM fixture
        let osm_path = find_data_file("fixtures/osm/shutoko-all.json");
        let osm_str = fs::read_to_string(osm_path).expect("read shutoko-all.json");
        let osm_resp: crate::osm::OverpassResponse =
            serde_json::from_str(&osm_str).expect("parse shutoko-all.json");
        let osm_res = validate_osm_ramp_bindings_against_osm(&bindings, &osm_resp);
        assert!(
            osm_res.is_ok(),
            "bindings against OSM fixture validation failed: {:?}",
            osm_res
        );
    }

    #[test]
    fn test_validate_real_tariffs_file() {
        let inv_path = find_data_file("data/ramp-inventory.json");
        let tar_path = find_data_file("data/od-tariffs.json");
        let inv_str = fs::read_to_string(inv_path).unwrap();
        let tar_str = fs::read_to_string(tar_path).unwrap();
        let inv: RampInventoryFile = serde_json::from_str(&inv_str).unwrap();
        let tariffs: OdTariffsFile = serde_json::from_str(&tar_str).unwrap();

        let res = validate_od_tariffs(&tariffs, &inv);
        assert!(res.is_ok(), "tariffs validation failed: {:?}", res);
        assert!(!tariffs.verified_od_pairs.is_empty());
    }

    #[test]
    fn test_reject_duplicate_ramp_id() {
        let inv = RampInventoryFile {
            version: 1,
            source: "test".into(),
            source_date: "2026-09-16".into(),
            coordinate_source: Some("osm".into()),
            description: "test".into(),
            ramps: vec![
                CanonicalRampInventoryItem {
                    ramp_id: "ramp:test:1".into(),
                    facility_id: "fac:test:1".into(),
                    facility_name: "Test 1".into(),
                    route: "C1".into(),
                    direction: "inner".into(),
                    kind: RampKind::GeneralEntry,
                    lat: 35.68,
                    lon: 139.76,
                    restrictions: vec![],
                    status: "active".into(),
                    source: "test".into(),
                    source_date: "2026-09-16".into(),
                    coordinate_source: Some("osm".into()),
                    coordinate_status: Some("derived".into()),
                    restriction_status: Some("unverified".into()),
                },
                CanonicalRampInventoryItem {
                    ramp_id: "ramp:test:1".into(),
                    facility_id: "fac:test:2".into(),
                    facility_name: "Test 2".into(),
                    route: "C1".into(),
                    direction: "outer".into(),
                    kind: RampKind::GeneralExit,
                    lat: 35.68,
                    lon: 139.76,
                    restrictions: vec![],
                    status: "active".into(),
                    source: "test".into(),
                    source_date: "2026-09-16".into(),
                    coordinate_source: Some("osm".into()),
                    coordinate_status: Some("derived".into()),
                    restriction_status: Some("unverified".into()),
                },
            ],
        };
        assert!(validate_ramp_inventory(&inv).is_err());
    }

    #[test]
    fn test_reject_out_of_bounds_coords() {
        let inv = RampInventoryFile {
            version: 1,
            source: "test".into(),
            source_date: "2026-09-16".into(),
            coordinate_source: Some("osm".into()),
            description: "test".into(),
            ramps: vec![CanonicalRampInventoryItem {
                ramp_id: "ramp:test:osaka".into(),
                facility_id: "fac:test:osaka".into(),
                facility_name: "Osaka".into(),
                route: "1".into(),
                direction: "inbound".into(),
                kind: RampKind::GeneralEntry,
                lat: 34.69,  // Osaka lat - outside Kanto
                lon: 135.50, // Osaka lon
                restrictions: vec![],
                status: "active".into(),
                source: "test".into(),
                source_date: "2026-09-16".into(),
                coordinate_source: Some("osm".into()),
                coordinate_status: Some("derived".into()),
                restriction_status: Some("unverified".into()),
            }],
        };
        assert!(validate_ramp_inventory(&inv).is_err());
    }

    #[test]
    fn test_reject_invalid_provenance_status() {
        let inv = RampInventoryFile {
            version: 1,
            source: "test".into(),
            source_date: "2026-09-16".into(),
            coordinate_source: Some("osm".into()),
            description: "test".into(),
            ramps: vec![CanonicalRampInventoryItem {
                ramp_id: "ramp:test:invalid".into(),
                facility_id: "fac:test:invalid".into(),
                facility_name: "Invalid".into(),
                route: "C1".into(),
                direction: "inner".into(),
                kind: RampKind::GeneralEntry,
                lat: 35.68,
                lon: 139.76,
                restrictions: vec![],
                status: "active".into(),
                source: "test".into(),
                source_date: "2026-09-16".into(),
                coordinate_source: Some("osm".into()),
                coordinate_status: Some("bogus_status".into()),
                restriction_status: Some("unverified".into()),
            }],
        };
        assert!(validate_ramp_inventory(&inv).is_err());
    }
}
