use crate::{
    invalid, utc, ArcPolicy, Duration, GeoJsonLineString, Handoff, LatLng, Loop,
    LoopValidationStatus, PairEligibilityStatus, PairKind, RampInfo, RouteAnchor,
    RoutePlanSegmentRole, RoutingError, SnappedOrigin, TariffStatus,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateResolvedRouteSegment {
    pub resolved_segment_id: String,
    pub role: RoutePlanSegmentRole,
    pub membership_id: String,
    pub source_segment_ids: Vec<String>,
    pub edge_ids_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateRoutePlan {
    pub membership_ids: Vec<String>,
    pub resolved_route_segments: Vec<CandidateResolvedRouteSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeRouteLeg {
    pub role: RoutePlanSegmentRole,
    pub resolved_segment_id: String,
    pub start_edge_index: usize,
    pub end_edge_index_exclusive: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstimatedLegRole {
    #[serde(rename = "surface_access")]
    SurfaceAccess,
    #[serde(rename = "surface_return")]
    SurfaceReturn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EstimatedLeg {
    pub role: EstimatedLegRole,
    pub estimated: bool,
    pub distance_meters: u64,
    pub duration_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateV2Toll {
    pub billing_pair_id: String,
    pub amount_yen: Option<u64>,
    pub pricing_at: String,
    pub effective_from: Option<String>,
    pub effective_to: Option<String>,
    pub billing_distance_meters: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toll_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopologyOnlyCandidateKind {
    #[serde(rename = "topologyOnly")]
    TopologyOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TopologyOnlyCandidate {
    pub id: String,
    pub release_id: String,
    pub pair_kind: TopologyOnlyCandidateKind,
    pub origin: Option<LatLng>,
    pub origin_node_id: String,
    pub snapped_origin: SnappedOrigin,
    pub entry: RampInfo,
    pub exit: RampInfo,
    pub entry_id: String,
    pub exit_id: String,
    pub road_names: Vec<String>,
    pub edge_ids: Vec<String>,
    pub geometry: GeoJsonLineString,
    pub estimated_legs: Vec<EstimatedLeg>,
    pub duration: Duration,
    pub distance_meters: u64,
    pub shutoko_distance_meters: u64,
    pub eligibility_status: PairEligibilityStatus,
    pub loop_validation_status: LoopValidationStatus,
    pub tariff_status: TariffStatus,
    pub toll: CandidateV2Toll,
    pub r#loop: Loop,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub handoff: Handoff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateV2Handoff {
    pub enabled: bool,
    pub leg_urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RadialReturnCandidate {
    pub id: String,
    pub release_id: String,
    pub pair_kind: PairKind,
    pub route_plan_version: u8,
    pub origin: Option<LatLng>,
    pub origin_node_id: String,
    pub snapped_origin: SnappedOrigin,
    pub entry: RampInfo,
    pub exit: RampInfo,
    pub entry_id: String,
    pub exit_id: String,
    pub road_names: Vec<String>,
    pub edge_ids: Vec<String>,
    pub geometry: GeoJsonLineString,
    pub anchor: RouteAnchor,
    pub route_plan: CandidateRoutePlan,
    pub edge_route_legs: Vec<EdgeRouteLeg>,
    pub estimated_legs: Vec<EstimatedLeg>,
    pub duration: Duration,
    pub distance_meters: u64,
    pub shutoko_distance_meters: u64,
    pub eligibility_status: PairEligibilityStatus,
    pub loop_validation_status: LoopValidationStatus,
    pub tariff_status: TariffStatus,
    pub toll: CandidateV2Toll,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub handoff: CandidateV2Handoff,
}

pub type RadialCandidate = RadialReturnCandidate;

pub fn validate_radial_return_candidate(
    candidate: &RadialReturnCandidate,
) -> Result<(), RoutingError> {
    if candidate.pair_kind != PairKind::RadialReturn
        || candidate.route_plan_version != 1
        || candidate.id.is_empty()
        || candidate.release_id.is_empty()
        || candidate.origin_node_id.is_empty()
        || candidate.entry_id.is_empty()
        || candidate.exit_id.is_empty()
        || candidate.entry.edge_id != candidate.entry_id
        || candidate.exit.edge_id != candidate.exit_id
        || candidate.edge_ids.is_empty()
        || candidate.edge_ids.len() > 20_000
        || candidate.geometry.r#type != "LineString"
        || candidate.geometry.coordinates.len() != candidate.edge_ids.len() + 1
        || candidate.toll.billing_pair_id.is_empty()
        || candidate.handoff.enabled
        || !candidate.handoff.leg_urls.is_empty()
        || candidate
            .reasons
            .iter()
            .any(|reason| reason == "ONE_SECTION_TOLL")
    {
        return Err(invalid("invalid radialReturn candidate base contract"));
    }
    let RouteAnchor::DirectedJunction(anchor) = &candidate.anchor else {
        return Err(invalid("radialReturn candidate requires directedJunction"));
    };
    if anchor.arc_policy != ArcPolicy::OrdinaryLongArc {
        return Err(invalid("invalid radialReturn candidate anchor"));
    }
    let pricing_at = utc(&candidate.toll.pricing_at)?;
    let effective_from = candidate
        .toll
        .effective_from
        .as_deref()
        .map(utc)
        .transpose()?;
    let effective_to = candidate
        .toll
        .effective_to
        .as_deref()
        .map(utc)
        .transpose()?;
    let interval_valid = effective_from
        .zip(effective_to)
        .is_none_or(|(from, to)| from < to);
    let tariff_consistent = match candidate.tariff_status {
        TariffStatus::Priced => {
            candidate.toll.amount_yen.is_some_and(|amount| amount > 0)
                && effective_from.is_some_and(|from| from <= pricing_at)
                && effective_to.is_none_or(|to| pricing_at < to)
        }
        TariffStatus::Unpriced => {
            candidate.toll.amount_yen.is_none()
                && candidate.toll.billing_distance_meters.is_none()
                && effective_from.is_none()
                && effective_to.is_none()
        }
        TariffStatus::Expired | TariffStatus::NotApplicable => {
            candidate.toll.amount_yen.is_none()
                && effective_from.is_none()
                && effective_to.is_none()
        }
    };
    if !interval_valid || !tariff_consistent {
        return Err(invalid("radialReturn tariff status is inconsistent"));
    }
    let expected_roles = [
        RoutePlanSegmentRole::EntryApproach,
        RoutePlanSegmentRole::MandatoryLap,
        RoutePlanSegmentRole::ReturnCorridor,
        RoutePlanSegmentRole::ExitApproach,
    ];
    if candidate.route_plan.resolved_route_segments.len() != expected_roles.len()
        || candidate.edge_route_legs.len() != expected_roles.len()
    {
        return Err(invalid("radialReturn candidate requires four route legs"));
    }
    let mut membership_ids = HashSet::new();
    for id in &candidate.route_plan.membership_ids {
        if id.is_empty() || !membership_ids.insert(id.as_str()) {
            return Err(invalid("invalid or duplicate candidate membership ID"));
        }
    }
    let mut segment_ids = HashSet::new();
    for (index, segment) in candidate
        .route_plan
        .resolved_route_segments
        .iter()
        .enumerate()
    {
        if segment.role != expected_roles[index]
            || segment.resolved_segment_id.is_empty()
            || !segment_ids.insert(segment.resolved_segment_id.as_str())
            || !membership_ids.contains(segment.membership_id.as_str())
            || segment.source_segment_ids.is_empty()
        {
            return Err(invalid("invalid or duplicate candidate resolved segment"));
        }
        let mut sources = HashSet::new();
        for source in &segment.source_segment_ids {
            if source.is_empty() || !sources.insert(source.as_str()) {
                return Err(invalid("invalid or duplicate candidate source segment"));
            }
        }
        validate_sha256(&segment.edge_ids_sha256)?;
    }
    let mut next_index = 0usize;
    for (index, leg) in candidate.edge_route_legs.iter().enumerate() {
        let segment = &candidate.route_plan.resolved_route_segments[index];
        if leg.role != expected_roles[index]
            || leg.resolved_segment_id != segment.resolved_segment_id
            || leg.start_edge_index != next_index
            || leg.end_edge_index_exclusive <= leg.start_edge_index
            || leg.end_edge_index_exclusive > candidate.edge_ids.len()
        {
            return Err(invalid("candidate edgeRouteLegs overlap or leave a gap"));
        }
        let edges = &candidate.edge_ids[leg.start_edge_index..leg.end_edge_index_exclusive];
        if ordered_edge_ids_sha256(edges).as_deref() != Some(segment.edge_ids_sha256.as_str()) {
            return Err(invalid("candidate edge route leg hash mismatch"));
        }
        next_index = leg.end_edge_index_exclusive;
    }
    if next_index != candidate.edge_ids.len() {
        return Err(invalid("candidate edgeRouteLegs do not cover every edge"));
    }
    validate_estimated_legs(&candidate.estimated_legs)?;
    validate_v2_duration_distance(
        &candidate.estimated_legs,
        &candidate.duration,
        candidate.distance_meters,
        candidate.shutoko_distance_meters,
    )?;
    let product_eligible = candidate.eligibility_status
        == PairEligibilityStatus::VerifiedOneSectionAhead
        && candidate.loop_validation_status == LoopValidationStatus::DeclaredRouteValidated;
    if !product_eligible
        && candidate
            .reasons
            .iter()
            .any(|reason| reason == "BEST_TIME_PER_YEN" || reason == "BEST_SHUTOKO_TIME")
    {
        return Err(invalid(
            "ineligible radial candidate must not be recommended",
        ));
    }
    Ok(())
}

pub fn validate_topology_only_candidate(
    candidate: &TopologyOnlyCandidate,
) -> Result<(), RoutingError> {
    if candidate.pair_kind != TopologyOnlyCandidateKind::TopologyOnly
        || candidate.id.is_empty()
        || candidate.release_id.is_empty()
        || candidate.origin_node_id.is_empty()
        || candidate.entry_id.is_empty()
        || candidate.exit_id.is_empty()
        || candidate.entry.edge_id != candidate.entry_id
        || candidate.exit.edge_id != candidate.exit_id
        || candidate.edge_ids.is_empty()
        || candidate.edge_ids.len() > 20_000
        || candidate.geometry.r#type != "LineString"
        || candidate.geometry.coordinates.len() != candidate.edge_ids.len() + 1
        || candidate.eligibility_status != PairEligibilityStatus::TopologyOnly
        || candidate.loop_validation_status != LoopValidationStatus::TopologyOnly
        || candidate.r#loop.validated
        || candidate
            .reasons
            .iter()
            .any(|reason| reason == "ONE_SECTION_TOLL" || reason.starts_with("BEST_"))
        || !candidate
            .reasons
            .iter()
            .any(|reason| reason == "TOPOLOGY_ONLY")
        || candidate.toll.billing_pair_id.is_empty()
    {
        return Err(invalid("invalid topologyOnly candidate contract"));
    }
    utc(&candidate.toll.pricing_at)?;
    let effective_from = candidate
        .toll
        .effective_from
        .as_deref()
        .map(utc)
        .transpose()?;
    let effective_to = candidate
        .toll
        .effective_to
        .as_deref()
        .map(utc)
        .transpose()?;
    if effective_from
        .zip(effective_to)
        .is_some_and(|(from, to)| to <= from)
    {
        return Err(invalid("topologyOnly tariff interval is invalid"));
    }
    let tariff_consistent = match candidate.tariff_status {
        TariffStatus::Priced => candidate.toll.amount_yen.is_some(),
        TariffStatus::Unpriced | TariffStatus::Expired | TariffStatus::NotApplicable => {
            candidate.toll.amount_yen.is_none()
        }
    };
    if !tariff_consistent {
        return Err(invalid("topologyOnly tariff status is inconsistent"));
    }
    validate_estimated_legs(&candidate.estimated_legs)?;
    validate_v2_duration_distance(
        &candidate.estimated_legs,
        &candidate.duration,
        candidate.distance_meters,
        candidate.shutoko_distance_meters,
    )
}

fn validate_estimated_legs(legs: &[EstimatedLeg]) -> Result<(), RoutingError> {
    if legs.len() != 2
        || legs[0].role != EstimatedLegRole::SurfaceAccess
        || legs[1].role != EstimatedLegRole::SurfaceReturn
        || legs
            .iter()
            .any(|leg| !leg.estimated || (leg.distance_meters == 0) != (leg.duration_seconds == 0))
    {
        return Err(invalid("invalid candidate estimatedLegs"));
    }
    Ok(())
}

fn validate_v2_duration_distance(
    legs: &[EstimatedLeg],
    duration: &Duration,
    distance_meters: u64,
    shutoko_distance_meters: u64,
) -> Result<(), RoutingError> {
    let surface_distance = legs
        .iter()
        .try_fold(0_u64, |total, leg| total.checked_add(leg.distance_meters));
    let base_seconds = duration
        .access_seconds
        .checked_add(duration.shutoko_seconds)
        .and_then(|value| value.checked_add(duration.return_seconds));
    let expected_buffer = base_seconds.map(|base| 300.max(base.div_ceil(5)));
    let plan_seconds = base_seconds.and_then(|value| value.checked_add(duration.buffer_seconds));
    if surface_distance.and_then(|value| value.checked_add(shutoko_distance_meters))
        != Some(distance_meters)
        || duration.access_seconds != legs[0].duration_seconds
        || duration.return_seconds != legs[1].duration_seconds
        || base_seconds != Some(duration.base_seconds)
        || expected_buffer != Some(duration.buffer_seconds)
        || plan_seconds != Some(duration.plan_seconds)
    {
        return Err(invalid(
            "candidate duration or distance totals are inconsistent",
        ));
    }
    Ok(())
}

fn ordered_edge_ids_sha256(edge_ids: &[String]) -> Option<String> {
    let bytes = serde_json::to_vec(edge_ids).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Some(format!("{:x}", hasher.finalize()))
}

fn validate_sha256(value: &str) -> Result<(), RoutingError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid("candidate route hash must be lowercase SHA-256"));
    }
    Ok(())
}
