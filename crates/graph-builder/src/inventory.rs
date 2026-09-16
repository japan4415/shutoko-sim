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
}

/// The root structure of `data/ramp-inventory.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RampInventoryFile {
    pub version: u32,
    pub source: String,
    pub source_date: String,
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

/// Distance-based toll calculation rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffRules {
    pub vehicle_profile: String,
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
    let inventory_ramp_ids: HashSet<&str> = inv.ramps.iter().map(|r| r.ramp_id.as_str()).collect();
    let mut bound_ramp_ids = HashSet::new();

    for (i, b) in bindings.bindings.iter().enumerate() {
        if !inventory_ramp_ids.contains(b.ramp_id.as_str()) {
            errors.push(format!(
                "binding[{}] references unknown ramp_id '{}'",
                i, b.ramp_id
            ));
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
        bound_ramp_ids.insert(b.ramp_id.as_str());
    }

    // Check for ramps without any binding
    for r in &inv.ramps {
        if !bound_ramp_ids.contains(r.ramp_id.as_str()) {
            errors.push(format!(
                "canonical ramp '{}' has no OSM binding in bindings file",
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
        if let Some(b) = binding {
            if let Some(candidate_edges) = edges_by_way.get(&b.osm_way_id) {
                // Determine target edge kind based on ramp kind
                let is_entry = matches!(item.kind, RampKind::GeneralEntry | RampKind::BoundaryIn);
                let target_kind = if is_entry {
                    EdgeKind::Entry
                } else {
                    EdgeKind::Exit
                };

                // Priority 1: Match edge with exact target EdgeKind
                matched_edge = candidate_edges
                    .iter()
                    .find(|e| e.kind == target_kind)
                    .copied();

                // Priority 2: Match first or last segment
                if matched_edge.is_none() && !candidate_edges.is_empty() {
                    matched_edge = if is_entry {
                        candidate_edges.first().copied()
                    } else {
                        candidate_edges.last().copied()
                    };
                }
            }
        }

        if let (Some(b), Some(edge)) = (binding, matched_edge) {
            let is_entry = matches!(item.kind, RampKind::GeneralEntry | RampKind::BoundaryIn);
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

    #[test]
    fn test_validate_real_inventory_file() {
        let path = Path::new("../../data/ramp-inventory.json");
        if !path.exists() {
            // Check from repo root
            let alt = Path::new("data/ramp-inventory.json");
            if !alt.exists() {
                return;
            }
        }
        let real_path = if path.exists() {
            path
        } else {
            Path::new("data/ramp-inventory.json")
        };
        let content = fs::read_to_string(real_path).expect("read ramp-inventory.json");
        let inv: RampInventoryFile = serde_json::from_str(&content).expect("parse ramp-inventory");
        let res = validate_ramp_inventory(&inv);
        assert!(res.is_ok(), "ramp inventory validation failed: {:?}", res);
        assert_eq!(inv.ramps.len(), 339, "expected 339 total canonical ramps");

        let general_entries = inv
            .ramps
            .iter()
            .filter(|r| r.kind == RampKind::GeneralEntry)
            .count();
        let general_exits = inv
            .ramps
            .iter()
            .filter(|r| r.kind == RampKind::GeneralExit)
            .count();
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

        assert_eq!(general_entries, 156);
        assert_eq!(general_exits, 159);
        assert_eq!(boundary_in, 12);
        assert_eq!(boundary_out, 12);
    }

    #[test]
    fn test_validate_real_bindings_file() {
        let inv_path = Path::new("data/ramp-inventory.json");
        let bin_path = Path::new("data/osm-ramp-bindings.json");
        if !inv_path.exists() || !bin_path.exists() {
            return;
        }
        let inv_str = fs::read_to_string(inv_path).unwrap();
        let bin_str = fs::read_to_string(bin_path).unwrap();
        let inv: RampInventoryFile = serde_json::from_str(&inv_str).unwrap();
        let bindings: OsmRampBindingsFile = serde_json::from_str(&bin_str).unwrap();

        let res = validate_osm_ramp_bindings(&bindings, &inv);
        assert!(res.is_ok(), "bindings validation failed: {:?}", res);
        assert_eq!(bindings.bindings.len(), 339);
    }

    #[test]
    fn test_validate_real_tariffs_file() {
        let inv_path = Path::new("data/ramp-inventory.json");
        let tar_path = Path::new("data/od-tariffs.json");
        if !inv_path.exists() || !tar_path.exists() {
            return;
        }
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
            }],
        };
        assert!(validate_ramp_inventory(&inv).is_err());
    }
}
