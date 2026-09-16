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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub support_evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_capability_reason: Option<String>,
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

/// Reviewed exception for multiple official IDs sharing one physical segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPhysicalOverride {
    pub id: String,
    pub osm_way_id: i64,
    pub osm_node_id: i64,
    pub motorway_node_id: i64,
    pub ramp_ids: Vec<String>,
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// The root structure of `data/osm-ramp-bindings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsmRampBindingsFile {
    pub version: u32,
    pub source_date: String,
    pub bindings: Vec<OsmRampBinding>,
    #[serde(default)]
    pub shared_physical_overrides: Vec<SharedPhysicalOverride>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_capability_reason: Option<String>,
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

        if inv.version >= 3 {
            let expected_general = r.status == "active"
                && matches!(r.kind, RampKind::GeneralEntry | RampKind::GeneralExit);
            let valid_state = if expected_general {
                matches!(
                    r.support_state.as_deref(),
                    Some("verified_bound" | "unsupported")
                )
            } else {
                r.support_state.as_deref() == Some("not_routable")
            };
            if !valid_state {
                errors.push(format!(
                    "ramp {} has supportState {:?} inconsistent with status/kind",
                    r.ramp_id, r.support_state
                ));
            }
            if r.support_reason.as_deref().unwrap_or_default().is_empty() {
                errors.push(format!("ramp {} has empty supportReason", r.ramp_id));
            }
            if r.support_evidence.is_empty() {
                errors.push(format!("ramp {} has no supportEvidence", r.ramp_id));
            }
            let expected_capabilities: &[&str] = if expected_general {
                if r.support_state.as_deref() == Some("verified_bound") {
                    &["routable", "structural_no_loop"]
                } else {
                    &["unsupported"]
                }
            } else {
                &["not_routable"]
            };
            if !r
                .routing_capability
                .as_deref()
                .is_some_and(|value| expected_capabilities.contains(&value))
            {
                errors.push(format!(
                    "ramp {} has routingCapability {:?} inconsistent with support state",
                    r.ramp_id, r.routing_capability
                ));
            }
            if r.routing_capability_reason
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            {
                errors.push(format!(
                    "ramp {} has empty routingCapabilityReason",
                    r.ramp_id
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
            Some(ramp)
                if inv.version >= 3
                    && (ramp.status != "active"
                        || !matches!(
                            ramp.kind,
                            RampKind::GeneralEntry | RampKind::GeneralExit
                        )
                        || ramp.support_state.as_deref() != Some("verified_bound")) =>
            {
                errors.push(format!(
                    "binding for '{}' references a non-routable inventory record",
                    b.ramp_id
                ))
            }
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

    // Active general records are exhaustively classified: verified records
    // must have exactly one binding, unsupported records must have none.
    for r in &inv.ramps {
        if r.status == "active" && matches!(r.kind, RampKind::GeneralEntry | RampKind::GeneralExit)
        {
            match (inv.version, r.support_state.as_deref()) {
                (0..=2, _) if !bound_ramp_ids.contains(r.ramp_id.as_str()) => {
                    errors.push(format!(
                        "active general ramp '{}' has no OSM binding",
                        r.ramp_id
                    ));
                }
                (_, Some("verified_bound")) if !bound_ramp_ids.contains(r.ramp_id.as_str()) => {
                    errors.push(format!(
                        "verified active general ramp '{}' has no OSM binding",
                        r.ramp_id
                    ));
                }
                (_, Some("unsupported")) if bound_ramp_ids.contains(r.ramp_id.as_str()) => {
                    errors.push(format!(
                        "unsupported active general ramp '{}' must not have an OSM binding",
                        r.ramp_id
                    ));
                }
                _ => {}
            }
        }
    }

    let binding_by_ramp: HashMap<&str, &OsmRampBinding> = bindings
        .bindings
        .iter()
        .map(|b| (b.ramp_id.as_str(), b))
        .collect();
    let mut segment_members: HashMap<(i64, i64, i64), Vec<&OsmRampBinding>> = HashMap::new();
    for binding in &bindings.bindings {
        segment_members
            .entry((
                binding.osm_way_id,
                binding.osm_node_id,
                binding.motorway_node_id,
            ))
            .or_default()
            .push(binding);
    }
    let mut override_ids = HashSet::new();
    let mut override_by_segment = HashMap::new();
    for override_ in &bindings.shared_physical_overrides {
        if !override_ids.insert(override_.id.as_str()) {
            errors.push(format!(
                "duplicate shared physical override id '{}'",
                override_.id
            ));
        }
        if override_.reason.is_empty()
            || override_.evidence.is_empty()
            || override_.ramp_ids.len() < 2
        {
            errors.push(format!(
                "shared physical override '{}' lacks reason/evidence/members",
                override_.id
            ));
            continue;
        }
        let segment = (
            override_.osm_way_id,
            override_.osm_node_id,
            override_.motorway_node_id,
        );
        if let Some(previous) = override_by_segment.insert(segment, override_) {
            errors.push(format!(
                "shared physical overrides '{}' and '{}' declare the same directed segment {:?}",
                previous.id, override_.id, segment
            ));
        }
        let declared_members: HashSet<&str> =
            override_.ramp_ids.iter().map(String::as_str).collect();
        if declared_members.len() != override_.ramp_ids.len() {
            errors.push(format!(
                "shared physical override '{}' contains duplicate members",
                override_.id
            ));
        }
        for ramp_id in &override_.ramp_ids {
            match binding_by_ramp.get(ramp_id.as_str()) {
                Some(b)
                    if (b.osm_way_id, b.osm_node_id, b.motorway_node_id)
                        == (
                            override_.osm_way_id,
                            override_.osm_node_id,
                            override_.motorway_node_id,
                        ) => {}
                _ => errors.push(format!(
                    "shared physical override '{}' does not match binding for '{}'",
                    override_.id, ramp_id
                )),
            }
        }
        let actual_members: HashSet<&str> = segment_members
            .get(&segment)
            .into_iter()
            .flatten()
            .map(|binding| binding.ramp_id.as_str())
            .collect();
        if actual_members != declared_members {
            errors.push(format!(
                "shared physical override '{}' members do not exactly match directed segment {:?}: declared={:?}, actual={:?}",
                override_.id, segment, declared_members, actual_members
            ));
        }
    }

    for (segment, members) in &segment_members {
        if members.len() > 1 {
            let actual_members: HashSet<&str> = members
                .iter()
                .map(|binding| binding.ramp_id.as_str())
                .collect();
            match override_by_segment.get(segment) {
                Some(override_)
                    if override_
                        .ramp_ids
                        .iter()
                        .map(String::as_str)
                        .collect::<HashSet<_>>()
                        == actual_members => {}
                Some(override_) => errors.push(format!(
                    "duplicate directed segment {:?} does not exactly match override '{}': {:?}",
                    segment, override_.id, actual_members
                )),
                None => errors.push(format!(
                    "duplicate directed segment {:?} lacks an exact override: {:?}",
                    segment, actual_members
                )),
            }
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
    inv: &RampInventoryFile,
    osm_resp: &crate::osm::OverpassResponse,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let inventory_by_ramp_id: HashMap<&str, &CanonicalRampInventoryItem> =
        inv.ramps.iter().map(|r| (r.ramp_id.as_str(), r)).collect();
    let mut way_map: HashMap<i64, &crate::osm::OsmElement> = HashMap::new();
    let mut node_map: HashMap<i64, &crate::osm::OsmElement> = HashMap::new();

    for elem in &osm_resp.elements {
        if elem.is_way() {
            way_map.insert(elem.id, elem);
        } else if elem.is_node() {
            node_map.insert(elem.id, elem);
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
            Some(way) => {
                if !way
                    .nodes
                    .as_ref()
                    .is_some_and(|nodes| nodes.contains(&b.osm_node_id))
                {
                    errors.push(format!(
                        "binding for '{}': osmNodeId {} is not a member of osmWayId {} nodes",
                        b.ramp_id, b.osm_node_id, b.osm_way_id
                    ));
                }

                // name/ref/destination are all inspected. Only an explicit
                // access-facility label is a hard identity signal; generic road
                // names and destinations may legitimately omit the facility.
                if let Some(ramp) = inventory_by_ramp_id.get(b.ramp_id.as_str()) {
                    for key in ["name", "ref", "destination"] {
                        let Some(value) = way.get_tag(key) else {
                            continue;
                        };
                        let explicitly_names_access =
                            value.contains("入口") || value.contains("出口");
                        if explicitly_names_access && !value.contains(&ramp.facility_name) {
                            errors.push(format!(
                                "binding for '{}' conflicts with OSM {}='{}' (official facility='{}')",
                                b.ramp_id, key, value, ramp.facility_name
                            ));
                        }
                    }
                }
            }
        }

        if !node_map.contains_key(&b.motorway_node_id) {
            errors.push(format!(
                "binding for '{}' references non-existent motorwayNodeId {}",
                b.ramp_id, b.motorway_node_id
            ));
        }

        if let (Some(ramp), Some(node)) = (
            inventory_by_ramp_id.get(b.ramp_id.as_str()),
            node_map.get(&b.osm_node_id),
        ) {
            if let (Some(lat), Some(lon)) = (node.lat, node.lon) {
                // Coarse displacement guard only: most active-ramp inventory
                // coordinates are derived from this same binding, so this is
                // not an independent facility-identity check. Identity relies
                // primarily on OSM name/ref/destination signals, exact/shared
                // segment triplets, official-facility consistency, and the
                // per-ramp evidence recorded by the data pipeline.
                let distance =
                    crate::topology::haversine_distance_meters(ramp.lat, ramp.lon, lat, lon);
                if distance > 5_000 {
                    errors.push(format!(
                        "binding for '{}' is {}m from its inventory coordinate (limit 5000m)",
                        b.ramp_id, distance
                    ));
                }
            }
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
        .filter(|r| {
            r.status == "active"
                && r.kind == RampKind::GeneralEntry
                && (inv.version < 3 || r.support_state.as_deref() == Some("verified_bound"))
        })
        .map(|r| r.ramp_id.as_str())
        .collect();
    let exit_ramp_ids: HashSet<&str> = inv
        .ramps
        .iter()
        .filter(|r| {
            r.status == "active"
                && r.kind == RampKind::GeneralExit
                && (inv.version < 3 || r.support_state.as_deref() == Some("verified_bound"))
        })
        .map(|r| r.ramp_id.as_str())
        .collect();

    for (i, pair) in tariffs.verified_od_pairs.iter().enumerate() {
        if !entry_ramp_ids.contains(pair.entry_ramp_id.as_str()) {
            errors.push(format!(
                "tariff[{}] entry_ramp_id '{}' is not a verified-bound active entry ramp",
                i, pair.entry_ramp_id
            ));
        }
        if !exit_ramp_ids.contains(pair.exit_ramp_id.as_str()) {
            errors.push(format!(
                "tariff[{}] exit_ramp_id '{}' is not a verified-bound active exit ramp",
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
                support_state: item.support_state.clone(),
                support_reason: item.support_reason.clone(),
                routing_capability: item.routing_capability.clone(),
                routing_capability_reason: item.routing_capability_reason.clone(),
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
                support_state: item.support_state.clone(),
                support_reason: item.support_reason.clone(),
                routing_capability: item.routing_capability.clone(),
                routing_capability_reason: item.routing_capability_reason.clone(),
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
    let mut edge_to_ramps: HashMap<&str, Vec<&str>> = HashMap::new();
    for ramp in &graph.ramps {
        edge_to_ramps
            .entry(ramp.edge_id.as_str())
            .or_default()
            .push(ramp.id.as_str());
    }

    for p in &mut graph.billing_pairs {
        if p.entry_ramp_id.is_none() {
            if let Some(rids) = edge_to_ramps.get(p.entry_id.as_str()) {
                if let [rid] = rids.as_slice() {
                    p.entry_ramp_id = Some((*rid).to_string());
                }
            }
        }
        if p.exit_ramp_id.is_none() {
            if let Some(rids) = edge_to_ramps.get(p.exit_id.as_str()) {
                if let [rid] = rids.as_slice() {
                    p.exit_ramp_id = Some((*rid).to_string());
                }
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

/// Classifies every bound endpoint against the directed Shutoko topology.
/// A routable entry must reach, and a routable exit must be reachable from, a
/// cyclic SCC containing at least 5 km of internal mainline edges.
pub fn classify_endpoint_capabilities(graph: &Graph) -> HashMap<String, String> {
    let node_index: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();
    let mut outgoing = vec![Vec::new(); graph.nodes.len()];
    let mut incoming = vec![Vec::new(); graph.nodes.len()];
    let mut shutoko_edges = Vec::new();
    for edge in graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Shutoko)
    {
        let (Some(&from), Some(&to)) = (
            node_index.get(edge.from.as_str()),
            node_index.get(edge.to.as_str()),
        ) else {
            continue;
        };
        outgoing[from].push(to);
        incoming[to].push(from);
        shutoko_edges.push((from, to, edge.distance_meters));
    }

    let mut visited = vec![false; graph.nodes.len()];
    let mut order = Vec::with_capacity(graph.nodes.len());
    for start in 0..graph.nodes.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, next_index)) = stack.last_mut() {
            if *next_index < outgoing[*node].len() {
                let next = outgoing[*node][*next_index];
                *next_index += 1;
                if !visited[next] {
                    visited[next] = true;
                    stack.push((next, 0));
                }
            } else {
                order.push(*node);
                stack.pop();
            }
        }
    }

    let mut component = vec![usize::MAX; graph.nodes.len()];
    let mut components: Vec<Vec<usize>> = Vec::new();
    for &start in order.iter().rev() {
        if component[start] != usize::MAX {
            continue;
        }
        let component_id = components.len();
        component[start] = component_id;
        let mut nodes = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            nodes.push(node);
            for &next in &incoming[node] {
                if component[next] == usize::MAX {
                    component[next] = component_id;
                    stack.push(next);
                }
            }
        }
        components.push(nodes);
    }

    let mut internal_distance = vec![0_u64; components.len()];
    let mut cyclic = vec![false; components.len()];
    for &(from, to, distance) in &shutoko_edges {
        if component[from] == component[to] {
            internal_distance[component[from]] =
                internal_distance[component[from]].saturating_add(distance);
            if from == to || components[component[from]].len() > 1 {
                cyclic[component[from]] = true;
            }
        }
    }
    let qualifying_nodes: Vec<usize> = components
        .iter()
        .enumerate()
        .filter(|(id, _)| cyclic[*id] && internal_distance[*id] >= 5_000)
        .flat_map(|(_, nodes)| nodes.iter().copied())
        .collect();

    fn closure(starts: &[usize], adjacency: &[Vec<usize>]) -> HashSet<usize> {
        let mut seen: HashSet<usize> = starts.iter().copied().collect();
        let mut stack = starts.to_vec();
        while let Some(node) = stack.pop() {
            for &next in &adjacency[node] {
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        seen
    }

    let can_reach_loop = closure(&qualifying_nodes, &incoming);
    let reachable_from_loop = closure(&qualifying_nodes, &outgoing);
    graph
        .ramps
        .iter()
        .map(|ramp| {
            let capable = node_index
                .get(ramp.mainline_node_id.as_str())
                .is_some_and(|node| match ramp.kind {
                    RampKind::GeneralEntry => can_reach_loop.contains(node),
                    RampKind::GeneralExit => reachable_from_loop.contains(node),
                    _ => false,
                });
            (
                ramp.id.clone(),
                if capable {
                    "routable"
                } else {
                    "structural_no_loop"
                }
                .to_string(),
            )
        })
        .collect()
}

/// Hard assertion that the machine-readable inventory declaration exactly
/// matches the current directed topology classification for every bound ramp.
pub fn validate_endpoint_capability_contract(
    graph: &Graph,
    inv: &RampInventoryFile,
) -> Result<(), Vec<String>> {
    let actual = classify_endpoint_capabilities(graph);
    let inventory_by_id: HashMap<&str, &CanonicalRampInventoryItem> =
        inv.ramps.iter().map(|r| (r.ramp_id.as_str(), r)).collect();
    let mut errors = Vec::new();
    for (ramp_id, capability) in actual {
        let declared = inventory_by_id
            .get(ramp_id.as_str())
            .and_then(|ramp| ramp.routing_capability.as_deref());
        if declared != Some(capability.as_str()) {
            errors.push(format!(
                "endpoint capability mismatch for '{}': declared={:?}, actual={}",
                ramp_id, declared, capability
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Ensures every verified billing pair resolves both endpoint edges to exactly
/// one verified-bound ramp and that declared endpoint names match the official
/// facility names. Unverified seeds remain diagnostic-only.
pub fn validate_verified_billing_pair_endpoints(graph: &Graph) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let ramps_by_id: HashMap<&str, &Ramp> =
        graph.ramps.iter().map(|r| (r.id.as_str(), r)).collect();
    let mut ramps_by_edge: HashMap<&str, Vec<&Ramp>> = HashMap::new();
    for ramp in &graph.ramps {
        ramps_by_edge
            .entry(ramp.edge_id.as_str())
            .or_default()
            .push(ramp);
    }

    for pair in &graph.billing_pairs {
        if pair.status != crate::model::VerificationStatus::Verified {
            continue;
        }
        let Some(entry_id) = pair.entry_ramp_id.as_deref() else {
            errors.push(format!(
                "verified billing pair '{}' has null entryRampId",
                pair.id
            ));
            continue;
        };
        let Some(exit_id) = pair.exit_ramp_id.as_deref() else {
            errors.push(format!(
                "verified billing pair '{}' has null exitRampId",
                pair.id
            ));
            continue;
        };
        let Some(entry) = ramps_by_id.get(entry_id) else {
            errors.push(format!(
                "verified billing pair '{}' entryRampId '{}' is not verified-bound",
                pair.id, entry_id
            ));
            continue;
        };
        let Some(exit) = ramps_by_id.get(exit_id) else {
            errors.push(format!(
                "verified billing pair '{}' exitRampId '{}' is not verified-bound",
                pair.id, exit_id
            ));
            continue;
        };
        if entry.kind != RampKind::GeneralEntry || exit.kind != RampKind::GeneralExit {
            errors.push(format!(
                "verified billing pair '{}' resolves to wrong endpoint kinds",
                pair.id
            ));
        }
        if !pair
            .entry_name
            .as_deref()
            .is_some_and(|name| name.contains(&entry.name))
        {
            errors.push(format!(
                "verified billing pair '{}' entryName {:?} conflicts with ramp '{}' ({})",
                pair.id, pair.entry_name, entry.id, entry.name
            ));
        }
        if !pair
            .exit_name
            .as_deref()
            .is_some_and(|name| name.contains(&exit.name))
        {
            errors.push(format!(
                "verified billing pair '{}' exitName {:?} conflicts with ramp '{}' ({})",
                pair.id, pair.exit_name, exit.id, exit.name
            ));
        }
        let expected_route_direction = pair.id.split(':').nth(1);
        let entry_route_direction = format!(
            "{}-{}",
            entry.route.to_ascii_lowercase(),
            entry.direction.to_ascii_lowercase()
        );
        let exit_route_direction = format!(
            "{}-{}",
            exit.route.to_ascii_lowercase(),
            exit.direction.to_ascii_lowercase()
        );
        if expected_route_direction.is_some_and(|expected| {
            expected.contains('-')
                && (expected != entry_route_direction || expected != exit_route_direction)
        }) {
            errors.push(format!(
                "verified billing pair '{}' route/direction conflicts with entry '{}' and exit '{}'",
                pair.id, entry.id, exit.id
            ));
        }
        if entry.edge_id != pair.entry_id || exit.edge_id != pair.exit_id {
            errors.push(format!(
                "verified billing pair '{}' ramp IDs do not reverse-map its exact endpoint edges",
                pair.id
            ));
        }
        for (role, edge_id, ramp_id) in [
            ("entry", pair.entry_id.as_str(), entry.id.as_str()),
            ("exit", pair.exit_id.as_str(), exit.id.as_str()),
        ] {
            let reverse_mapped = ramps_by_edge
                .get(edge_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if reverse_mapped.len() != 1 || reverse_mapped[0].id != ramp_id {
                let candidates: Vec<&str> =
                    reverse_mapped.iter().map(|ramp| ramp.id.as_str()).collect();
                errors.push(format!(
                    "verified billing pair '{}' {} edge '{}' does not uniquely reverse-map to '{}': {:?}",
                    pair.id, role, edge_id, ramp_id, candidates
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
            232,
            "only verified active general ramps should have OSM bindings"
        );

        let binding_by_id: HashMap<_, _> = bindings
            .bindings
            .iter()
            .map(|b| (b.ramp_id.as_str(), b))
            .collect();
        let active_general: Vec<_> = inv
            .ramps
            .iter()
            .filter(|r| {
                r.status == "active"
                    && matches!(r.kind, RampKind::GeneralEntry | RampKind::GeneralExit)
            })
            .collect();
        assert_eq!(active_general.len(), 371);
        assert_eq!(
            active_general
                .iter()
                .filter(|r| r.support_state.as_deref() == Some("verified_bound"))
                .count(),
            232
        );
        assert_eq!(
            active_general
                .iter()
                .filter(|r| r.support_state.as_deref() == Some("unsupported"))
                .count(),
            139
        );
        for ramp in &active_general {
            let bound = binding_by_id.contains_key(ramp.ramp_id.as_str());
            assert_eq!(
                ramp.support_state.as_deref() == Some("verified_bound"),
                bound,
                "verified-bound XOR unsupported contract failed for {}",
                ramp.ramp_id
            );
        }

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
        let osm_res = validate_osm_ramp_bindings_against_osm(&bindings, &inv, &osm_resp);
        assert!(
            osm_res.is_ok(),
            "bindings against OSM fixture validation failed: {:?}",
            osm_res
        );

        // Every verified binding names the exact directed graph edge and kind.
        let graph_path = find_data_file("fixtures/generated/graph.json");
        let graph: Graph = serde_json::from_str(&fs::read_to_string(graph_path).unwrap()).unwrap();
        assert_eq!(
            classify_endpoint_capabilities(&graph)
                .values()
                .filter(|capability| capability.as_str() == "structural_no_loop")
                .count(),
            35
        );
        assert!(validate_endpoint_capability_contract(&graph, &inv).is_ok());
        for b in &bindings.bindings {
            let ramp = inv.ramps.iter().find(|r| r.ramp_id == b.ramp_id).unwrap();
            let expected_kind = if ramp.kind == RampKind::GeneralEntry {
                EdgeKind::Entry
            } else {
                EdgeKind::Exit
            };
            let (from, to) = if ramp.kind == RampKind::GeneralEntry {
                (
                    format!("n:{}", b.osm_node_id),
                    format!("n:{}", b.motorway_node_id),
                )
            } else {
                (
                    format!("n:{}", b.motorway_node_id),
                    format!("n:{}", b.osm_node_id),
                )
            };
            assert!(
                graph.edges.iter().any(|e| {
                    e.id.split(':').nth(1) == Some(format!("w{}", b.osm_way_id).as_str())
                        && e.from == from
                        && e.to == to
                        && e.kind == expected_kind
                }),
                "{} does not reference an exact directed {:?} edge",
                b.ramp_id,
                expected_kind
            );
        }

        // Every reused directed segment, including two directions of the same
        // facility, must have one declaration whose triplet and complete member
        // set exactly match. Display-name/facility equality is irrelevant.
        let mut by_segment: HashMap<(i64, i64, i64), Vec<&OsmRampBinding>> = HashMap::new();
        for b in &bindings.bindings {
            by_segment
                .entry((b.osm_way_id, b.osm_node_id, b.motorway_node_id))
                .or_default()
                .push(b);
        }
        let declared_by_segment: HashMap<_, HashSet<_>> = bindings
            .shared_physical_overrides
            .iter()
            .map(|override_| {
                (
                    (
                        override_.osm_way_id,
                        override_.osm_node_id,
                        override_.motorway_node_id,
                    ),
                    override_
                        .ramp_ids
                        .iter()
                        .map(String::as_str)
                        .collect::<HashSet<_>>(),
                )
            })
            .collect();
        assert_eq!(
            declared_by_segment.len(),
            bindings.shared_physical_overrides.len(),
            "each override must declare a unique directed segment"
        );
        let mut duplicate_segments = HashSet::new();
        for (segment, group) in &by_segment {
            if group.len() > 1 {
                duplicate_segments.insert(*segment);
                let actual_members: HashSet<_> = group.iter().map(|b| b.ramp_id.as_str()).collect();
                assert_eq!(
                    declared_by_segment.get(segment),
                    Some(&actual_members),
                    "duplicate must exactly match its declared override: {:?}",
                    segment
                );
            }
        }
        assert_eq!(
            duplicate_segments,
            declared_by_segment.keys().copied().collect(),
            "override segments and actual duplicate triplets must be identical"
        );
        let override_ids: HashSet<_> = bindings
            .shared_physical_overrides
            .iter()
            .map(|o| o.id.as_str())
            .collect();
        assert_eq!(override_ids, HashSet::from(["G15", "G27", "G53"]));

        // Regression lockouts for the known false nearest-edge mappings.
        let forbidden = [
            ("ramp:b-west:maihama-entry", 1006338881),
            ("ramp:5-outbound:toda-entry", 409002623),
            ("ramp:5-inbound:toda-exit", 409002624),
            ("ramp:6s-outbound:yashio-exit", 251808352),
            ("ramp:y-south:yaesu-entry", 378284514),
            ("ramp:y-south:yaesu-exit", 203301443),
            ("ramp:y-south:marunouchi-exit", 1232083939),
            ("ramp:k1-inbound:asada-exit", 1022523258),
        ];
        for (ramp_id, way_id) in forbidden {
            assert_ne!(
                binding_by_id.get(ramp_id).map(|b| b.osm_way_id),
                Some(way_id)
            );
        }
        for ramp in &inv.ramps {
            if ramp.status != "active"
                || matches!(ramp.kind, RampKind::BoundaryIn | RampKind::BoundaryOut)
            {
                assert!(!binding_by_id.contains_key(ramp.ramp_id.as_str()));
            }
        }
    }

    #[test]
    fn test_reject_inexact_shared_physical_override_contract() {
        let inv_path = find_data_file("data/ramp-inventory.json");
        let bin_path = find_data_file("data/osm-ramp-bindings.json");
        let inv: RampInventoryFile =
            serde_json::from_str(&fs::read_to_string(inv_path).unwrap()).unwrap();
        let bindings: OsmRampBindingsFile =
            serde_json::from_str(&fs::read_to_string(bin_path).unwrap()).unwrap();

        // G53 is the regression case: its facility IDs differ while every
        // display name is 大師, so removing the declaration must fail.
        let mut missing = bindings.clone();
        missing
            .shared_physical_overrides
            .retain(|override_| override_.id != "G53");
        assert!(validate_osm_ramp_bindings(&missing, &inv).is_err());

        // Facility equality is deliberately irrelevant: even when every G27
        // member is declared as one facility, its duplicate directed triplet
        // still requires an exact override.
        let mut same_facility_inventory = inv.clone();
        for ramp in &mut same_facility_inventory.ramps {
            if [
                "ramp:6s-outbound:kahei-exit",
                "ramp:6s-inbound:kahei-654-exit",
            ]
            .contains(&ramp.ramp_id.as_str())
            {
                ramp.facility_id = "fac:6s:kahei".into();
            }
        }
        let mut same_facility_missing = bindings.clone();
        same_facility_missing
            .shared_physical_overrides
            .retain(|override_| override_.id != "G27");
        assert!(
            validate_osm_ramp_bindings(&same_facility_missing, &same_facility_inventory).is_err()
        );

        // An override may neither omit a real segment member nor declare a
        // different triplet for otherwise valid members.
        let mut incomplete = bindings.clone();
        incomplete
            .shared_physical_overrides
            .iter_mut()
            .find(|override_| override_.id == "G53")
            .unwrap()
            .ramp_ids
            .pop();
        assert!(validate_osm_ramp_bindings(&incomplete, &inv).is_err());

        let mut wrong_triplet = bindings;
        wrong_triplet
            .shared_physical_overrides
            .iter_mut()
            .find(|override_| override_.id == "G53")
            .unwrap()
            .motorway_node_id += 1;
        assert!(validate_osm_ramp_bindings(&wrong_triplet, &inv).is_err());
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
                    support_state: None,
                    support_reason: None,
                    support_evidence: vec![],
                    routing_capability: None,
                    routing_capability_reason: None,
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
                    support_state: None,
                    support_reason: None,
                    support_evidence: vec![],
                    routing_capability: None,
                    routing_capability_reason: None,
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
                support_state: None,
                support_reason: None,
                support_evidence: vec![],
                routing_capability: None,
                routing_capability_reason: None,
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
                support_state: None,
                support_reason: None,
                support_evidence: vec![],
                routing_capability: None,
                routing_capability_reason: None,
            }],
        };
        assert!(validate_ramp_inventory(&inv).is_err());
    }
}
