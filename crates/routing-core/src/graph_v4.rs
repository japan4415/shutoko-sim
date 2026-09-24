use crate::{
    invalid, utc, BillingPair, EdgeKind, Graph, Price, RampKind, RoutingError, VerificationStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub const ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION: &str = "osm-relation-role/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PairKind {
    LegacyRing,
    RadialReturn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorKind {
    SameNode,
    DirectedJunction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArcPolicy {
    SameNodeLoop,
    OrdinaryLongArc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointSupportState {
    #[serde(rename = "verified_bound")]
    VerifiedBound,
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "unresolved")]
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutingCapability {
    #[serde(rename = "routable")]
    Routable,
    #[serde(rename = "structural_no_loop")]
    StructuralNoLoop,
    #[serde(rename = "unsupported")]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairEligibilityStatus {
    #[serde(rename = "verified_one_section_ahead")]
    VerifiedOneSectionAhead,
    #[serde(rename = "unverified")]
    Unverified,
    #[serde(rename = "topology_only")]
    TopologyOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopValidationStatus {
    #[serde(rename = "declared_route_validated")]
    DeclaredRouteValidated,
    #[serde(rename = "unresolved")]
    Unresolved,
    #[serde(rename = "topology_only")]
    TopologyOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TariffStatus {
    #[serde(rename = "priced")]
    Priced,
    #[serde(rename = "unpriced")]
    Unpriced,
    #[serde(rename = "expired")]
    Expired,
    #[serde(rename = "not_applicable")]
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteMembershipSourceKind {
    RelationMainline,
    BoundRamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteMembershipIndex {
    pub membership_id: String,
    pub route_id: String,
    pub direction: String,
    pub direction_mapping_version: String,
    pub segments: Vec<RouteMembershipSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteMembershipSegment {
    pub segment_id: String,
    pub source_kind: RouteMembershipSourceKind,
    pub source_relation_id: Option<String>,
    pub source_snapshot_sha256: String,
    pub binding_evidence_id: Option<String>,
    pub ordered_edge_ids: Vec<String>,
    pub ordered_edge_ids_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairEligibility {
    pub status: PairEligibilityStatus,
    pub one_section_ahead_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoopValidation {
    pub status: LoopValidationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tariff {
    pub status: TariffStatus,
    pub amount_yen: Option<u64>,
    pub billing_distance_meters: Option<u64>,
    pub prices: Vec<Price>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SameNodeAnchor {
    pub node_id: String,
    pub route_id: String,
    pub direction: String,
    pub arc_policy: ArcPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExcludedShortConnector {
    pub from_node_id: String,
    pub to_node_id: String,
    pub osm_way_id: i64,
    pub edge_count: u16,
    pub distance_meters: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectedJunctionAnchor {
    pub merge_node_id: String,
    pub branch_node_id: String,
    pub merge_terminal_edge_id: String,
    pub branch_initial_edge_id: String,
    pub route_id: String,
    pub direction: String,
    pub arc_policy: ArcPolicy,
    pub excluded_short_connector: ExcludedShortConnector,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "anchorKind", rename_all = "camelCase")]
pub enum RouteAnchor {
    SameNode(SameNodeAnchor),
    DirectedJunction(DirectedJunctionAnchor),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryCorridor {
    pub membership_id: String,
    pub terminal_edge_id: String,
    pub merge_node_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MandatoryLap {
    pub membership_id: String,
    pub first_edge_id: String,
    pub last_edge_id: String,
    pub lap_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FirstGeneralExit {
    pub rule: String,
    pub expected_ramp_id: String,
    pub exact_directed_binding: EndpointSupportState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReturnCorridor {
    pub membership_id: String,
    pub start_node_id: String,
    pub initial_edge_id: String,
    pub first_general_exit: FirstGeneralExit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutePlanV1 {
    pub entry_corridor: EntryCorridor,
    pub anchor: RouteAnchor,
    pub mandatory_lap: MandatoryLap,
    pub return_corridor: ReturnCorridor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutePlanSegmentRole {
    #[serde(rename = "entry_approach")]
    EntryApproach,
    #[serde(rename = "mandatory_lap")]
    MandatoryLap,
    #[serde(rename = "return_corridor")]
    ReturnCorridor,
    #[serde(rename = "exit_approach")]
    ExitApproach,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedRouteSegment {
    pub resolved_segment_id: String,
    pub role: RoutePlanSegmentRole,
    pub membership_id: String,
    pub source_segment_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub edge_ids_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectedEndpointSegment {
    pub segment_id: String,
    pub osm_way_ids: Vec<i64>,
    pub edge_ids: Vec<String>,
    pub from_node_id: String,
    pub to_node_id: String,
    pub edge_ids_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingCandidate {
    pub candidate_id: String,
    pub status: EndpointSupportState,
    pub directed_segments: Vec<DirectedEndpointSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingEndpoint {
    pub ramp_id: String,
    pub name: String,
    pub support_state: EndpointSupportState,
    pub directed_segments: Vec<DirectedEndpointSegment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binding_candidates: Vec<BindingCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyRingBillingPair {
    pub id: String,
    pub pair_kind: PairKind,
    pub vehicle_profile: String,
    pub entry_id: String,
    pub exit_id: String,
    pub anchor: RouteAnchor,
    pub entry_to_anchor_edge_ids: Vec<String>,
    pub anchor_to_exit_edge_ids: Vec<String>,
    pub pair_eligibility: PairEligibility,
    pub loop_validation: LoopValidation,
    pub tariff: Tariff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RadialReturnBillingPair {
    pub id: String,
    pub pair_kind: PairKind,
    pub route_plan_version: u8,
    pub vehicle_profile: String,
    pub entry_id: String,
    pub exit_id: String,
    pub entry_endpoint: BillingEndpoint,
    pub exit_endpoint: BillingEndpoint,
    pub route_plan: RoutePlanV1,
    pub resolved_route_segments: Vec<ResolvedRouteSegment>,
    pub routing_capability: RoutingCapability,
    pub pair_eligibility: PairEligibility,
    pub loop_validation: LoopValidation,
    pub tariff: Tariff,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedGraph {
    pub(crate) graph: Graph,
    pub(crate) radial_billing_pairs: Vec<RadialReturnBillingPair>,
    pub(crate) route_memberships: Vec<RouteMembershipIndex>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GraphEnvelope {
    schema_version: u32,
    release_id: String,
    vehicle_profile: String,
    nodes: Vec<crate::Node>,
    edges: Vec<crate::Edge>,
    billing_pairs: Vec<Value>,
    #[serde(default)]
    forbidden_transitions: Vec<Vec<String>>,
    #[serde(default)]
    ramps: Vec<crate::Ramp>,
    #[serde(default)]
    od_tariffs: Vec<crate::OdTariff>,
    #[serde(default)]
    route_memberships: Option<Vec<RouteMembershipIndex>>,
}

pub(crate) fn read_graph_json(input: &str) -> Result<ParsedGraph, RoutingError> {
    const MAX_GRAPH_JSON_BYTES: usize = 512 * 1024 * 1024;
    if input.len() > MAX_GRAPH_JSON_BYTES {
        return Err(invalid("JSON payload exceeds prototype size limit"));
    }
    let value: Value = serde_json::from_str(input).map_err(|_| invalid("invalid graph JSON"))?;
    let schema_version = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| invalid("unsupported graph schema version"))?;
    if !(2..=4).contains(&schema_version) {
        return Err(invalid("unsupported graph schema version"));
    }
    let has_route_memberships = value.get("routeMemberships").is_some();
    let envelope: GraphEnvelope =
        serde_json::from_value(value).map_err(|_| invalid("invalid graph JSON"))?;
    if schema_version != 4 && has_route_memberships {
        return Err(invalid("routeMemberships requires graph schema 4"));
    }
    let mut billing_pairs = Vec::with_capacity(envelope.billing_pairs.len());
    let mut schema4_legacy_pairs = Vec::new();
    let mut radial_billing_pairs = Vec::new();
    for value in envelope.billing_pairs {
        match schema_version {
            2 | 3 => billing_pairs.push(
                serde_json::from_value(value)
                    .map_err(|_| invalid("invalid legacy billing pair"))?,
            ),
            4 => {
                let pair_kind = value
                    .get("pairKind")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("schema 4 billing pair requires pairKind"))?;
                match pair_kind {
                    "legacyRing" => {
                        let pair: LegacyRingBillingPair = serde_json::from_value(value)
                            .map_err(|_| invalid("invalid legacyRing billing pair"))?;
                        billing_pairs.push(pair.to_legacy()?);
                        schema4_legacy_pairs.push(pair);
                    }
                    "radialReturn" => {
                        let pair: RadialReturnBillingPair = serde_json::from_value(value)
                            .map_err(|_| invalid("invalid radialReturn billing pair"))?;
                        if pair.pair_kind != PairKind::RadialReturn {
                            return Err(invalid("radialReturn billing pair kind mismatch"));
                        }
                        radial_billing_pairs.push(pair);
                    }
                    _ => return Err(invalid("unsupported graph billing pair kind")),
                }
            }
            _ => unreachable!(),
        }
    }
    let mut radial_ids = HashSet::new();
    for pair in &radial_billing_pairs {
        if !radial_ids.insert(pair.id.as_str()) {
            return Err(invalid("duplicate radialReturn billing pair id"));
        }
    }
    let graph = Graph {
        schema_version: envelope.schema_version,
        release_id: envelope.release_id,
        vehicle_profile: envelope.vehicle_profile,
        nodes: envelope.nodes,
        edges: envelope.edges,
        billing_pairs,
        forbidden_transitions: envelope.forbidden_transitions,
        ramps: envelope.ramps,
        od_tariffs: envelope.od_tariffs,
    };
    let route_memberships = match schema_version {
        4 => envelope
            .route_memberships
            .ok_or_else(|| invalid("schema 4 graph requires routeMemberships"))?,
        _ => Vec::new(),
    };
    validate_route_memberships(&graph, &route_memberships)?;
    for (wire, normalized) in schema4_legacy_pairs.iter().zip(&graph.billing_pairs) {
        validate_schema4_legacy_pair(&graph, &route_memberships, wire, normalized)?;
    }
    for pair in &radial_billing_pairs {
        validate_radial_return_pair(&graph, &route_memberships, pair)?;
    }
    Ok(ParsedGraph {
        graph,
        radial_billing_pairs,
        route_memberships,
    })
}

impl LegacyRingBillingPair {
    pub fn from_legacy(
        pair: &BillingPair,
        graph: &Graph,
        memberships: &[RouteMembershipIndex],
    ) -> Result<Self, RoutingError> {
        let membership = infer_legacy_membership(pair, graph, memberships)?;
        let (status, one_section_ahead_verified) = match pair.status {
            VerificationStatus::Verified => (PairEligibilityStatus::VerifiedOneSectionAhead, true),
            VerificationStatus::Unverified => (PairEligibilityStatus::Unverified, false),
        };
        let tariff_status = if pair.prices.is_empty() {
            TariffStatus::Unpriced
        } else {
            TariffStatus::Priced
        };
        Ok(Self {
            id: pair.id.clone(),
            pair_kind: PairKind::LegacyRing,
            vehicle_profile: pair.vehicle_profile.clone(),
            entry_id: pair.entry_id.clone(),
            exit_id: pair.exit_id.clone(),
            anchor: RouteAnchor::SameNode(SameNodeAnchor {
                node_id: pair.anchor_node_id.clone(),
                route_id: membership.route_id.clone(),
                direction: membership.direction.clone(),
                arc_policy: ArcPolicy::SameNodeLoop,
            }),
            entry_to_anchor_edge_ids: pair.entry_to_anchor_edge_ids.clone(),
            anchor_to_exit_edge_ids: pair.anchor_to_exit_edge_ids.clone(),
            pair_eligibility: PairEligibility {
                status,
                one_section_ahead_verified,
            },
            loop_validation: LoopValidation {
                status: LoopValidationStatus::DeclaredRouteValidated,
            },
            tariff: Tariff {
                status: tariff_status,
                amount_yen: pair.prices.first().map(|price| price.amount_yen),
                billing_distance_meters: pair.billing_distance_meters,
                prices: pair.prices.clone(),
            },
        })
    }

    fn to_legacy(&self) -> Result<BillingPair, RoutingError> {
        let RouteAnchor::SameNode(anchor) = &self.anchor else {
            return Err(invalid("legacyRing requires sameNode anchor"));
        };
        let eligibility_consistent = matches!(
            (
                self.pair_eligibility.status,
                self.pair_eligibility.one_section_ahead_verified
            ),
            (PairEligibilityStatus::VerifiedOneSectionAhead, true)
                | (PairEligibilityStatus::Unverified, false)
                | (PairEligibilityStatus::TopologyOnly, false)
        );
        let tariff_consistent = match self.tariff.status {
            TariffStatus::Priced => {
                !self.tariff.prices.is_empty() && self.tariff.amount_yen.is_some()
            }
            TariffStatus::Unpriced => {
                self.tariff.prices.is_empty() && self.tariff.amount_yen.is_none()
            }
            TariffStatus::Expired | TariffStatus::NotApplicable => true,
        };
        if self.pair_kind != PairKind::LegacyRing
            || anchor.arc_policy != ArcPolicy::SameNodeLoop
            || self.id.is_empty()
            || self.vehicle_profile.is_empty()
            || self.entry_id.is_empty()
            || self.exit_id.is_empty()
            || anchor.node_id.is_empty()
            || anchor.route_id.is_empty()
            || anchor.direction.is_empty()
            || self.entry_to_anchor_edge_ids.is_empty()
            || self.anchor_to_exit_edge_ids.is_empty()
            || !eligibility_consistent
            || self.loop_validation.status != LoopValidationStatus::DeclaredRouteValidated
            || !tariff_consistent
        {
            return Err(invalid("invalid legacyRing billing pair contract"));
        }
        Ok(BillingPair {
            id: self.id.clone(),
            entry_id: self.entry_id.clone(),
            exit_id: self.exit_id.clone(),
            anchor_node_id: anchor.node_id.clone(),
            entry_to_anchor_edge_ids: self.entry_to_anchor_edge_ids.clone(),
            anchor_to_exit_edge_ids: self.anchor_to_exit_edge_ids.clone(),
            status: match self.pair_eligibility.status {
                PairEligibilityStatus::VerifiedOneSectionAhead => VerificationStatus::Verified,
                PairEligibilityStatus::Unverified | PairEligibilityStatus::TopologyOnly => {
                    VerificationStatus::Unverified
                }
            },
            vehicle_profile: self.vehicle_profile.clone(),
            prices: self.tariff.prices.clone(),
            entry_name: None,
            exit_name: None,
            entry_ramp_id: None,
            exit_ramp_id: None,
            billing_distance_meters: self.tariff.billing_distance_meters,
        })
    }
}

fn infer_legacy_membership<'a>(
    pair: &BillingPair,
    graph: &Graph,
    memberships: &'a [RouteMembershipIndex],
) -> Result<&'a RouteMembershipIndex, RoutingError> {
    let required = pair
        .entry_to_anchor_edge_ids
        .iter()
        .skip(1)
        .chain(
            pair.anchor_to_exit_edge_ids
                .iter()
                .take(pair.anchor_to_exit_edge_ids.len().saturating_sub(1)),
        )
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if required.is_empty() {
        return Err(invalid("legacy billing pair has no mainline edge sequence"));
    }
    let declared_entry = pair.entry_ramp_id.as_deref().and_then(|id| {
        graph
            .ramps
            .iter()
            .find(|ramp| ramp.id == id)
            .map(|ramp| (ramp.route.as_str(), ramp.direction.as_str()))
    });
    let scored = memberships
        .iter()
        .filter(|membership| {
            !declared_entry.is_some_and(|(route, direction)| {
                membership.route_id != route || membership.direction != direction
            })
        })
        .map(|membership| {
            let available = membership
                .segments
                .iter()
                .filter(|segment| {
                    segment.source_kind == RouteMembershipSourceKind::RelationMainline
                })
                .flat_map(|segment| segment.ordered_edge_ids.iter().map(String::as_str))
                .collect::<HashSet<_>>();
            let overlap = required
                .iter()
                .filter(|edge_id| available.contains(*edge_id))
                .count();
            (membership, overlap)
        })
        .filter(|(_, overlap)| *overlap > 0)
        .collect::<Vec<_>>();
    let best_overlap = scored
        .iter()
        .map(|(_, overlap)| *overlap)
        .max()
        .unwrap_or(0);
    let matches = scored
        .into_iter()
        .filter(|(_, overlap)| *overlap == best_overlap)
        .map(|(membership, _)| membership)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(invalid(format!(
            "legacy billing pair {} must resolve to exactly one mainline membership (matches={})",
            pair.id,
            matches.len()
        )));
    }
    Ok(matches[0])
}

fn validate_schema4_legacy_pair(
    graph: &Graph,
    memberships: &[RouteMembershipIndex],
    wire: &LegacyRingBillingPair,
    normalized: &BillingPair,
) -> Result<(), RoutingError> {
    let RouteAnchor::SameNode(anchor) = &wire.anchor else {
        return Err(invalid("legacyRing requires sameNode anchor"));
    };
    if wire.id != normalized.id
        || wire.entry_id != normalized.entry_id
        || wire.exit_id != normalized.exit_id
        || anchor.node_id != normalized.anchor_node_id
        || wire.vehicle_profile != graph.vehicle_profile
    {
        return Err(invalid("legacyRing normalized identity mismatch"));
    }
    let required = wire
        .entry_to_anchor_edge_ids
        .iter()
        .skip(1)
        .chain(
            wire.anchor_to_exit_edge_ids
                .iter()
                .take(wire.anchor_to_exit_edge_ids.len().saturating_sub(1)),
        )
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let scored = memberships
        .iter()
        .filter(|membership| {
            membership.route_id == anchor.route_id && membership.direction == anchor.direction
        })
        .map(|membership| {
            let available = membership
                .segments
                .iter()
                .filter(|segment| {
                    segment.source_kind == RouteMembershipSourceKind::RelationMainline
                })
                .flat_map(|segment| segment.ordered_edge_ids.iter().map(String::as_str))
                .collect::<HashSet<_>>();
            (
                membership,
                required
                    .iter()
                    .filter(|edge_id| available.contains(*edge_id))
                    .count(),
            )
        })
        .filter(|(_, overlap)| *overlap > 0)
        .collect::<Vec<_>>();
    let best = scored.iter().map(|(_, overlap)| *overlap).max();
    if best.is_none()
        || scored
            .iter()
            .filter(|(_, overlap)| Some(*overlap) == best)
            .count()
            != 1
    {
        return Err(invalid(format!(
            "legacyRing {} anchor route/direction does not resolve uniquely",
            wire.id
        )));
    }
    Ok(())
}

pub(crate) fn validate_route_memberships(
    graph: &Graph,
    memberships: &[RouteMembershipIndex],
) -> Result<(), RoutingError> {
    let edge_map = graph
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let mut membership_ids = HashSet::new();
    let mut segment_ids = HashSet::new();
    let mut source_snapshot = None::<&str>;
    for membership in memberships {
        valid_id(&membership.membership_id, "membershipId")?;
        valid_id(&membership.route_id, "routeId")?;
        valid_id(&membership.direction, "direction")?;
        if !membership_ids.insert(membership.membership_id.as_str())
            || membership.membership_id
                != format!("route:{}:{}", membership.route_id, membership.direction)
            || membership.direction_mapping_version != ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION
            || membership.segments.is_empty()
        {
            return Err(invalid("invalid or duplicate route membership"));
        }
        for segment in &membership.segments {
            valid_id(&segment.segment_id, "segmentId")?;
            if !segment_ids.insert(segment.segment_id.as_str())
                || segment.ordered_edge_ids.is_empty()
                || segment.ordered_edge_ids.len() > 2000
            {
                return Err(invalid("invalid or duplicate route membership segment"));
            }
            validate_sha256(&segment.source_snapshot_sha256, "sourceSnapshotSha256")?;
            validate_sha256(&segment.ordered_edge_ids_sha256, "orderedEdgeIdsSha256")?;
            if ordered_edge_ids_sha256(&segment.ordered_edge_ids)
                .ok()
                .as_deref()
                != Some(segment.ordered_edge_ids_sha256.as_str())
            {
                return Err(invalid("route membership edge hash mismatch"));
            }
            let snapshot = source_snapshot.get_or_insert(&segment.source_snapshot_sha256);
            if *snapshot != segment.source_snapshot_sha256 {
                return Err(invalid("route memberships use different source snapshots"));
            }
            let bound = match segment.source_kind {
                RouteMembershipSourceKind::RelationMainline => {
                    segment
                        .source_relation_id
                        .as_deref()
                        .is_some_and(valid_id_value)
                        && segment.binding_evidence_id.is_none()
                }
                RouteMembershipSourceKind::BoundRamp => {
                    segment.source_relation_id.is_none()
                        && segment
                            .binding_evidence_id
                            .as_deref()
                            .is_some_and(valid_id_value)
                }
            };
            if !bound {
                return Err(invalid("invalid route membership source binding"));
            }
            let mut edge_ids = HashSet::new();
            let mut previous_to = None::<&str>;
            for edge_id in &segment.ordered_edge_ids {
                valid_id(edge_id, "ordered edge id")?;
                if !edge_ids.insert(edge_id.as_str()) {
                    return Err(invalid("route membership segment repeats an edge"));
                }
                let edge = edge_map
                    .get(edge_id.as_str())
                    .ok_or_else(|| invalid("route membership references unknown edge"))?;
                if previous_to.is_some_and(|node| node != edge.from.as_str()) {
                    return Err(invalid("route membership segment is disconnected"));
                }
                if segment.source_kind == RouteMembershipSourceKind::RelationMainline
                    && edge.kind != EdgeKind::Shutoko
                {
                    return Err(invalid("relationMainline contains a non-mainline edge"));
                }
                if segment.source_kind == RouteMembershipSourceKind::BoundRamp
                    && !matches!(edge.kind, EdgeKind::Entry | EdgeKind::Exit)
                {
                    return Err(invalid("boundRamp contains a non-ramp edge"));
                }
                previous_to = Some(&edge.to);
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_radial_return_pair(
    graph: &Graph,
    memberships: &[RouteMembershipIndex],
    pair: &RadialReturnBillingPair,
) -> Result<(), RoutingError> {
    valid_id(&pair.id, "radial billing pair id")?;
    if pair.vehicle_profile != graph.vehicle_profile
        || pair.route_plan_version != 1
        || pair.pair_kind != PairKind::RadialReturn
        || graph
            .billing_pairs
            .iter()
            .any(|existing| existing.id == pair.id)
        || memberships.len() > 20_000
    {
        return Err(invalid("invalid radialReturn billing pair identity"));
    }
    validate_prices(&pair.tariff.prices)?;
    if matches!(pair.tariff.status, TariffStatus::Priced)
        && (pair.tariff.prices.is_empty() || pair.tariff.amount_yen.is_none())
        || matches!(pair.tariff.status, TariffStatus::Unpriced)
            && (!pair.tariff.prices.is_empty()
                || pair.tariff.amount_yen.is_some()
                || pair.tariff.billing_distance_meters.is_some())
    {
        return Err(invalid("radialReturn tariff status is inconsistent"));
    }
    validate_endpoint(
        graph,
        &pair.entry_endpoint,
        &pair.entry_id,
        EdgeKind::Entry,
        RampKind::GeneralEntry,
    )?;
    validate_endpoint(
        graph,
        &pair.exit_endpoint,
        &pair.exit_id,
        EdgeKind::Exit,
        RampKind::GeneralExit,
    )?;
    let RouteAnchor::DirectedJunction(anchor) = &pair.route_plan.anchor else {
        return Err(invalid("radialReturn requires directedJunction anchor"));
    };
    if anchor.arc_policy != ArcPolicy::OrdinaryLongArc
        || anchor.merge_node_id == anchor.branch_node_id
        || anchor.route_id.is_empty()
        || anchor.direction.is_empty()
        || anchor.excluded_short_connector.from_node_id != anchor.branch_node_id
        || anchor.excluded_short_connector.to_node_id != anchor.merge_node_id
        || anchor.excluded_short_connector.osm_way_id <= 0
        || anchor.excluded_short_connector.edge_count == 0
        || anchor.excluded_short_connector.distance_meters == 0
    {
        return Err(invalid("invalid radialReturn directedJunction anchor"));
    }
    let membership_map = memberships
        .iter()
        .map(|membership| (membership.membership_id.as_str(), membership))
        .collect::<HashMap<_, _>>();
    let entry_membership = membership_map
        .get(pair.route_plan.entry_corridor.membership_id.as_str())
        .ok_or_else(|| invalid("unknown entry corridor membership"))?;
    let lap_membership = membership_map
        .get(pair.route_plan.mandatory_lap.membership_id.as_str())
        .ok_or_else(|| invalid("unknown mandatory lap membership"))?;
    let return_membership = membership_map
        .get(pair.route_plan.return_corridor.membership_id.as_str())
        .ok_or_else(|| invalid("unknown return corridor membership"))?;
    if lap_membership.route_id != anchor.route_id
        || lap_membership.direction != anchor.direction
        || pair.route_plan.mandatory_lap.lap_count != 1
        || pair.route_plan.return_corridor.first_general_exit.rule != "firstGeneralExit"
        || pair
            .route_plan
            .return_corridor
            .first_general_exit
            .expected_ramp_id
            != pair.exit_endpoint.ramp_id
        || pair
            .route_plan
            .return_corridor
            .first_general_exit
            .exact_directed_binding
            != EndpointSupportState::VerifiedBound
    {
        return Err(invalid("radialReturn route plan status mismatch"));
    }
    let edge_map = graph
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let entry_terminal = edge_map
        .get(pair.route_plan.entry_corridor.terminal_edge_id.as_str())
        .ok_or_else(|| invalid("unknown entry corridor terminal edge"))?;
    let merge_terminal = edge_map
        .get(anchor.merge_terminal_edge_id.as_str())
        .ok_or_else(|| invalid("unknown merge terminal edge"))?;
    let branch_initial = edge_map
        .get(anchor.branch_initial_edge_id.as_str())
        .ok_or_else(|| invalid("unknown branch initial edge"))?;
    if entry_terminal.to != anchor.merge_node_id
        || pair.route_plan.entry_corridor.merge_node_id != anchor.merge_node_id
        || pair.route_plan.entry_corridor.terminal_edge_id != anchor.merge_terminal_edge_id
        || merge_terminal.to != anchor.merge_node_id
        || branch_initial.from != anchor.branch_node_id
        || pair.route_plan.return_corridor.start_node_id != anchor.branch_node_id
        || pair.route_plan.return_corridor.initial_edge_id != anchor.branch_initial_edge_id
    {
        return Err(invalid("radialReturn M/B boundary mismatch"));
    }
    let expected_roles = [
        RoutePlanSegmentRole::EntryApproach,
        RoutePlanSegmentRole::MandatoryLap,
        RoutePlanSegmentRole::ReturnCorridor,
        RoutePlanSegmentRole::ExitApproach,
    ];
    if pair.resolved_route_segments.len() != expected_roles.len() {
        return Err(invalid(
            "radialReturn requires four resolved route segments",
        ));
    }
    let mut resolved_ids = HashSet::new();
    let mut full_edge_ids = Vec::new();
    for (index, segment) in pair.resolved_route_segments.iter().enumerate() {
        valid_id(&segment.resolved_segment_id, "resolvedSegmentId")?;
        valid_id(&segment.membership_id, "resolved membershipId")?;
        if segment.role != expected_roles[index]
            || !resolved_ids.insert(segment.resolved_segment_id.as_str())
            || segment.source_segment_ids.is_empty()
        {
            return Err(invalid("invalid or duplicate resolved route segment"));
        }
        let membership = membership_map
            .get(segment.membership_id.as_str())
            .ok_or_else(|| invalid("resolved route segment references unknown membership"))?;
        let source_ids = membership
            .segments
            .iter()
            .map(|source| source.segment_id.as_str())
            .collect::<HashSet<_>>();
        let mut sources = HashSet::new();
        for source in &segment.source_segment_ids {
            if !source_ids.contains(source.as_str()) || !sources.insert(source.as_str()) {
                return Err(invalid("resolved route segment source binding mismatch"));
            }
        }
        validate_resolved_segment_sources(membership, segment)?;
        validate_sha256(&segment.edge_ids_sha256, "edgeIdsSha256")?;
        if ordered_edge_ids_sha256(&segment.edge_ids).ok().as_deref()
            != Some(segment.edge_ids_sha256.as_str())
        {
            return Err(invalid("resolved route segment edge hash mismatch"));
        }
        validate_ordered_graph_edges(
            &edge_map,
            &segment.edge_ids,
            &segment.resolved_segment_id,
            true,
        )?;
        full_edge_ids.extend(segment.edge_ids.iter().cloned());
    }
    if full_edge_ids.first() != Some(&pair.entry_id) || full_edge_ids.last() != Some(&pair.exit_id)
    {
        return Err(invalid(
            "resolved route segments do not bind entry and exit",
        ));
    }
    let entry = &pair.resolved_route_segments[0];
    let lap = &pair.resolved_route_segments[1];
    let return_segment = &pair.resolved_route_segments[2];
    let exit = &pair.resolved_route_segments[3];
    let entry_edges = ordered_edges(&edge_map, &entry.edge_ids)?;
    let lap_edges = ordered_edges(&edge_map, &lap.edge_ids)?;
    let return_edges = ordered_edges(&edge_map, &return_segment.edge_ids)?;
    let exit_edges = ordered_edges(&edge_map, &exit.edge_ids)?;
    if entry.membership_id != entry_membership.membership_id
        || lap.membership_id != lap_membership.membership_id
        || return_segment.membership_id != return_membership.membership_id
        || exit.membership_id != return_membership.membership_id
        || entry_edges.last().map(|edge| edge.to.as_str()) != Some(anchor.merge_node_id.as_str())
        || lap_edges.first().map(|edge| edge.from.as_str()) != Some(anchor.merge_node_id.as_str())
        || lap_edges.last().map(|edge| edge.to.as_str()) != Some(anchor.branch_node_id.as_str())
        || return_edges.first().map(|edge| edge.from.as_str())
            != Some(anchor.branch_node_id.as_str())
        || !entry_edges
            .iter()
            .any(|edge| edge.id == anchor.merge_terminal_edge_id)
        || !return_edges
            .iter()
            .any(|edge| edge.id == anchor.branch_initial_edge_id)
        || lap_edges.first().map(|edge| edge.id.as_str())
            != Some(pair.route_plan.mandatory_lap.first_edge_id.as_str())
        || lap_edges.last().map(|edge| edge.id.as_str())
            != Some(pair.route_plan.mandatory_lap.last_edge_id.as_str())
        || exit_edges.last().map(|edge| edge.kind) != Some(EdgeKind::Exit)
    {
        return Err(invalid("radialReturn resolved segment boundary mismatch"));
    }
    validate_endpoint_membership_binding(memberships, &pair.entry_endpoint, entry)?;
    validate_endpoint_membership_binding(memberships, &pair.exit_endpoint, exit)?;
    validate_ordered_graph_edges(&edge_map, &full_edge_ids, "radialReturn full route", false)?;
    Ok(())
}

fn validate_resolved_segment_sources(
    membership: &RouteMembershipIndex,
    resolved: &ResolvedRouteSegment,
) -> Result<(), RoutingError> {
    let mut edge_ids = Vec::new();
    for source_id in &resolved.source_segment_ids {
        let source = membership
            .segments
            .iter()
            .find(|segment| &segment.segment_id == source_id)
            .ok_or_else(|| invalid("resolved route source segment is missing"))?;
        edge_ids.extend(source.ordered_edge_ids.iter().cloned());
    }
    if edge_ids != resolved.edge_ids {
        return Err(invalid(
            "resolved route source segments do not match edgeIds",
        ));
    }
    Ok(())
}

fn validate_endpoint_membership_binding(
    memberships: &[RouteMembershipIndex],
    endpoint: &BillingEndpoint,
    resolved: &ResolvedRouteSegment,
) -> Result<(), RoutingError> {
    let membership = memberships
        .iter()
        .find(|membership| membership.membership_id == resolved.membership_id)
        .ok_or_else(|| invalid("resolved endpoint membership is missing"))?;
    for endpoint_segment in &endpoint.directed_segments {
        if !resolved
            .source_segment_ids
            .iter()
            .any(|source| source == &endpoint_segment.segment_id)
        {
            return Err(invalid(
                "endpoint segment is not referenced by resolved route",
            ));
        }
        let membership_segment = membership
            .segments
            .iter()
            .find(|segment| segment.segment_id == endpoint_segment.segment_id)
            .ok_or_else(|| invalid("endpoint segment membership reference is missing"))?;
        if membership_segment.source_kind != RouteMembershipSourceKind::BoundRamp
            || membership_segment.ordered_edge_ids != endpoint_segment.edge_ids
        {
            return Err(invalid("endpoint boundRamp segment mismatch"));
        }
    }
    Ok(())
}

fn validate_endpoint(
    graph: &Graph,
    endpoint: &BillingEndpoint,
    expected_edge_id: &str,
    expected_edge_kind: EdgeKind,
    expected_ramp_kind: RampKind,
) -> Result<(), RoutingError> {
    valid_id(&endpoint.ramp_id, "endpoint rampId")?;
    if endpoint.name.is_empty()
        || endpoint.support_state != EndpointSupportState::VerifiedBound
        || endpoint.directed_segments.is_empty()
        || !endpoint.binding_candidates.is_empty()
    {
        return Err(invalid(
            "schema 4 endpoint is not an exact verified binding",
        ));
    }
    let ramp = graph
        .ramps
        .iter()
        .find(|ramp| ramp.id == endpoint.ramp_id)
        .ok_or_else(|| invalid("endpoint references unknown ramp"))?;
    if ramp.edge_id != expected_edge_id || ramp.kind != expected_ramp_kind {
        return Err(invalid("endpoint ramp and graph edge mismatch"));
    }
    let edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == expected_edge_id)
        .ok_or_else(|| invalid("endpoint references unknown graph edge"))?;
    let bound = match expected_edge_kind {
        EdgeKind::Entry => {
            edge.kind == EdgeKind::Entry
                && ramp.node_id == edge.from
                && ramp.mainline_node_id == edge.to
        }
        EdgeKind::Exit => {
            edge.kind == EdgeKind::Exit
                && ramp.mainline_node_id == edge.from
                && ramp.node_id == edge.to
        }
        _ => false,
    };
    if !bound {
        return Err(invalid("endpoint direction binding mismatch"));
    }
    let edge_map = graph
        .edges
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let mut segment_ids = HashSet::new();
    for segment in &endpoint.directed_segments {
        valid_id(&segment.segment_id, "endpoint segmentId")?;
        valid_id(&segment.from_node_id, "endpoint fromNodeId")?;
        valid_id(&segment.to_node_id, "endpoint toNodeId")?;
        validate_sha256(&segment.edge_ids_sha256, "edgeIdsSha256")?;
        if !segment_ids.insert(segment.segment_id.as_str())
            || segment.osm_way_ids.is_empty()
            || segment.osm_way_ids.iter().any(|way| *way <= 0)
            || segment
                .osm_way_ids
                .windows(2)
                .any(|pair| pair[0] == pair[1])
        {
            return Err(invalid("invalid endpoint directed segment"));
        }
        if ordered_edge_ids_sha256(&segment.edge_ids).ok().as_deref()
            != Some(segment.edge_ids_sha256.as_str())
        {
            return Err(invalid("endpoint segment edge hash mismatch"));
        }
        let edges =
            validate_ordered_graph_edges(&edge_map, &segment.edge_ids, &segment.segment_id, true)?;
        if edges.iter().any(|edge| edge.kind != expected_edge_kind)
            || edges.first().map(|edge| edge.from.as_str()) != Some(segment.from_node_id.as_str())
            || edges.last().map(|edge| edge.to.as_str()) != Some(segment.to_node_id.as_str())
        {
            return Err(invalid("endpoint segment graph binding mismatch"));
        }
    }
    let all_edges = endpoint
        .directed_segments
        .iter()
        .flat_map(|segment| segment.edge_ids.iter().cloned())
        .collect::<Vec<_>>();
    let all_edges_ref = validate_ordered_graph_edges(&edge_map, &all_edges, "endpoint", true)?;
    let expected_first_or_last = if expected_edge_kind == EdgeKind::Entry {
        all_edges_ref.first().map(|edge| edge.id.as_str())
    } else {
        all_edges_ref.last().map(|edge| edge.id.as_str())
    };
    if expected_first_or_last != Some(expected_edge_id) {
        return Err(invalid("endpoint does not start or end at its ramp edge"));
    }
    Ok(())
}

fn validate_ordered_graph_edges<'a>(
    edge_map: &HashMap<&'a str, &'a crate::Edge>,
    edge_ids: &[String],
    label: &str,
    reject_repeats: bool,
) -> Result<Vec<&'a crate::Edge>, RoutingError> {
    if edge_ids.is_empty() || edge_ids.len() > 20_000 {
        return Err(invalid(format!("{label} edge list is empty or oversized")));
    }
    let mut seen = HashSet::new();
    let mut edges = Vec::with_capacity(edge_ids.len());
    for edge_id in edge_ids {
        let edge = edge_map
            .get(edge_id.as_str())
            .ok_or_else(|| invalid(format!("{label} references unknown edge")))?;
        if reject_repeats && !seen.insert(edge_id.as_str()) {
            return Err(invalid(format!("{label} repeats an edge")));
        }
        if edges
            .last()
            .is_some_and(|previous: &&crate::Edge| previous.to != edge.from)
        {
            return Err(invalid(format!("{label} is disconnected")));
        }
        edges.push(edge);
    }
    Ok(edges)
}

fn ordered_edges<'a>(
    edge_map: &HashMap<&'a str, &'a crate::Edge>,
    edge_ids: &[String],
) -> Result<Vec<&'a crate::Edge>, RoutingError> {
    validate_ordered_graph_edges(edge_map, edge_ids, "ordered edge", false)
}

pub(crate) fn validate_prices(prices: &[Price]) -> Result<(), RoutingError> {
    if prices.len() > 1000 {
        return Err(invalid("too many price records"));
    }
    let mut periods = Vec::with_capacity(prices.len());
    for price in prices {
        let from = utc(&price.effective_from)?;
        let to = price.effective_to.as_deref().map(utc).transpose()?;
        if price.amount_yen == 0 || to.is_some_and(|value| value <= from) {
            return Err(invalid("invalid toll amount or interval"));
        }
        periods.push((from, to));
    }
    periods.sort_by_key(|period| period.0);
    if periods
        .windows(2)
        .any(|pair| pair[0].1.is_none_or(|end| end > pair[1].0))
    {
        return Err(invalid("overlapping toll intervals"));
    }
    Ok(())
}

pub fn ordered_edge_ids_sha256(edge_ids: &[String]) -> Result<String, serde_json::Error> {
    serde_json::to_vec(edge_ids).map(|bytes| {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    })
}

fn valid_id(value: &str, label: &str) -> Result<(), RoutingError> {
    if !valid_id_value(value) {
        return Err(invalid(format!("invalid or empty {label}")));
    }
    Ok(())
}

fn valid_id_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
}

fn validate_sha256(value: &str, label: &str) -> Result<(), RoutingError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid(format!(
            "{label} must be a lowercase SHA-256 value"
        )));
    }
    Ok(())
}
