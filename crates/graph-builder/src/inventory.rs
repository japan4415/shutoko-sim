//! Canonical ramp inventory, OSM bindings, and OD tariff integration.
//!
//! Provides validation and binding of:
//! - `data/ramp-inventory.json` (canonical population of all Shutoko ramps)
//! - `data/osm-ramp-bindings.json` (explicit OSM way/node bindings)
//! - `data/od-tariffs.json` (official ETC OD tariffs and distance rules)

use crate::model::{EdgeKind, Graph, OdTariff, Ramp, RampKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsmRampDirectedSegment {
    pub segment_id: String,
    pub osm_way_ids: Vec<i64>,
    pub osm_node_ids: Vec<i64>,
    pub edge_ids: Vec<String>,
    pub from_node_id: String,
    pub to_node_id: String,
    pub edge_ids_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsmRampRouteEvidence {
    pub route_id: String,
    pub direction: String,
    pub relation_id: i64,
    pub relation_member_way_id: i64,
    pub relation_member_role: String,
    pub official_exit_number: String,
    pub official_downstream_exit_numbers: Vec<String>,
    pub ground_way_id: i64,
    pub ground_way_name: String,
    pub first_exit_after_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsmRampBindingCandidate {
    pub candidate_id: String,
    pub ramp_id: String,
    pub status: String,
    pub public_projection: String,
    pub direction: String,
    pub unresolved_reason: String,
    pub unresolved_reason_codes: Vec<String>,
    pub directed_segments: Vec<OsmRampDirectedSegment>,
    pub route_evidence: OsmRampRouteEvidence,
    pub support_evidence: Vec<String>,
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
    pub binding_candidates: Vec<OsmRampBindingCandidate>,
    #[serde(default)]
    pub shared_physical_overrides: Vec<SharedPhysicalOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstPublicRoadConnectionDiagnosticMismatch {
    pub ramp_id: String,
    pub evidence_id: String,
    pub declared_support_state: String,
    pub resolved_support_state: String,
    pub declared_ground_node_id: Option<i64>,
    pub resolved_ground_node_id: Option<i64>,
    pub resolved_ground_way_id: Option<i64>,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstPublicRoadConnectionDiagnosticReport {
    pub rule: String,
    pub schema_binding_total: usize,
    pub schema_binding_matched: usize,
    pub schema_binding_mismatched: usize,
    pub schema_binding_mismatches: Vec<FirstPublicRoadConnectionDiagnosticMismatch>,
    pub binding_candidate_total: usize,
    pub binding_candidate_matched: usize,
    pub binding_candidate_mismatched: usize,
    pub binding_candidate_mismatches: Vec<FirstPublicRoadConnectionDiagnosticMismatch>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffDocumentV3 {
    pub document_id: String,
    pub edition: String,
    pub url: String,
    pub cache_path: String,
    pub document_sha256: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffRoundingV3 {
    pub mode: String,
    pub multiple_yen: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffSourceRefV3 {
    pub document_id: Option<String>,
    pub source: Option<String>,
    pub page: Option<u64>,
    pub status: Option<String>,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffRuleV3 {
    pub rule_id: String,
    pub vehicle_class: String,
    pub payment_method: String,
    pub fare_basis: String,
    pub distance_unit_meters: u64,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub rate_micros_yen_per_unit: u64,
    pub terminal_charge_yen: u64,
    pub tax_basis_points: u64,
    pub minimum_yen: u64,
    pub maximum_yen: u64,
    pub minimum_distance_meters: Option<u64>,
    pub rounding: TariffRoundingV3,
    pub source_refs: Vec<TariffSourceRefV3>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistanceEvidenceV3 {
    pub evidence_id: String,
    pub document_id: String,
    pub edition: String,
    pub document_sha256: String,
    pub page: u64,
    pub row_label: String,
    pub column_label: String,
    pub cell: String,
    pub distance_meters: u64,
    pub distance_label: String,
    pub observed_base_fare_yen: u64,
    pub calculated_base_fare_yen: u64,
    pub entry_ramp_id: String,
    pub exit_ramp_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingEvidenceV3 {
    pub evidence_id: String,
    pub document_id: String,
    pub edition: String,
    pub page: Option<u64>,
    pub row_label: String,
    pub column_label: String,
    pub cell: Option<String>,
    pub status: String,
    pub observed_base_fare_yen: Option<u64>,
    pub observed_distance_meters: Option<u64>,
    pub reviewed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffPriceV3 {
    pub status: String,
    pub tariff_status: String,
    pub amount_yen: Option<u64>,
    pub observed_base_fare_yen: Option<u64>,
    pub observed_distance_meters: Option<u64>,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub rule_id: String,
    pub evidence_id: String,
    pub distance_evidence_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TariffAssignmentV3 {
    pub assignment_id: String,
    pub od_key: String,
    pub pair_ids: Vec<String>,
    pub entry_name: String,
    pub exit_name: String,
    pub entry_ramp_id: String,
    pub exit_ramp_id: String,
    pub vehicle_profile: String,
    pub vehicle_class: String,
    pub payment_method: String,
    pub fare_basis: String,
    pub billing_distance_meters: u64,
    pub distance_evidence_id: String,
    pub verification_status: String,
    pub prices: Vec<TariffPriceV3>,
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
    #[serde(default)]
    pub vehicle_profile: Option<String>,
    #[serde(default)]
    pub vehicle_class: Option<String>,
    #[serde(default)]
    pub payment_method: Option<String>,
    #[serde(default)]
    pub fare_basis: Option<String>,
    #[serde(default)]
    pub documents: Vec<TariffDocumentV3>,
    #[serde(default)]
    pub tariff_rules: Vec<TariffRuleV3>,
    #[serde(default)]
    pub distance_evidence: Vec<DistanceEvidenceV3>,
    #[serde(default)]
    pub pending_evidence: Vec<PendingEvidenceV3>,
    #[serde(default)]
    pub assignments: Vec<TariffAssignmentV3>,
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

    if !bindings.binding_candidates.is_empty() && bindings.version < 4 {
        errors.push("bindingCandidates require bindings version 4 or newer".into());
    }
    let mut candidate_ids = HashSet::new();
    let mut candidate_ramp_ids = HashSet::new();
    let mut candidate_segment_ids = HashSet::new();
    let mut verified_candidate_ramp_ids = HashSet::new();
    for (i, candidate) in bindings.binding_candidates.iter().enumerate() {
        if candidate.candidate_id.is_empty() {
            errors.push(format!("bindingCandidate[{}] has empty candidateId", i));
        } else if !candidate_ids.insert(candidate.candidate_id.as_str()) {
            errors.push(format!(
                "duplicate binding candidate id '{}'",
                candidate.candidate_id
            ));
        }
        if !candidate_ramp_ids.insert(candidate.ramp_id.as_str()) {
            errors.push(format!(
                "multiple binding candidates reference ramp '{}'",
                candidate.ramp_id
            ));
        }
        match inventory_by_ramp_id.get(candidate.ramp_id.as_str()) {
            None => errors.push(format!(
                "binding candidate '{}' references unknown ramp '{}'",
                candidate.candidate_id, candidate.ramp_id
            )),
            Some(ramp)
                if ramp.status != "active"
                    || !matches!(ramp.kind, RampKind::GeneralEntry | RampKind::GeneralExit) =>
            {
                errors.push(format!(
                    "binding candidate '{}' references non-general ramp '{}'",
                    candidate.candidate_id, candidate.ramp_id
                ));
            }
            Some(ramp) if ramp.direction != candidate.direction => errors.push(format!(
                "binding candidate '{}' has direction '{}' but inventory requires '{}'",
                candidate.candidate_id, candidate.direction, ramp.direction
            )),
            Some(ramp)
                if candidate.status == "verified_bound"
                    && ramp.support_state.as_deref() != Some("verified_bound") =>
            {
                errors.push(format!(
                    "binding candidate '{}' is verified_bound but inventory projection is not",
                    candidate.candidate_id
                ))
            }
            Some(ramp)
                if candidate.status != "verified_bound"
                    && ramp.support_state.as_deref() != Some("unsupported") =>
            {
                errors.push(format!(
                    "binding candidate '{}' is not verified_bound but inventory projection is",
                    candidate.candidate_id
                ))
            }
            Some(_) => {}
        }
        if bound_ramp_ids.contains(candidate.ramp_id.as_str()) {
            errors.push(format!(
                "binding candidate '{}' must not also have a schema-2 binding",
                candidate.candidate_id
            ));
        }
        if !matches!(
            candidate.status.as_str(),
            "unresolved" | "unsupported" | "verified_bound"
        ) {
            errors.push(format!(
                "binding candidate '{}' has invalid status '{}'",
                candidate.candidate_id, candidate.status
            ));
        }
        if candidate.status == "verified_bound" {
            verified_candidate_ramp_ids.insert(candidate.ramp_id.as_str());
            if candidate.public_projection != "included_verified" {
                errors.push(format!(
                    "binding candidate '{}' has invalid publicProjection '{}'",
                    candidate.candidate_id, candidate.public_projection
                ));
            }
            if !candidate.unresolved_reason.trim().is_empty()
                || !candidate.unresolved_reason_codes.is_empty()
            {
                errors.push(format!(
                    "binding candidate '{}' is verified_bound but retains unresolved evidence",
                    candidate.candidate_id
                ));
            }
            if candidate.support_evidence.is_empty() {
                errors.push(format!(
                    "binding candidate '{}' lacks supportEvidence",
                    candidate.candidate_id
                ));
            }
        } else {
            if candidate.public_projection != "excluded_unresolved" {
                errors.push(format!(
                    "binding candidate '{}' has invalid publicProjection '{}'",
                    candidate.candidate_id, candidate.public_projection
                ));
            }
            if candidate.unresolved_reason.trim().is_empty()
                || candidate.unresolved_reason_codes.is_empty()
                || candidate.support_evidence.is_empty()
            {
                errors.push(format!(
                    "binding candidate '{}' lacks reason/reasonCodes/evidence",
                    candidate.candidate_id
                ));
            }
        }
        if candidate.directed_segments.len() != 1 {
            errors.push(format!(
                "binding candidate '{}' must have exactly one directed segment",
                candidate.candidate_id
            ));
        }
        for segment in &candidate.directed_segments {
            if segment.segment_id.is_empty()
                || !candidate_segment_ids.insert(segment.segment_id.as_str())
            {
                errors.push(format!(
                    "binding candidate '{}' has empty or duplicate segmentId '{}'",
                    candidate.candidate_id, segment.segment_id
                ));
            }
            if segment.osm_way_ids.len() < 2
                || segment.osm_node_ids.len() < 3
                || segment.edge_ids.len() < 2
            {
                errors.push(format!(
                    "binding candidate '{}' has a non-multi-way directed segment",
                    candidate.candidate_id
                ));
            }
            if segment.osm_way_ids.iter().any(|way_id| *way_id <= 0)
                || segment.osm_node_ids.iter().any(|node_id| *node_id <= 0)
                || segment.edge_ids.iter().any(String::is_empty)
            {
                errors.push(format!(
                    "binding candidate '{}' has invalid way/node/edge IDs",
                    candidate.candidate_id
                ));
            }
            let expected_hash = crate::manifest::compute_sha256(
                serde_json::to_string(&segment.edge_ids)
                    .unwrap_or_default()
                    .as_bytes(),
            );
            if segment.edge_ids_sha256 != expected_hash {
                errors.push(format!(
                    "binding candidate '{}' has invalid edgeIdsSha256",
                    candidate.candidate_id
                ));
            }
        }
        let route = &candidate.route_evidence;
        if route.direction != candidate.direction
            || route.relation_id <= 0
            || route.relation_member_way_id <= 0
            || route.relation_member_role.is_empty()
            || route.official_exit_number.is_empty()
            || route.official_downstream_exit_numbers.is_empty()
            || route.ground_way_id <= 0
            || route.ground_way_name.is_empty()
            || !route.first_exit_after_branch
        {
            errors.push(format!(
                "binding candidate '{}' has incomplete route/facility evidence",
                candidate.candidate_id
            ));
        }
    }

    // Active general records are exhaustively classified: verified records
    // must have exactly one binding, unsupported records must have none.
    for r in &inv.ramps {
        if r.status == "active" && matches!(r.kind, RampKind::GeneralEntry | RampKind::GeneralExit)
        {
            let has_schema_binding = bound_ramp_ids.contains(r.ramp_id.as_str());
            let has_verified_candidate = verified_candidate_ramp_ids.contains(r.ramp_id.as_str());
            match (inv.version, r.support_state.as_deref()) {
                (0..=2, _) if !has_schema_binding => {
                    errors.push(format!(
                        "active general ramp '{}' has no OSM binding",
                        r.ramp_id
                    ));
                }
                (_, Some("verified_bound")) if !has_schema_binding && !has_verified_candidate => {
                    errors.push(format!(
                        "verified active general ramp '{}' has no exact binding evidence",
                        r.ramp_id
                    ));
                }
                (_, Some("unsupported")) if has_schema_binding || has_verified_candidate => {
                    errors.push(format!(
                        "unsupported active general ramp '{}' must not have exact binding evidence",
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

    for candidate in &bindings.binding_candidates {
        if let Err(candidate_errors) =
            audit_osm_ramp_binding_candidate_against_osm(candidate, inv, osm_resp)
        {
            errors.extend(
                candidate_errors.into_iter().map(|error| {
                    format!("binding candidate '{}': {error}", candidate.candidate_id)
                }),
            );
        }
        let resolution = audit_first_public_road_connection(candidate, inv, osm_resp)
            .unwrap_or_else(
                |errors| crate::topology::FirstPublicRoadConnectionResolution {
                    rule: crate::topology::FIRST_PUBLIC_ROAD_CONNECTION_RULE.to_string(),
                    support_state: crate::seed::EndpointSupportState::Unresolved,
                    ground_node_id: None,
                    ground_way_id: None,
                    ground_way_name: None,
                    reason_codes: vec!["CANDIDATE_EVIDENCE_ERROR".to_string()],
                    notes: errors,
                },
            );
        if candidate.status == "verified_bound" {
            let ramp = inventory_by_ramp_id
                .get(candidate.ramp_id.as_str())
                .expect("candidate ramp was checked by candidate audit");
            let flow = ramp_flow_direction(ramp.kind).expect("candidate is a general ramp");
            let expected_ground_node =
                candidate
                    .directed_segments
                    .first()
                    .and_then(|segment| match flow {
                        crate::topology::RampFlowDirection::Exit => {
                            segment.osm_node_ids.last().copied()
                        }
                        crate::topology::RampFlowDirection::Entry => {
                            segment.osm_node_ids.first().copied()
                        }
                    });
            if resolution.support_state != crate::seed::EndpointSupportState::VerifiedBound
                || resolution.ground_way_id != Some(candidate.route_evidence.ground_way_id)
                || resolution.ground_node_id != expected_ground_node
            {
                errors.push(format!(
                    "binding candidate '{}' is not proven by {}: ground={:?}, way={:?}, reasons={:?}",
                    candidate.candidate_id,
                    crate::topology::FIRST_PUBLIC_ROAD_CONNECTION_RULE,
                    resolution.ground_node_id,
                    resolution.ground_way_id,
                    resolution.reason_codes
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

pub fn audit_osm_ramp_binding_candidate_against_osm(
    candidate: &OsmRampBindingCandidate,
    inv: &RampInventoryFile,
    osm_resp: &crate::osm::OverpassResponse,
) -> Result<Vec<String>, Vec<String>> {
    let mut errors = Vec::new();
    let Some(ramp) = inv
        .ramps
        .iter()
        .find(|ramp| ramp.ramp_id == candidate.ramp_id)
    else {
        return Err(vec![format!(
            "binding candidate '{}' references unknown ramp '{}'",
            candidate.candidate_id, candidate.ramp_id
        )]);
    };
    if ramp.direction != candidate.direction {
        errors.push(format!(
            "binding candidate '{}' direction '{}' does not match inventory '{}'",
            candidate.candidate_id, candidate.direction, ramp.direction
        ));
    }
    let [segment] = candidate.directed_segments.as_slice() else {
        return Err(vec![format!(
            "binding candidate '{}' must have exactly one directed segment",
            candidate.candidate_id
        )]);
    };
    if segment.osm_node_ids.len() < 2 {
        return Err(vec![format!(
            "binding candidate '{}' has fewer than two directed nodes",
            candidate.candidate_id
        )]);
    }

    let mut node_map: HashMap<i64, &crate::osm::OsmElement> = HashMap::new();
    let mut way_map: HashMap<i64, &crate::osm::OsmElement> = HashMap::new();
    let mut relation_map: HashMap<i64, &crate::osm::OsmElement> = HashMap::new();
    for element in &osm_resp.elements {
        if element.is_node() {
            node_map.insert(element.id, element);
        } else if element.is_way() {
            way_map.insert(element.id, element);
        } else if element.is_relation() {
            relation_map.insert(element.id, element);
        }
    }

    let mut surface_nodes: HashSet<i64> = HashSet::new();
    let mut mainline_nodes: HashSet<i64> = HashSet::new();
    let mut network_outgoing: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for way in way_map.values() {
        let highway = way.get_tag("highway");
        let Some(nodes) = way.nodes.as_ref() else {
            continue;
        };
        if crate::topology::is_vehicle_highway(highway.unwrap_or_default(), way.get_tag("service"))
            == Some(true)
        {
            surface_nodes.extend(nodes.iter().copied());
        }
        if highway == Some("motorway") {
            mainline_nodes.extend(nodes.iter().copied());
        }
        if !matches!(highway, Some("motorway" | "motorway_link")) || nodes.len() < 2 {
            continue;
        }
        let oneway = crate::topology::parse_oneway(way);
        if oneway != crate::topology::OnewayDirection::ReverseOnly {
            for (edge_index, (from, to)) in nodes.iter().zip(nodes.iter().skip(1)).enumerate() {
                network_outgoing
                    .entry(*from)
                    .or_default()
                    .push((*to, format!("e:w{}:{}:f", way.id, edge_index)));
            }
        }
        if oneway != crate::topology::OnewayDirection::ForwardOnly {
            for (edge_index, (from, to)) in nodes.iter().zip(nodes.iter().skip(1)).enumerate() {
                network_outgoing
                    .entry(*to)
                    .or_default()
                    .push((*from, format!("e:w{}:{}:r", way.id, edge_index)));
            }
        }
    }

    let mut expected_node_ids = Vec::new();
    let mut expected_edge_ids = Vec::new();
    for (way_index, way_id) in segment.osm_way_ids.iter().enumerate() {
        let Some(way) = way_map.get(way_id) else {
            errors.push(format!(
                "binding candidate '{}' references missing way {}",
                candidate.candidate_id, way_id
            ));
            continue;
        };
        if way.get_tag("highway") != Some("motorway_link") {
            errors.push(format!(
                "binding candidate '{}' way {} is not motorway_link",
                candidate.candidate_id, way_id
            ));
        }
        if crate::topology::parse_oneway(way) != crate::topology::OnewayDirection::ForwardOnly {
            errors.push(format!(
                "binding candidate '{}' way {} is not forward-only",
                candidate.candidate_id, way_id
            ));
        }
        if let Some(name) = way.get_tag("name") {
            if !name.contains(&ramp.facility_name) || !name.contains("出口") {
                errors.push(format!(
                    "binding candidate '{}' way {} has conflicting name '{}'",
                    candidate.candidate_id, way_id, name
                ));
            }
        }
        if let Some(destination) = way.get_tag("destination") {
            if destination.contains("入口")
                || (destination.contains("出口") && !destination.contains(&ramp.facility_name))
            {
                errors.push(format!(
                    "binding candidate '{}' way {} has conflicting destination '{}'",
                    candidate.candidate_id, way_id, destination
                ));
            }
        }
        let Some(nodes) = way.nodes.as_ref().filter(|nodes| nodes.len() >= 2) else {
            errors.push(format!(
                "binding candidate '{}' way {} has fewer than two nodes",
                candidate.candidate_id, way_id
            ));
            continue;
        };
        if way_index == 0 {
            expected_node_ids.extend(nodes.iter().copied());
        } else {
            expected_node_ids.extend(nodes.iter().skip(1).copied());
        }
        expected_edge_ids.extend(
            (0..nodes.len() - 1).map(|edge_index| format!("e:w{}:{}:f", way_id, edge_index)),
        );
    }

    if segment.osm_node_ids != expected_node_ids {
        errors.push(format!(
            "binding candidate '{}' osmNodeIds do not match ordered way nodes",
            candidate.candidate_id
        ));
    }
    if segment.edge_ids != expected_edge_ids {
        errors.push(format!(
            "binding candidate '{}' edgeIds do not match ordered forward way segments",
            candidate.candidate_id
        ));
    }
    let expected_hash = crate::manifest::compute_sha256(
        serde_json::to_string(&segment.edge_ids)
            .unwrap_or_default()
            .as_bytes(),
    );
    if segment.edge_ids_sha256 != expected_hash {
        errors.push(format!(
            "binding candidate '{}' has invalid edgeIdsSha256",
            candidate.candidate_id
        ));
    }
    let Some((mainline_node_id, ground_node_id)) = segment
        .osm_node_ids
        .first()
        .zip(segment.osm_node_ids.last())
    else {
        return Err(vec![format!(
            "binding candidate '{}' has no endpoints",
            candidate.candidate_id
        )]);
    };
    if segment.from_node_id != format!("n:{mainline_node_id}")
        || segment.to_node_id != format!("n:{ground_node_id}")
    {
        errors.push(format!(
            "binding candidate '{}' endpoint IDs do not match node order",
            candidate.candidate_id
        ));
    }

    let route = &candidate.route_evidence;
    if route.direction != candidate.direction {
        errors.push(format!(
            "binding candidate '{}' route direction does not match candidate direction",
            candidate.candidate_id
        ));
    }
    let Some(start_node) = node_map.get(mainline_node_id) else {
        errors.push(format!(
            "binding candidate '{}' references missing mainline node {}",
            candidate.candidate_id, mainline_node_id
        ));
        return Err(errors);
    };
    let expected_start_name = format!("{}出口", ramp.facility_name);
    if start_node.get_tag("highway") != Some("motorway_junction")
        || start_node.get_tag("name") != Some(expected_start_name.as_str())
        || start_node.get_tag("ref") != Some(route.official_exit_number.as_str())
    {
        errors.push(format!(
            "binding candidate '{}' mainline node is not the official {} exit {}",
            candidate.candidate_id, ramp.facility_name, route.official_exit_number
        ));
    }
    let Some(mainline_way) = way_map.get(&route.relation_member_way_id) else {
        errors.push(format!(
            "binding candidate '{}' references missing route member way {}",
            candidate.candidate_id, route.relation_member_way_id
        ));
        return Err(errors);
    };
    if mainline_way.get_tag("highway") != Some("motorway")
        || !mainline_way
            .nodes
            .as_ref()
            .is_some_and(|nodes| nodes.contains(mainline_node_id))
    {
        errors.push(format!(
            "binding candidate '{}' start is not on declared mainline way {}",
            candidate.candidate_id, route.relation_member_way_id
        ));
    }
    let Some(relation) = relation_map.get(&route.relation_id) else {
        errors.push(format!(
            "binding candidate '{}' references missing route relation {}",
            candidate.candidate_id, route.relation_id
        ));
        return Err(errors);
    };
    if relation.get_tag("ref") != Some(route.route_id.as_str()) {
        errors.push(format!(
            "binding candidate '{}' relation {} is not route {}",
            candidate.candidate_id, route.relation_id, route.route_id
        ));
    }
    let members = relation.members.as_deref().unwrap_or_default();
    if !members.iter().any(|member| {
        member.member_type == "way"
            && member.ref_id == route.relation_member_way_id
            && member.role == route.relation_member_role
    }) {
        errors.push(format!(
            "binding candidate '{}' relation {} lacks way {} role {}",
            candidate.candidate_id,
            route.relation_id,
            route.relation_member_way_id,
            route.relation_member_role
        ));
    }
    for way_id in &segment.osm_way_ids {
        if members
            .iter()
            .any(|member| member.member_type == "way" && member.ref_id == *way_id)
        {
            errors.push(format!(
                "binding candidate '{}' ramp way {} must not be a route relation member",
                candidate.candidate_id, way_id
            ));
        }
    }

    let Some(ground_way) = way_map.get(&route.ground_way_id) else {
        errors.push(format!(
            "binding candidate '{}' references missing ground way {}",
            candidate.candidate_id, route.ground_way_id
        ));
        return Err(errors);
    };
    if crate::topology::is_vehicle_highway(
        ground_way.get_tag("highway").unwrap_or_default(),
        ground_way.get_tag("service"),
    ) != Some(true)
        || ground_way.get_tag("name") != Some(route.ground_way_name.as_str())
        || !ground_way
            .nodes
            .as_ref()
            .is_some_and(|nodes| nodes.contains(ground_node_id))
    {
        errors.push(format!(
            "binding candidate '{}' endpoint {} is not on declared ground way {}",
            candidate.candidate_id, ground_node_id, route.ground_way_id
        ));
    }

    if !route.first_exit_after_branch
        || route.official_downstream_exit_numbers.first() != Some(&route.official_exit_number)
    {
        errors.push(format!(
            "binding candidate '{}' official exit {} is not first after the branch",
            candidate.candidate_id, route.official_exit_number
        ));
    }
    let official_numbers = route
        .official_downstream_exit_numbers
        .iter()
        .filter_map(|number| number.parse::<u32>().ok())
        .collect::<Vec<_>>();
    let declared_official_number = route.official_exit_number.parse::<u32>().ok();
    if official_numbers.len() != route.official_downstream_exit_numbers.len()
        || official_numbers.first() != declared_official_number.as_ref()
        || official_numbers.windows(2).any(|pair| pair[0] >= pair[1])
    {
        errors.push(format!(
            "binding candidate '{}' has invalid official downstream order",
            candidate.candidate_id
        ));
    }

    let expected_next_edge = expected_node_ids
        .windows(2)
        .zip(&expected_edge_ids)
        .map(|(nodes, edge_id)| (nodes[0], edge_id))
        .collect::<HashMap<_, _>>();
    let mut reason_codes = HashSet::new();
    let mut early_surface_nodes = Vec::new();
    for node_id in segment
        .osm_node_ids
        .iter()
        .skip(1)
        .take(segment.osm_node_ids.len() - 2)
    {
        if mainline_nodes.contains(node_id) {
            errors.push(format!(
                "binding candidate '{}' has an intermediate mainline branch at node {}",
                candidate.candidate_id, node_id
            ));
        }
        if surface_nodes.contains(node_id) {
            early_surface_nodes.push(*node_id);
        }
        if let Some(outgoing) = network_outgoing.get(node_id) {
            if let Some(expected_edge_id) = expected_next_edge.get(node_id) {
                let alternatives = outgoing
                    .iter()
                    .filter(|(_, edge_id)| edge_id.as_str() != expected_edge_id.as_str())
                    .collect::<Vec<_>>();
                if !alternatives.is_empty() {
                    errors.push(format!(
                        "binding candidate '{}' has an ambiguous internal branch at node {}",
                        candidate.candidate_id, node_id
                    ));
                }
            }
        }
    }
    if !early_surface_nodes.is_empty() {
        reason_codes.insert("EARLY_SURFACE_CONNECTION".to_string());
        reason_codes.insert("MULTIPLE_GROUND_CONNECTION_CANDIDATES".to_string());
    }

    let declared_codes = candidate
        .unresolved_reason_codes
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    if candidate.status == "unresolved" && reason_codes.is_empty() {
        errors.push(format!(
            "binding candidate '{}' is unresolved without a topology reason",
            candidate.candidate_id
        ));
    }
    if candidate.status == "unresolved" && declared_codes != reason_codes {
        errors.push(format!(
            "binding candidate '{}' unresolved reason codes {:?} do not match audit {:?}",
            candidate.candidate_id, candidate.unresolved_reason_codes, reason_codes
        ));
    }
    if candidate.status == "verified_bound" && !reason_codes.is_empty() {
        errors.push(format!(
            "binding candidate '{}' is verified_bound but has unresolved reason codes {:?}",
            candidate.candidate_id, reason_codes
        ));
    }

    if errors.is_empty() {
        let mut reason_codes = reason_codes.into_iter().collect::<Vec<_>>();
        reason_codes.sort();
        Ok(reason_codes)
    } else {
        Err(errors)
    }
}

pub fn calculate_versioned_tariff_yen(distance_meters: u64, rule: &TariffRuleV3) -> u64 {
    if rule
        .minimum_distance_meters
        .is_some_and(|minimum| distance_meters <= minimum)
    {
        return rule.minimum_yen;
    }
    let units = distance_meters.div_ceil(rule.distance_unit_meters);
    let subtotal_micros = rule
        .terminal_charge_yen
        .saturating_mul(1_000_000)
        .saturating_add(units.saturating_mul(rule.rate_micros_yen_per_unit));
    let taxed_micros = subtotal_micros.saturating_mul(rule.tax_basis_points) / 10_000;
    let rounded = ((taxed_micros + 5_000_000) / 10_000_000) * 10;
    rounded.clamp(rule.minimum_yen, rule.maximum_yen)
}

fn endpoint_label_matches(name: &str, label: &str, entry: bool) -> bool {
    let name = name.trim();
    let label = label.trim();
    let endpoint = if entry {
        name.strip_suffix("入口").unwrap_or(name)
    } else {
        name.strip_suffix("出口").unwrap_or(name)
    };
    endpoint == label
}

fn evidence_matches_assignment(
    assignment: &TariffAssignmentV3,
    evidence: &DistanceEvidenceV3,
) -> bool {
    evidence.entry_ramp_id == assignment.entry_ramp_id
        && evidence.exit_ramp_id == assignment.exit_ramp_id
        && endpoint_label_matches(&assignment.entry_name, &evidence.row_label, true)
        && endpoint_label_matches(&assignment.exit_name, &evidence.column_label, false)
}

fn pending_evidence_matches_assignment(
    assignment: &TariffAssignmentV3,
    evidence: &PendingEvidenceV3,
) -> bool {
    endpoint_label_matches(&assignment.entry_name, &evidence.row_label, true)
        && endpoint_label_matches(&assignment.exit_name, &evidence.column_label, false)
}

fn rule_references_evidence_document(
    rule: &TariffRuleV3,
    document_id: &str,
    edition: &str,
    page: Option<u64>,
) -> bool {
    rule.source_refs.iter().any(|source| {
        source.document_id.as_deref() == Some(document_id)
            && source.page == page
            && edition_overlaps_rule(rule, edition)
    })
}

fn edition_overlaps_rule(rule: &TariffRuleV3, edition: &str) -> bool {
    let Some((year, month)) = edition
        .split_once('-')
        .and_then(|(year, month)| Some((year.parse::<u32>().ok()?, month.parse::<u32>().ok()?)))
        .filter(|(_, month)| (1..=12).contains(month))
    else {
        return false;
    };
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let edition_start = format!("{year:04}-{month:02}-01T00:00:00Z");
    let edition_end = format!("{next_year:04}-{next_month:02}-01T00:00:00Z");
    let rule_end = rule
        .effective_to
        .as_deref()
        .unwrap_or("9999-12-31T23:59:59Z");
    edition_start.as_str() < rule_end && edition_end.as_str() > rule.effective_from.as_str()
}

fn validate_versioned_od_tariffs(
    tariffs: &OdTariffsFile,
    inv: &RampInventoryFile,
    errors: &mut Vec<String>,
) {
    if tariffs.vehicle_profile.is_none() {
        errors.push("tariff v3 vehicleProfile is required".to_string());
    }
    if tariffs.vehicle_class.is_none() {
        errors.push("tariff v3 vehicleClass is required".to_string());
    }
    if tariffs.payment_method.is_none() {
        errors.push("tariff v3 paymentMethod is required".to_string());
    }
    if tariffs.fare_basis.is_none() {
        errors.push("tariff v3 fareBasis is required".to_string());
    }
    if tariffs.tariff_rules.is_empty() {
        errors.push("tariff v3 tariffRules cannot be empty".to_string());
    }
    if tariffs.assignments.is_empty() {
        errors.push("tariff v3 assignments cannot be empty".to_string());
    }

    let mut document_by_id = HashMap::new();
    for document in &tariffs.documents {
        if document_by_id
            .insert(document.document_id.as_str(), document)
            .is_some()
        {
            errors.push(format!(
                "tariff v3 documentId '{}' is duplicated",
                document.document_id
            ));
        }
        if document.document_id.is_empty()
            || document.edition.is_empty()
            || document.url.is_empty()
            || document.cache_path.is_empty()
        {
            errors.push(format!(
                "tariff v3 document '{}' has an empty required field",
                document.document_id
            ));
        }
        if document.status.starts_with("verified")
            && document
                .document_sha256
                .as_deref()
                .is_none_or(str::is_empty)
        {
            errors.push(format!(
                "tariff v3 verified document '{}' requires documentSha256",
                document.document_id
            ));
        }
    }

    let mut rule_by_id = HashMap::new();
    let mut rule_bounds = HashMap::new();
    let mut rule_groups: BTreeMap<(&str, &str, &str), Vec<&TariffRuleV3>> = BTreeMap::new();
    for rule in &tariffs.tariff_rules {
        if rule_by_id.insert(rule.rule_id.as_str(), rule).is_some() {
            errors.push(format!("tariff v3 ruleId '{}' is duplicated", rule.rule_id));
        }
        if rule.vehicle_class.is_empty()
            || rule.payment_method.is_empty()
            || rule.fare_basis.is_empty()
        {
            errors.push(format!(
                "tariff v3 rule '{}' has an empty scope",
                rule.rule_id
            ));
        }
        if rule.distance_unit_meters == 0 {
            errors.push(format!(
                "tariff v3 rule '{}' has zero distanceUnitMeters",
                rule.rule_id
            ));
        }
        if rule.rate_micros_yen_per_unit == 0 {
            errors.push(format!(
                "tariff v3 rule '{}' has zero rateMicrosYenPerUnit",
                rule.rule_id
            ));
        }
        if rule.tax_basis_points == 0 {
            errors.push(format!(
                "tariff v3 rule '{}' has zero taxBasisPoints",
                rule.rule_id
            ));
        }
        if rule.minimum_yen > rule.maximum_yen {
            errors.push(format!(
                "tariff v3 rule '{}' has minimumYen above maximumYen",
                rule.rule_id
            ));
        }
        if rule.minimum_distance_meters.is_none() {
            errors.push(format!(
                "tariff v3 rule '{}' requires minimumDistanceMeters",
                rule.rule_id
            ));
        }
        if rule.rounding.mode != "half_up" || rule.rounding.multiple_yen == 0 {
            errors.push(format!(
                "tariff v3 rule '{}' has unsupported rounding",
                rule.rule_id
            ));
        }
        if rule.source_refs.is_empty() {
            errors.push(format!(
                "tariff v3 rule '{}' requires sourceRefs",
                rule.rule_id
            ));
        }
        for source_ref in &rule.source_refs {
            if source_ref.location.is_empty() {
                errors.push(format!(
                    "tariff v3 rule '{}' has an empty sourceRef location",
                    rule.rule_id
                ));
            }
            if let Some(document_id) = &source_ref.document_id {
                if !document_by_id.contains_key(document_id.as_str()) {
                    errors.push(format!(
                        "tariff v3 rule '{}' references unknown document '{}'",
                        rule.rule_id, document_id
                    ));
                }
            } else if source_ref.source.as_deref().is_none_or(str::is_empty) {
                errors.push(format!(
                    "tariff v3 rule '{}' has a sourceRef without documentId or source",
                    rule.rule_id
                ));
            }
        }
        let effective_from = match crate::validate::parse_utc_timestamp(&rule.effective_from) {
            Ok(value) => value,
            Err(message) => {
                errors.push(format!(
                    "tariff v3 rule '{}' has invalid effectiveFrom: {}",
                    rule.rule_id, message
                ));
                continue;
            }
        };
        let effective_to = match &rule.effective_to {
            Some(value) => match crate::validate::parse_utc_timestamp(value) {
                Ok(parsed) => Some(parsed),
                Err(message) => {
                    errors.push(format!(
                        "tariff v3 rule '{}' has invalid effectiveTo: {}",
                        rule.rule_id, message
                    ));
                    continue;
                }
            },
            None => None,
        };
        if effective_to.is_some_and(|end| end <= effective_from) {
            errors.push(format!(
                "tariff v3 rule '{}' effectiveTo must be after effectiveFrom",
                rule.rule_id
            ));
        }
        rule_bounds.insert(rule.rule_id.as_str(), (effective_from, effective_to));
        rule_groups
            .entry((
                rule.vehicle_class.as_str(),
                rule.payment_method.as_str(),
                rule.fare_basis.as_str(),
            ))
            .or_default()
            .push(rule);
    }
    for rules in rule_groups.values_mut() {
        rules.sort_by(|left, right| left.effective_from.cmp(&right.effective_from));
        for pair in rules.windows(2) {
            let previous = rule_bounds.get(pair[0].rule_id.as_str());
            let current = rule_bounds.get(pair[1].rule_id.as_str());
            if let (Some((_, previous_end)), Some((current_start, _))) = (previous, current) {
                if previous_end.is_none() || previous_end.is_some_and(|end| end > *current_start) {
                    errors.push(format!(
                        "tariff v3 rules '{}' and '{}' have overlapping effective intervals",
                        pair[0].rule_id, pair[1].rule_id
                    ));
                }
            }
        }
    }

    let mut distance_by_id = HashMap::new();
    let mut evidence_ids = HashSet::new();
    for evidence in &tariffs.distance_evidence {
        if !evidence_ids.insert(evidence.evidence_id.as_str()) {
            errors.push(format!(
                "tariff v3 evidenceId '{}' is duplicated",
                evidence.evidence_id
            ));
        }
        if distance_by_id
            .insert(evidence.evidence_id.as_str(), evidence)
            .is_some()
        {
            errors.push(format!(
                "tariff v3 distance evidenceId '{}' is duplicated",
                evidence.evidence_id
            ));
        }
        let Some(document) = document_by_id.get(evidence.document_id.as_str()) else {
            errors.push(format!(
                "tariff v3 distance evidence '{}' references unknown document '{}'",
                evidence.evidence_id, evidence.document_id
            ));
            continue;
        };
        if document.edition != evidence.edition
            || document.document_sha256.as_deref() != Some(evidence.document_sha256.as_str())
        {
            errors.push(format!(
                "tariff v3 distance evidence '{}' does not match its document",
                evidence.evidence_id
            ));
        }
        if evidence.page == 0
            || evidence.row_label.is_empty()
            || evidence.column_label.is_empty()
            || evidence.cell.is_empty()
            || evidence.entry_ramp_id.is_empty()
            || evidence.exit_ramp_id.is_empty()
        {
            errors.push(format!(
                "tariff v3 distance evidence '{}' has an empty PDF or endpoint field",
                evidence.evidence_id
            ));
        }
        if evidence.distance_meters == 0
            || evidence.observed_base_fare_yen == 0
            || evidence.observed_base_fare_yen != evidence.calculated_base_fare_yen
        {
            errors.push(format!(
                "tariff v3 distance evidence '{}' has inconsistent observed values",
                evidence.evidence_id
            ));
        }
    }

    let mut pending_by_id = HashMap::new();
    for evidence in &tariffs.pending_evidence {
        if !evidence_ids.insert(evidence.evidence_id.as_str()) {
            errors.push(format!(
                "tariff v3 evidenceId '{}' is reused across evidence sets",
                evidence.evidence_id
            ));
        }
        if pending_by_id
            .insert(evidence.evidence_id.as_str(), evidence)
            .is_some()
        {
            errors.push(format!(
                "tariff v3 pending evidenceId '{}' is duplicated",
                evidence.evidence_id
            ));
        }
        let Some(document) = document_by_id.get(evidence.document_id.as_str()) else {
            errors.push(format!(
                "tariff v3 pending evidence '{}' references unknown document '{}'",
                evidence.evidence_id, evidence.document_id
            ));
            continue;
        };
        if document.edition != evidence.edition {
            errors.push(format!(
                "tariff v3 pending evidence '{}' does not match its document edition",
                evidence.evidence_id
            ));
        }
        if evidence.status != "pending_manual_pdf_review"
            || evidence.page.is_some()
            || evidence.cell.is_some()
            || evidence.observed_base_fare_yen.is_some()
            || evidence.observed_distance_meters.is_some()
            || evidence.reviewed_at.is_some()
        {
            errors.push(format!(
                "tariff v3 pending evidence '{}' is not fully unpriced",
                evidence.evidence_id
            ));
        }
    }

    let inventory_by_id: HashMap<&str, &CanonicalRampInventoryItem> = inv
        .ramps
        .iter()
        .map(|ramp| (ramp.ramp_id.as_str(), ramp))
        .collect();
    let mut assignment_ids = HashSet::new();
    let mut od_keys = HashSet::new();
    let mut assignments_by_ramps: HashMap<(&str, &str), Vec<&TariffAssignmentV3>> = HashMap::new();
    for assignment in &tariffs.assignments {
        if !assignment_ids.insert(assignment.assignment_id.as_str()) {
            errors.push(format!(
                "tariff v3 assignmentId '{}' is duplicated",
                assignment.assignment_id
            ));
        }
        if !od_keys.insert(assignment.od_key.as_str()) {
            errors.push(format!(
                "tariff v3 odKey '{}' is duplicated",
                assignment.od_key
            ));
        }
        if assignment.pair_ids.is_empty()
            || assignment.pair_ids.iter().any(String::is_empty)
            || assignment.pair_ids.iter().collect::<HashSet<_>>().len() != assignment.pair_ids.len()
        {
            errors.push(format!(
                "tariff v3 assignment '{}' has empty or duplicate pairIds",
                assignment.assignment_id
            ));
        }
        if assignment.vehicle_profile.as_str() != tariffs.vehicle_profile.as_deref().unwrap_or("")
            || assignment.vehicle_class.as_str() != tariffs.vehicle_class.as_deref().unwrap_or("")
            || assignment.payment_method.as_str() != tariffs.payment_method.as_deref().unwrap_or("")
            || assignment.fare_basis.as_str() != tariffs.fare_basis.as_deref().unwrap_or("")
        {
            errors.push(format!(
                "tariff v3 assignment '{}' does not match the root scope",
                assignment.assignment_id
            ));
        }
        match inventory_by_id.get(assignment.entry_ramp_id.as_str()) {
            Some(ramp) if ramp.status == "active" && ramp.kind == RampKind::GeneralEntry => {}
            _ => errors.push(format!(
                "tariff v3 assignment '{}' has invalid entryRampId '{}'",
                assignment.assignment_id, assignment.entry_ramp_id
            )),
        }
        match inventory_by_id.get(assignment.exit_ramp_id.as_str()) {
            Some(ramp) if ramp.status == "active" && ramp.kind == RampKind::GeneralExit => {}
            _ => errors.push(format!(
                "tariff v3 assignment '{}' has invalid exitRampId '{}'",
                assignment.assignment_id, assignment.exit_ramp_id
            )),
        }
        assignments_by_ramps
            .entry((
                assignment.entry_ramp_id.as_str(),
                assignment.exit_ramp_id.as_str(),
            ))
            .or_default()
            .push(assignment);

        let Some(base_evidence) = distance_by_id.get(assignment.distance_evidence_id.as_str())
        else {
            errors.push(format!(
                "tariff v3 assignment '{}' references unknown distanceEvidenceId '{}'",
                assignment.assignment_id, assignment.distance_evidence_id
            ));
            continue;
        };
        if base_evidence.entry_ramp_id != assignment.entry_ramp_id
            || base_evidence.exit_ramp_id != assignment.exit_ramp_id
            || base_evidence.distance_meters != assignment.billing_distance_meters
            || !evidence_matches_assignment(assignment, base_evidence)
        {
            errors.push(format!(
                "tariff v3 assignment '{}' does not match its base distance evidence",
                assignment.assignment_id
            ));
        }

        let expected_rule_ids = tariffs
            .tariff_rules
            .iter()
            .filter(|rule| {
                rule.vehicle_class == assignment.vehicle_class
                    && rule.payment_method == assignment.payment_method
                    && rule.fare_basis == assignment.fare_basis
            })
            .map(|rule| rule.rule_id.as_str())
            .collect::<HashSet<_>>();
        if assignment.prices.len() != expected_rule_ids.len() {
            errors.push(format!(
                "tariff v3 assignment '{}' has {} prices for {} applicable rules",
                assignment.assignment_id,
                assignment.prices.len(),
                expected_rule_ids.len()
            ));
        }
        let mut used_rule_ids = HashSet::new();
        for price in &assignment.prices {
            if !used_rule_ids.insert(price.rule_id.as_str()) {
                errors.push(format!(
                    "tariff v3 assignment '{}' repeats price ruleId '{}'",
                    assignment.assignment_id, price.rule_id
                ));
            }
            if !expected_rule_ids.contains(price.rule_id.as_str()) {
                errors.push(format!(
                    "tariff v3 assignment '{}' uses out-of-scope rule '{}'",
                    assignment.assignment_id, price.rule_id
                ));
            }
            let Some(rule) = rule_by_id.get(price.rule_id.as_str()) else {
                errors.push(format!(
                    "tariff v3 assignment '{}' references unknown ruleId '{}'",
                    assignment.assignment_id, price.rule_id
                ));
                continue;
            };
            if price.effective_from != rule.effective_from
                || price.effective_to != rule.effective_to
            {
                errors.push(format!(
                    "tariff v3 assignment '{}' price interval does not match rule '{}'",
                    assignment.assignment_id, price.rule_id
                ));
            }
            if price.status == "priced" {
                let Some(evidence) = distance_by_id.get(price.evidence_id.as_str()) else {
                    errors.push(format!(
                        "tariff v3 assignment '{}' priced record references unknown evidence '{}'",
                        assignment.assignment_id, price.evidence_id
                    ));
                    continue;
                };
                let Some(distance_evidence) =
                    distance_by_id.get(price.distance_evidence_id.as_str())
                else {
                    errors.push(format!(
                        "tariff v3 assignment '{}' priced record references unknown distance evidence '{}'",
                        assignment.assignment_id, price.distance_evidence_id
                    ));
                    continue;
                };
                if price.evidence_id != price.distance_evidence_id
                    || evidence.evidence_id != distance_evidence.evidence_id
                    || !evidence_matches_assignment(assignment, evidence)
                    || !rule_references_evidence_document(
                        rule,
                        evidence.document_id.as_str(),
                        evidence.edition.as_str(),
                        Some(evidence.page),
                    )
                    || price.tariff_status != "priced"
                    || price.amount_yen != Some(evidence.observed_base_fare_yen)
                    || price.observed_base_fare_yen != Some(evidence.observed_base_fare_yen)
                    || price.observed_distance_meters != Some(evidence.distance_meters)
                {
                    errors.push(format!(
                        "tariff v3 assignment '{}' priced record does not match its evidence",
                        assignment.assignment_id
                    ));
                }
            } else if price.status == "pending_pdf_review" {
                let Some(evidence) = pending_by_id.get(price.evidence_id.as_str()) else {
                    errors.push(format!(
                        "tariff v3 assignment '{}' pending record references unknown evidence '{}'",
                        assignment.assignment_id, price.evidence_id
                    ));
                    continue;
                };
                if price.evidence_id != price.distance_evidence_id
                    || evidence.evidence_id != price.distance_evidence_id
                    || !pending_evidence_matches_assignment(assignment, evidence)
                    || !rule_references_evidence_document(
                        rule,
                        evidence.document_id.as_str(),
                        evidence.edition.as_str(),
                        None,
                    )
                    || price.tariff_status != "unpriced"
                    || price.amount_yen.is_some()
                    || price.observed_base_fare_yen.is_some()
                    || price.observed_distance_meters.is_some()
                {
                    errors.push(format!(
                        "tariff v3 assignment '{}' pending record is not fully unpriced",
                        assignment.assignment_id
                    ));
                }
            } else {
                errors.push(format!(
                    "tariff v3 assignment '{}' has unknown price status '{}'",
                    assignment.assignment_id, price.status
                ));
            }
        }
        for rule_id in expected_rule_ids.difference(&used_rule_ids) {
            errors.push(format!(
                "tariff v3 assignment '{}' is missing ruleId '{}'",
                assignment.assignment_id, rule_id
            ));
        }
    }

    for pair in &tariffs.verified_od_pairs {
        let Some(assignments) =
            assignments_by_ramps.get(&(pair.entry_ramp_id.as_str(), pair.exit_ramp_id.as_str()))
        else {
            continue;
        };
        for assignment in assignments {
            if pair.billing_distance_meters != assignment.billing_distance_meters {
                errors.push(format!(
                    "legacy tariff projection for '{} -> {}' does not match assignment '{}'",
                    pair.entry_ramp_id, pair.exit_ramp_id, assignment.assignment_id
                ));
            }
            let matching_price = assignment.prices.iter().find(|price| {
                price.effective_from == pair.effective_from.clone().unwrap_or_default()
                    && price.effective_to == pair.effective_to
            });
            let exact_match = match (pair.amount_yen, matching_price) {
                (Some(amount), Some(price)) => price.amount_yen == Some(amount),
                (None, Some(price)) => price.amount_yen.is_none(),
                _ => false,
            };
            let compatible_open_projection = pair.effective_to.is_none()
                && assignment.prices.iter().any(|price| {
                    price.status == "priced"
                        && Some(price.effective_from.as_str()) == pair.effective_from.as_deref()
                        && price.amount_yen == pair.amount_yen
                });
            if !exact_match && !compatible_open_projection {
                errors.push(format!(
                    "legacy tariff projection for '{} -> {}' does not match assignment '{}'",
                    pair.entry_ramp_id, pair.exit_ramp_id, assignment.assignment_id
                ));
            }
        }
    }
}

/// Audits a binding candidate against the `firstPublicRoadConnection/v1` rule.
pub fn audit_first_public_road_connection(
    candidate: &OsmRampBindingCandidate,
    inv: &RampInventoryFile,
    osm_resp: &crate::osm::OverpassResponse,
) -> Result<crate::topology::FirstPublicRoadConnectionResolution, Vec<String>> {
    let Some(ramp) = inv
        .ramps
        .iter()
        .find(|ramp| ramp.ramp_id == candidate.ramp_id)
    else {
        return Err(vec![format!(
            "binding candidate '{}' references unknown ramp '{}'",
            candidate.candidate_id, candidate.ramp_id
        )]);
    };
    let [segment] = candidate.directed_segments.as_slice() else {
        return Err(vec![format!(
            "binding candidate '{}' must have exactly one directed segment",
            candidate.candidate_id
        )]);
    };

    let flow = match ramp.kind {
        RampKind::GeneralExit | RampKind::BoundaryOut => crate::topology::RampFlowDirection::Exit,
        RampKind::GeneralEntry | RampKind::BoundaryIn => crate::topology::RampFlowDirection::Entry,
    };

    Ok(
        crate::topology::resolve_first_public_road_connection_from_osm(
            &segment.osm_node_ids,
            flow,
            osm_resp,
            Some(candidate.route_evidence.ground_way_id),
        ),
    )
}

fn ramp_flow_direction(kind: RampKind) -> Option<crate::topology::RampFlowDirection> {
    match kind {
        RampKind::GeneralExit | RampKind::BoundaryOut => {
            Some(crate::topology::RampFlowDirection::Exit)
        }
        RampKind::GeneralEntry | RampKind::BoundaryIn => {
            Some(crate::topology::RampFlowDirection::Entry)
        }
    }
}

fn schema_binding_chain(
    binding: &OsmRampBinding,
    flow: crate::topology::RampFlowDirection,
    way: &crate::osm::OsmElement,
) -> Result<Vec<i64>, String> {
    let nodes = way
        .nodes
        .as_ref()
        .ok_or_else(|| format!("binding way {} has no nodes", binding.osm_way_id))?;
    let motorway_index = nodes
        .iter()
        .position(|node| *node == binding.motorway_node_id)
        .ok_or_else(|| {
            format!(
                "binding way {} does not contain motorway node {}",
                binding.osm_way_id, binding.motorway_node_id
            )
        })?;
    let ground_index = nodes
        .iter()
        .position(|node| *node == binding.osm_node_id)
        .ok_or_else(|| {
            format!(
                "binding way {} does not contain ground node {}",
                binding.osm_way_id, binding.osm_node_id
            )
        })?;
    match flow {
        crate::topology::RampFlowDirection::Exit if motorway_index < ground_index => {
            Ok(vec![binding.osm_node_id])
        }
        crate::topology::RampFlowDirection::Entry if ground_index < motorway_index => {
            Ok(vec![binding.osm_node_id])
        }
        _ => Err(format!(
            "binding {} endpoints are not ordered for {:?}",
            binding.ramp_id, flow
        )),
    }
}

fn endpoint_support_state_wire_value(state: crate::seed::EndpointSupportState) -> &'static str {
    match state {
        crate::seed::EndpointSupportState::VerifiedBound => "verified_bound",
        crate::seed::EndpointSupportState::Unresolved => "unresolved",
        crate::seed::EndpointSupportState::Unsupported => "unsupported",
    }
}

fn diagnostic_mismatch(
    ramp_id: String,
    evidence_id: String,
    declared_support_state: String,
    resolution: &crate::topology::FirstPublicRoadConnectionResolution,
    declared_ground_node_id: Option<i64>,
    mut reason_codes: Vec<String>,
) -> FirstPublicRoadConnectionDiagnosticMismatch {
    reason_codes.extend(resolution.reason_codes.iter().cloned());
    reason_codes.sort();
    reason_codes.dedup();
    FirstPublicRoadConnectionDiagnosticMismatch {
        ramp_id,
        evidence_id,
        declared_support_state,
        resolved_support_state: endpoint_support_state_wire_value(resolution.support_state)
            .to_string(),
        declared_ground_node_id,
        resolved_ground_node_id: resolution.ground_node_id,
        resolved_ground_way_id: resolution.ground_way_id,
        reason_codes,
    }
}

pub fn audit_first_public_road_connections(
    bindings: &OsmRampBindingsFile,
    inv: &RampInventoryFile,
    osm_resp: &crate::osm::OverpassResponse,
) -> FirstPublicRoadConnectionDiagnosticReport {
    let inventory_by_ramp_id: HashMap<&str, &CanonicalRampInventoryItem> = inv
        .ramps
        .iter()
        .map(|ramp| (ramp.ramp_id.as_str(), ramp))
        .collect();
    let mut way_map: HashMap<i64, &crate::osm::OsmElement> = HashMap::new();
    let mut node_to_ways: HashMap<i64, Vec<i64>> = HashMap::new();
    for element in &osm_resp.elements {
        if element.is_way() {
            way_map.insert(element.id, element);
            if let Some(nodes) = &element.nodes {
                for node_id in nodes {
                    node_to_ways.entry(*node_id).or_default().push(element.id);
                }
            }
        }
    }

    let mut ordered_bindings = bindings.bindings.iter().collect::<Vec<_>>();
    ordered_bindings.sort_by(|left, right| left.ramp_id.cmp(&right.ramp_id));
    let mut schema_binding_mismatches = Vec::new();
    for binding in ordered_bindings {
        let Some(ramp) = inventory_by_ramp_id.get(binding.ramp_id.as_str()) else {
            continue;
        };
        let Some(flow) = ramp_flow_direction(ramp.kind) else {
            continue;
        };
        let chain = way_map
            .get(&binding.osm_way_id)
            .ok_or_else(|| format!("missing binding way {}", binding.osm_way_id))
            .and_then(|way| schema_binding_chain(binding, flow, way));
        let resolution = match chain {
            Ok(chain) => crate::topology::resolve_first_public_road_connection(
                &chain,
                flow,
                &way_map,
                &node_to_ways,
                None,
            ),
            Err(_) => crate::topology::FirstPublicRoadConnectionResolution {
                rule: crate::topology::FIRST_PUBLIC_ROAD_CONNECTION_RULE.to_string(),
                support_state: crate::seed::EndpointSupportState::Unresolved,
                ground_node_id: None,
                ground_way_id: None,
                ground_way_name: None,
                reason_codes: vec!["BINDING_CHAIN_UNAVAILABLE".to_string()],
                notes: Vec::new(),
            },
        };
        let mut reason_codes = Vec::new();
        if resolution.support_state != crate::seed::EndpointSupportState::VerifiedBound {
            reason_codes.push("SCHEMA_BINDING_STATUS_MISMATCH".to_string());
        }
        if resolution.ground_node_id != Some(binding.osm_node_id) {
            reason_codes.push("DECLARED_GROUND_NODE_MISMATCH".to_string());
            if resolution.support_state == crate::seed::EndpointSupportState::VerifiedBound {
                reason_codes.push(crate::topology::REASON_EARLY_SURFACE_CONNECTION.to_string());
            }
        }
        if !reason_codes.is_empty() {
            schema_binding_mismatches.push(diagnostic_mismatch(
                binding.ramp_id.clone(),
                format!("osm-ramp-binding:{}", binding.ramp_id),
                "verified_bound".to_string(),
                &resolution,
                Some(binding.osm_node_id),
                reason_codes,
            ));
        }
    }

    let mut ordered_candidates = bindings.binding_candidates.iter().collect::<Vec<_>>();
    ordered_candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    let mut binding_candidate_mismatches = Vec::new();
    for candidate in ordered_candidates {
        let declared_ground_node_id = inventory_by_ramp_id
            .get(candidate.ramp_id.as_str())
            .and_then(|ramp| ramp_flow_direction(ramp.kind))
            .and_then(|flow| {
                candidate
                    .directed_segments
                    .first()
                    .and_then(|segment| match flow {
                        crate::topology::RampFlowDirection::Exit => {
                            segment.osm_node_ids.last().copied()
                        }
                        crate::topology::RampFlowDirection::Entry => {
                            segment.osm_node_ids.first().copied()
                        }
                    })
            });
        let resolution = match audit_first_public_road_connection(candidate, inv, osm_resp) {
            Ok(resolution) => resolution,
            Err(errors) => crate::topology::FirstPublicRoadConnectionResolution {
                rule: crate::topology::FIRST_PUBLIC_ROAD_CONNECTION_RULE.to_string(),
                support_state: crate::seed::EndpointSupportState::Unresolved,
                ground_node_id: None,
                ground_way_id: None,
                ground_way_name: None,
                reason_codes: vec!["CANDIDATE_EVIDENCE_ERROR".to_string()],
                notes: errors,
            },
        };
        let status_matches = (candidate.status == "verified_bound"
            && resolution.support_state == crate::seed::EndpointSupportState::VerifiedBound)
            || (candidate.status != "verified_bound"
                && resolution.support_state == crate::seed::EndpointSupportState::Unresolved);
        let mut reason_codes = Vec::new();
        if !status_matches {
            reason_codes.push("CANDIDATE_STATUS_MISMATCH".to_string());
        }
        let expected_projection = if candidate.status == "verified_bound" {
            "included_verified"
        } else {
            "excluded_unresolved"
        };
        if candidate.public_projection != expected_projection {
            reason_codes.push("PUBLIC_PROJECTION_MISMATCH".to_string());
        }
        if declared_ground_node_id.is_some() && declared_ground_node_id != resolution.ground_node_id
        {
            reason_codes.push("DECLARED_GROUND_NODE_MISMATCH".to_string());
        }
        if candidate.status != "verified_bound"
            && resolution.support_state == crate::seed::EndpointSupportState::Unresolved
            && !resolution.reason_codes.is_empty()
            && resolution.reason_codes != candidate.unresolved_reason_codes
        {
            reason_codes.push("CANDIDATE_REASON_CODES_MISMATCH".to_string());
        }
        if !reason_codes.is_empty() {
            binding_candidate_mismatches.push(diagnostic_mismatch(
                candidate.ramp_id.clone(),
                format!("osm-ramp-binding-candidate:{}", candidate.candidate_id),
                candidate.status.clone(),
                &resolution,
                declared_ground_node_id,
                reason_codes,
            ));
        }
    }

    let schema_binding_total = bindings.bindings.len();
    let schema_binding_mismatched = schema_binding_mismatches.len();
    let binding_candidate_total = bindings.binding_candidates.len();
    let binding_candidate_mismatched = binding_candidate_mismatches.len();
    FirstPublicRoadConnectionDiagnosticReport {
        rule: crate::topology::FIRST_PUBLIC_ROAD_CONNECTION_RULE.to_string(),
        schema_binding_total,
        schema_binding_matched: schema_binding_total - schema_binding_mismatched,
        schema_binding_mismatched,
        schema_binding_mismatches,
        binding_candidate_total,
        binding_candidate_matched: binding_candidate_total - binding_candidate_mismatched,
        binding_candidate_mismatched,
        binding_candidate_mismatches,
    }
}

/// Validates OD tariffs against the canonical inventory.
pub fn validate_od_tariffs(
    tariffs: &OdTariffsFile,
    inv: &RampInventoryFile,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if tariffs.version >= 3 {
        validate_versioned_od_tariffs(tariffs, inv, &mut errors);
    }
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
    graph: &mut Graph,
    inv: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
) -> (Vec<Ramp>, Vec<RampArtifactEntry>, Vec<String>) {
    let mut bound_ramps = Vec::new();
    let mut artifact_entries = Vec::new();
    let mut unbound_notes = Vec::new();
    let binding_map: HashMap<&str, &OsmRampBinding> = bindings
        .bindings
        .iter()
        .map(|binding| (binding.ramp_id.as_str(), binding))
        .collect();
    let verified_candidate_map: HashMap<&str, &OsmRampBindingCandidate> = bindings
        .binding_candidates
        .iter()
        .filter(|candidate| candidate.status == "verified_bound")
        .map(|candidate| (candidate.ramp_id.as_str(), candidate))
        .collect();

    for item in &inv.ramps {
        let is_active_general = item.status == "active"
            && matches!(item.kind, RampKind::GeneralEntry | RampKind::GeneralExit);
        let target_kind = match item.kind {
            RampKind::GeneralEntry => EdgeKind::Entry,
            RampKind::GeneralExit => EdgeKind::Exit,
            _ => EdgeKind::Shutoko,
        };
        let mut projection = None;

        if is_active_general {
            if let Some(binding) = binding_map.get(item.ramp_id.as_str()).copied() {
                let ground_node = format!("n:{}", binding.osm_node_id);
                let motorway_node = format!("n:{}", binding.motorway_node_id);
                let (expected_from, expected_to) = if item.kind == RampKind::GeneralEntry {
                    (ground_node.as_str(), motorway_node.as_str())
                } else {
                    (motorway_node.as_str(), ground_node.as_str())
                };
                if let Some(edge) = graph.edges.iter().find(|edge| {
                    edge.id.split(':').nth(1) == Some(format!("w{}", binding.osm_way_id).as_str())
                        && edge.kind == target_kind
                        && edge.from == expected_from
                        && edge.to == expected_to
                }) {
                    let motorway_node_id =
                        if graph.nodes.iter().any(|node| node.id == motorway_node) {
                            motorway_node
                        } else if item.kind == RampKind::GeneralEntry {
                            edge.to.clone()
                        } else {
                            edge.from.clone()
                        };
                    projection = Some((
                        edge.id.clone(),
                        if item.kind == RampKind::GeneralEntry {
                            edge.from.clone()
                        } else {
                            edge.to.clone()
                        },
                        motorway_node_id,
                    ));
                }
            } else if let Some(candidate) =
                verified_candidate_map.get(item.ramp_id.as_str()).copied()
            {
                if let [segment] = candidate.directed_segments.as_slice() {
                    let ground_node = if item.kind == RampKind::GeneralEntry {
                        segment.from_node_id.as_str()
                    } else {
                        segment.to_node_id.as_str()
                    };
                    let motorway_node = if item.kind == RampKind::GeneralEntry {
                        segment.to_node_id.as_str()
                    } else {
                        segment.from_node_id.as_str()
                    };
                    let edges = segment
                        .edge_ids
                        .iter()
                        .map(|edge_id| {
                            graph
                                .edges
                                .iter()
                                .find(|edge| edge.id == *edge_id)
                                .map(|edge| {
                                    (edge.id.as_str(), edge.from.as_str(), edge.to.as_str())
                                })
                        })
                        .collect::<Option<Vec<_>>>();
                    let valid_edges = edges.as_ref().is_some_and(|edges| {
                        edges
                            .first()
                            .is_some_and(|(_, from, _)| *from == segment.from_node_id)
                            && edges
                                .last()
                                .is_some_and(|(_, _, to)| *to == segment.to_node_id)
                            && edges.windows(2).all(|pair| pair[0].2 == pair[1].1)
                    });
                    if valid_edges {
                        let edge_ids = segment.edge_ids.iter().cloned().collect::<HashSet<_>>();
                        for edge in &mut graph.edges {
                            if edge_ids.contains(&edge.id) {
                                edge.kind = target_kind;
                            }
                        }
                        projection = Some((
                            segment.edge_ids[0].clone(),
                            ground_node.to_string(),
                            motorway_node.to_string(),
                        ));
                    }
                }
            }
        }

        if let Some((edge_id, node_id, mainline_node_id)) = projection {
            bound_ramps.push(Ramp {
                id: item.ramp_id.clone(),
                facility_id: item.facility_id.clone(),
                name: item.facility_name.clone(),
                route: item.route.clone(),
                direction: item.direction.clone(),
                kind: item.kind,
                edge_id: edge_id.clone(),
                node_id: node_id.clone(),
                mainline_node_id: mainline_node_id.clone(),
                restrictions: item.restrictions.clone(),
            });
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
                edge_id: Some(edge_id),
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
    use serde_json::json;
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

    fn load_binding_candidate_fixture() -> (
        RampInventoryFile,
        OsmRampBindingsFile,
        crate::osm::OverpassResponse,
        OsmRampBindingCandidate,
    ) {
        let inv: RampInventoryFile = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/ramp-inventory.json")).unwrap(),
        )
        .unwrap();
        let bindings: OsmRampBindingsFile = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/osm-ramp-bindings.json")).unwrap(),
        )
        .unwrap();
        let osm: crate::osm::OverpassResponse = serde_json::from_str(
            &fs::read_to_string(find_data_file("fixtures/osm/shutoko-all.json")).unwrap(),
        )
        .unwrap();
        let candidate = bindings
            .binding_candidates
            .iter()
            .find(|candidate| candidate.ramp_id == "ramp:2-outbound:tengenji-exit")
            .unwrap()
            .clone();
        (inv, bindings, osm, candidate)
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
        let graph: Graph =
            shutoko_routing_core::prepare_json(&fs::read_to_string(graph_path).unwrap(), "{}")
                .expect("generated graph must pass the schema-aware reader")
                .graph()
                .clone();
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
    fn test_tengenji_multi_way_candidate_is_audited_unresolved() {
        let (inv, bindings, osm, candidate) = load_binding_candidate_fixture();
        assert_eq!(bindings.version, 4);
        assert_eq!(bindings.binding_candidates.len(), 1);
        assert!(validate_osm_ramp_bindings(&bindings, &inv).is_ok());
        assert!(validate_osm_ramp_bindings_against_osm(&bindings, &inv, &osm).is_ok());
        assert!(!bindings
            .bindings
            .iter()
            .any(|binding| binding.ramp_id == "ramp:2-outbound:tengenji-exit"));
        let generated_graph: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/generated/graph.json")).unwrap();
        assert!(generated_graph["ramps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|ramp| ramp["id"] != "ramp:2-outbound:tengenji-exit"));
        let generated_ramps: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/generated/ramps.json")).unwrap();
        let published_tengenji = generated_ramps["ramps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|ramp| ramp["id"] == "ramp:2-outbound:tengenji-exit")
            .unwrap();
        assert_eq!(published_tengenji["supportState"], "unsupported");
        assert_eq!(published_tengenji["bound"], false);
        assert!(published_tengenji["edgeId"].is_null());
        assert_eq!(candidate.status, "unresolved");
        assert_eq!(candidate.public_projection, "excluded_unresolved");
        assert_eq!(candidate.directed_segments.len(), 1);
        let segment = &candidate.directed_segments[0];
        assert_eq!(
            segment.osm_way_ids,
            vec![172358461, 422023171, 931759044, 172358460, 172358466]
        );
        assert_eq!(segment.osm_node_ids.len(), 18);
        assert_eq!(segment.edge_ids.len(), 17);
        assert_eq!(segment.from_node_id, "n:252175582");
        assert_eq!(segment.to_node_id, "n:1832672205");
        assert_eq!(
            segment.edge_ids_sha256,
            "06c4971f3e6f5a72b7eb89fc9c51dd1deed3778cdfb13bef1ae89d84f236f93a"
        );
        assert_eq!(
            audit_osm_ramp_binding_candidate_against_osm(&candidate, &inv, &osm).unwrap(),
            vec![
                "EARLY_SURFACE_CONNECTION".to_string(),
                "MULTIPLE_GROUND_CONNECTION_CANDIDATES".to_string()
            ]
        );

        let official: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/official-population-snapshot.json")).unwrap(),
        )
        .unwrap();
        let mut outbound_exits = official["generalExits"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|exit| {
                exit["route"] == "2"
                    && exit["direction"] == "outbound"
                    && exit["kind"] == "general_exit"
            })
            .map(|exit| {
                (
                    exit["inoutNumber"].as_str().unwrap().to_string(),
                    exit["facilityName"].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        outbound_exits.sort_by_key(|exit| exit.0.parse::<u32>().unwrap());
        assert_eq!(
            outbound_exits,
            vec![
                ("201".to_string(), "天現寺".to_string()),
                ("203".to_string(), "目黒".to_string()),
                ("205".to_string(), "戸越".to_string()),
                ("207".to_string(), "荏原".to_string())
            ]
        );

        let decisions: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/ramp-support-decisions.json")).unwrap(),
        )
        .unwrap();
        let decision = decisions["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|decision| decision["rampId"] == candidate.ramp_id)
            .unwrap();
        assert_eq!(decision["supportState"], "unsupported");
        assert_eq!(
            decision["bindingCandidateEvidence"]["rampIdInverseMap"]["rampId"],
            candidate.ramp_id
        );
        assert_eq!(
            decision["bindingCandidateEvidence"]["rampIdInverseMap"]["candidateId"],
            candidate.candidate_id
        );
        assert_eq!(
            decision["bindingCandidateEvidence"]["officialFacilityOrder"]["exitNumbers"],
            json!(["201", "203", "205", "207"])
        );
        assert_eq!(
            decision["bindingCandidateEvidence"]["unresolvedReasonCodes"],
            json!(candidate.unresolved_reason_codes)
        );
    }

    #[test]
    fn test_verified_multi_way_candidate_validates_and_projects() {
        let (mut inventory, mut bindings, osm, mut candidate) = load_binding_candidate_fixture();
        let segment = &mut candidate.directed_segments[0];
        segment.osm_way_ids.pop();
        segment.osm_node_ids.pop();
        segment.edge_ids.pop();
        segment.to_node_id = "n:1832672162".into();
        segment.edge_ids_sha256 = crate::manifest::compute_sha256(
            serde_json::to_string(&segment.edge_ids)
                .unwrap_or_default()
                .as_bytes(),
        );
        candidate.status = "verified_bound".into();
        candidate.public_projection = "included_verified".into();
        candidate.unresolved_reason.clear();
        candidate.unresolved_reason_codes.clear();
        candidate.route_evidence.ground_way_id = 258834790;
        bindings.binding_candidates = vec![candidate.clone()];

        let ramp = inventory
            .ramps
            .iter_mut()
            .find(|ramp| ramp.ramp_id == candidate.ramp_id)
            .unwrap();
        ramp.support_state = Some("verified_bound".into());
        ramp.support_reason =
            Some("firstPublicRoadConnection/v1が4-way exact bindingを証明した。".into());
        ramp.support_evidence
            .push("fixtures/osm/shutoko-all.json:firstPublicRoadConnection/v1".into());
        ramp.routing_capability = Some("routable".into());
        ramp.routing_capability_reason = Some("有向Shutoko実グラフ上の周回接続を監査済み。".into());

        validate_osm_ramp_bindings(&bindings, &inventory).unwrap();
        validate_osm_ramp_bindings_against_osm(&bindings, &inventory, &osm).unwrap();
        let audit = audit_first_public_road_connection(&candidate, &inventory, &osm).unwrap();
        assert_eq!(
            audit.support_state,
            crate::seed::EndpointSupportState::VerifiedBound
        );
        assert_eq!(audit.ground_node_id, Some(1832672162));
        assert_eq!(audit.ground_way_id, Some(258834790));

        let mut graph: Graph = shutoko_routing_core::prepare_json(
            include_str!("../../../fixtures/generated/graph.json"),
            "{}",
        )
        .unwrap()
        .graph()
        .clone();
        let original_billing_pairs = serde_json::to_value(&graph.billing_pairs).unwrap();
        let (ramps, artifacts, notes) = bind_ramps_to_graph(&mut graph, &inventory, &bindings);
        assert_eq!(ramps.len(), 233);
        assert!(notes.iter().all(|note| !note.contains(&candidate.ramp_id)));
        graph.ramps = ramps;
        let projected = graph
            .ramps
            .iter()
            .find(|ramp| ramp.id == candidate.ramp_id)
            .unwrap();
        assert_eq!(
            projected.edge_id,
            candidate.directed_segments[0].edge_ids[0]
        );
        assert_eq!(projected.node_id, "n:1832672162");
        assert_eq!(projected.mainline_node_id, "n:252175582");
        for edge_id in &candidate.directed_segments[0].edge_ids {
            assert_eq!(
                graph
                    .edges
                    .iter()
                    .find(|edge| edge.id == *edge_id)
                    .unwrap()
                    .kind,
                EdgeKind::Exit
            );
        }
        let artifact = artifacts
            .iter()
            .find(|artifact| artifact.id == candidate.ramp_id)
            .unwrap();
        assert!(artifact.bound);
        assert_eq!(artifact.support_state.as_deref(), Some("verified_bound"));
        assert_eq!(
            serde_json::to_value(&graph.billing_pairs).unwrap(),
            original_billing_pairs
        );

        let evidence = crate::route_membership::bound_ramp_evidence_from_inventory(
            &graph, &inventory, &bindings,
        )
        .unwrap();
        let candidate_evidence = evidence
            .iter()
            .find(|evidence| evidence.ramp_id == candidate.ramp_id)
            .unwrap();
        assert_eq!(
            candidate_evidence.edge_ids,
            candidate.directed_segments[0].edge_ids
        );
        let source_snapshot_sha256 = crate::manifest::compute_sha256(
            include_bytes!("../../../fixtures/osm/shutoko-all.json").as_slice(),
        );
        crate::route_membership::build_route_membership_indices(
            &osm,
            &graph,
            &crate::route_membership::RouteMembershipBuildOptions {
                source_snapshot_sha256,
                relation_ids: Some(vec![4256008, 4256339]),
                bound_ramp_evidence: evidence,
            },
        )
        .unwrap();
        validate_endpoint_capability_contract(&graph, &inventory).unwrap();
    }

    #[test]
    fn test_reject_disconnected_reversed_and_incomplete_binding_candidates() {
        let (inv, _bindings, osm, candidate) = load_binding_candidate_fixture();

        let mut disconnected = candidate.clone();
        disconnected.directed_segments[0].osm_node_ids[6] = 99_999_999;
        assert!(audit_osm_ramp_binding_candidate_against_osm(&disconnected, &inv, &osm).is_err());

        let mut reversed = candidate.clone();
        reversed.directed_segments[0].osm_way_ids.reverse();
        assert!(audit_osm_ramp_binding_candidate_against_osm(&reversed, &inv, &osm).is_err());

        let mut missing_way = candidate;
        missing_way.directed_segments[0].osm_way_ids.remove(2);
        assert!(audit_osm_ramp_binding_candidate_against_osm(&missing_way, &inv, &osm).is_err());
    }

    #[test]
    fn test_reject_ambiguous_internal_branch_and_non_surface_endpoint() {
        let (inv, _bindings, osm, candidate) = load_binding_candidate_fixture();
        let mut osm_value = serde_json::to_value(&osm).unwrap();
        osm_value["elements"].as_array_mut().unwrap().push(json!({
            "type": "node",
            "id": 999999999,
            "lat": 35.646,
            "lon": 139.725
        }));
        osm_value["elements"].as_array_mut().unwrap().push(json!({
            "type": "way",
            "id": 999999998,
            "nodes": [1832672214, 999999999],
            "tags": {
                "highway": "motorway_link",
                "oneway": "yes"
            }
        }));
        let branched_osm: crate::osm::OverpassResponse = serde_json::from_value(osm_value).unwrap();
        let errors = audit_osm_ramp_binding_candidate_against_osm(&candidate, &inv, &branched_osm)
            .unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("ambiguous internal branch")));

        let mut non_surface = candidate;
        let segment = &mut non_surface.directed_segments[0];
        segment.osm_way_ids.truncate(2);
        segment.edge_ids.truncate(9);
        segment.osm_node_ids.truncate(10);
        segment.to_node_id = "n:1832672090".into();
        segment.edge_ids_sha256 = crate::manifest::compute_sha256(
            serde_json::to_string(&segment.edge_ids)
                .unwrap_or_default()
                .as_bytes(),
        );
        let errors =
            audit_osm_ramp_binding_candidate_against_osm(&non_surface, &inv, &osm).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("endpoint 1832672090 is not on declared ground way")));
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
    fn test_tariff_v3_golden_table_and_revision_boundary() {
        let inv: RampInventoryFile = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/ramp-inventory.json")).unwrap(),
        )
        .unwrap();
        let tariffs: OdTariffsFile = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/od-tariffs.json")).unwrap(),
        )
        .unwrap();
        validate_od_tariffs(&tariffs, &inv).unwrap();

        let expected = [
            (
                "c1-outer:kandabashi-takaracho",
                3,
                "神田橋",
                "宝町",
                1_700,
                300,
                "p03:row-c1-kandabashi:column-c1-takaracho:base-etc:300yen:1.7km",
            ),
            (
                "c1-outer:kasumigaseki-daikancho",
                3,
                "霞が関",
                "代官町",
                12_400,
                570,
                "p03:row-c1-kasumigaseki:column-c1-daikancho:base-etc:570yen:12.4km",
            ),
            (
                "c1-outer:ginza-shibakoen",
                3,
                "銀座",
                "芝公園",
                3_400,
                300,
                "p03:row-c1-ginza:column-c1-shibakoen:base-etc:300yen:3.4km",
            ),
            (
                "c1-outer:shibakoen-iikura",
                3,
                "芝公園",
                "飯倉",
                1_600,
                300,
                "p03:row-c1-shibakoen:column-c1-iikura:base-etc:300yen:1.6km",
            ),
            (
                "c1-inner:kasumigaseki-shibakoen",
                3,
                "霞が関",
                "芝公園",
                3_700,
                300,
                "p03:row-c1-kasumigaseki:column-c1-shibakoen:base-etc:300yen:3.7km",
            ),
            (
                "c1-inner:daikancho-kasumigaseki",
                3,
                "代官町",
                "霞が関",
                2_300,
                300,
                "p03:row-c1-daikancho:column-c1-kasumigaseki:base-etc:300yen:2.3km",
            ),
            (
                "c1-inner:shibakoen-shiodome",
                3,
                "芝公園",
                "汐留",
                2_400,
                300,
                "p03:row-c1-shibakoen:column-c1-shiodome:base-etc:300yen:2.4km",
            ),
            (
                "c1-inner:takaracho-kandabashi",
                3,
                "宝町",
                "神田橋",
                1_700,
                300,
                "p03:row-c1-takaracho:column-c1-kandabashi:base-etc:300yen:1.7km",
            ),
            (
                "c1-inner:ginza-shintomicho",
                3,
                "銀座",
                "新富町",
                400,
                300,
                "p03:row-c1-ginza:column-c1-shintomicho:base-etc:300yen:0.4km",
            ),
            (
                "2:meguro-tengenji",
                4,
                "目黒",
                "天現寺",
                19_400,
                790,
                "p04:row-2-meguro:column-2-tengenji:base-etc:790yen:19.4km",
            ),
        ];
        assert_eq!(tariffs.assignments.len(), expected.len());
        for (od_key, page, row, column, distance_meters, amount_yen, cell) in expected {
            let assignment = tariffs
                .assignments
                .iter()
                .find(|assignment| assignment.od_key == od_key)
                .unwrap();
            let evidence = tariffs
                .distance_evidence
                .iter()
                .find(|evidence| evidence.evidence_id == assignment.distance_evidence_id)
                .unwrap();
            let price = assignment
                .prices
                .iter()
                .find(|price| price.status == "priced")
                .unwrap();
            assert_eq!(evidence.page, page);
            assert_eq!(evidence.row_label, row);
            assert_eq!(evidence.column_label, column);
            assert_eq!(evidence.cell, cell);
            assert_eq!(evidence.distance_meters, distance_meters);
            assert_eq!(evidence.observed_base_fare_yen, amount_yen);
            assert_eq!(assignment.billing_distance_meters, distance_meters);
            assert_eq!(price.amount_yen, Some(amount_yen));
            assert_eq!(price.observed_distance_meters, Some(distance_meters));
        }

        let revision = tariffs
            .documents
            .iter()
            .find(|document| document.document_id == "shutoko-2026-10-revision-material")
            .unwrap();
        assert_eq!(
            revision.document_sha256.as_deref(),
            Some("f80126994b3deee36e198f947f3f4f4c3219dd16473bbd9bc9dd296115345702")
        );
        let revised_rule = tariffs
            .tariff_rules
            .iter()
            .find(|rule| rule.rule_id == "shutoko-etc-ordinary-2026-10")
            .unwrap();
        assert_eq!(revised_rule.minimum_distance_meters, Some(3_900));
        assert!(revised_rule.source_refs.iter().any(|source| {
            source.document_id.as_deref() == Some("shutoko-2026-10-revision-material")
        }));
        assert_eq!(calculate_versioned_tariff_yen(3_900, revised_rule), 300);
        assert_eq!(calculate_versioned_tariff_yen(4_000, revised_rule), 310);
    }

    #[test]
    fn test_tariff_v3_rejects_bad_references_and_overlapping_rules() {
        let inv: RampInventoryFile = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/ramp-inventory.json")).unwrap(),
        )
        .unwrap();
        let tariffs: OdTariffsFile = serde_json::from_str(
            &fs::read_to_string(find_data_file("data/od-tariffs.json")).unwrap(),
        )
        .unwrap();

        let mut bad_reference = tariffs.clone();
        bad_reference.assignments[0].prices[0].evidence_id = "evidence:missing".to_string();
        let errors = validate_od_tariffs(&bad_reference, &inv).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("unknown evidence")));

        let mut overlapping = tariffs.clone();
        overlapping.tariff_rules[0].effective_to = None;
        let errors = validate_od_tariffs(&overlapping, &inv).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.contains("overlapping effective intervals")));
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
