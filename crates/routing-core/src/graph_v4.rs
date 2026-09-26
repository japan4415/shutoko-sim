use crate::{
    invalid,
    tariff::{read_tariff_v3, TariffResolver},
    utc, BillingPair, EdgeKind, Graph, Price, RampKind, RoutingError, VerificationStatus,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_indexes: Option<Vec<usize>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_order_matches_relation: Option<bool>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_base_fare_yen: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_distance_meters: Option<u64>,
    pub billing_distance_meters: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_to: Option<String>,
    #[serde(default)]
    pub prices: Vec<Price>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_id: Option<String>,
    #[serde(
        default,
        alias = "tariffRuleId",
        skip_serializing_if = "Option::is_none"
    )]
    pub rule_id: Option<String>,
    #[serde(default, alias = "evidenceId", skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    #[serde(
        default,
        alias = "billingDistanceEvidenceId",
        skip_serializing_if = "Option::is_none"
    )]
    pub distance_evidence_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fare_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fare_basis: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub discounts_excluded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toll_source: Option<String>,
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
    pub osm_node_ids: Vec<i64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_ramp_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_ramp_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_name: Option<String>,
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
    pub(crate) tariff_resolver: Option<TariffResolver>,
    pub(crate) tariff_overrides: HashMap<String, Tariff>,
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
    od_tariffs: Option<Value>,
    #[serde(default)]
    tariff: Option<Value>,
    #[serde(default)]
    od_tariffs_v3: Option<Value>,
    #[serde(default)]
    tariff_catalog: Option<Value>,
    #[serde(default)]
    tariff_model: Option<Value>,
    #[serde(default)]
    billing_pairs_version: Option<String>,
    #[serde(default)]
    tariff_model_version: Option<u32>,
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
    if envelope
        .billing_pairs_version
        .as_deref()
        .is_some_and(|version| !matches!(version, "v2" | "v3"))
        || envelope
            .tariff_model_version
            .is_some_and(|version| version != crate::tariff::TARIFF_MODEL_VERSION)
    {
        return Err(invalid("unsupported tariff model version"));
    }
    let (od_tariffs, od_tariff_resolver) = match envelope.od_tariffs {
        Some(Value::Array(values)) => {
            let tariffs = serde_json::from_value(Value::Array(values))
                .map_err(|_| invalid("invalid legacy odTariffs"))?;
            (tariffs, None)
        }
        Some(value) => {
            let resolver = parse_graph_tariff_catalog(&value, "odTariffs")?;
            (Vec::new(), resolver)
        }
        None => (Vec::new(), None),
    };
    let mut tariff_resolver = od_tariff_resolver;
    for (value, label) in [
        (envelope.tariff.as_ref(), "tariff"),
        (envelope.od_tariffs_v3.as_ref(), "odTariffsV3"),
        (envelope.tariff_catalog.as_ref(), "tariffCatalog"),
        (envelope.tariff_model.as_ref(), "tariffModel"),
    ] {
        if let Some(value) = value {
            if let Some(resolver) = parse_graph_tariff_catalog(value, label)? {
                if tariff_resolver.is_some() {
                    return Err(invalid("multiple tariff catalogs are not allowed"));
                }
                tariff_resolver = Some(resolver);
            }
        }
    }
    if tariff_resolver
        .as_ref()
        .is_some_and(|resolver| resolver.catalog().vehicle_profile != envelope.vehicle_profile)
    {
        return Err(invalid(
            "tariff catalog vehicle profile does not match graph",
        ));
    }
    let mut billing_pairs = Vec::with_capacity(envelope.billing_pairs.len());
    let mut schema4_legacy_pairs = Vec::new();
    let mut radial_billing_pairs = Vec::new();
    let mut tariff_overrides = HashMap::new();
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
                        tariff_overrides.insert(pair.id.clone(), pair.tariff.clone());
                        billing_pairs.push(pair.to_legacy()?);
                        schema4_legacy_pairs.push(pair);
                    }
                    "radialReturn" => {
                        let pair: RadialReturnBillingPair = serde_json::from_value(value)
                            .map_err(|_| invalid("invalid radialReturn billing pair"))?;
                        tariff_overrides.insert(pair.id.clone(), pair.tariff.clone());
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
        od_tariffs,
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
    if let Some(resolver) = &tariff_resolver {
        for pair in &schema4_legacy_pairs {
            if let Some(assignment_id) = pair.tariff.assignment_id.as_deref() {
                resolver
                    .validate_assignment_selectors(
                        assignment_id,
                        pair.entry_ramp_id.as_deref(),
                        pair.exit_ramp_id.as_deref(),
                        Some(pair.id.as_str()),
                    )
                    .map_err(|error| {
                        invalid(format!(
                            "legacyRing tariff assignment binding is invalid: {error}"
                        ))
                    })?;
            }
        }
        for pair in &radial_billing_pairs {
            if let Some(assignment_id) = pair.tariff.assignment_id.as_deref() {
                resolver
                    .validate_assignment_selectors(
                        assignment_id,
                        Some(pair.entry_endpoint.ramp_id.as_str()),
                        Some(pair.exit_endpoint.ramp_id.as_str()),
                        Some(pair.id.as_str()),
                    )
                    .map_err(|error| {
                        invalid(format!(
                            "radialReturn tariff assignment binding is invalid: {error}"
                        ))
                    })?;
            }
        }
    }
    Ok(ParsedGraph {
        graph,
        radial_billing_pairs,
        route_memberships,
        tariff_resolver,
        tariff_overrides,
    })
}

fn parse_graph_tariff_catalog(
    value: &Value,
    label: &str,
) -> Result<Option<TariffResolver>, RoutingError> {
    if value.is_null() {
        return Ok(None);
    }
    let Some(object) = value.as_object() else {
        if label == "tariffModel" {
            return Ok(None);
        }
        return Err(invalid(format!("{label} must be an object")));
    };
    let nested = object
        .get("catalog")
        .or_else(|| object.get("tariffCatalog"))
        .or_else(|| object.get("odTariffs"));
    let value = nested.unwrap_or(value);
    let Some(object) = value.as_object() else {
        return Err(invalid(format!("{label} catalog must be an object")));
    };
    if object.get("assignments").is_none() || object.get("tariffRules").is_none() {
        return Ok(None);
    }
    let serialized = serde_json::to_string(value)
        .map_err(|_| invalid(format!("{label} could not be serialized")))?;
    let catalog = read_tariff_v3(&serialized)
        .map_err(|error| invalid(format!("{label} is invalid: {error}")))?;
    TariffResolver::new(catalog)
        .map(Some)
        .map_err(|error| invalid(format!("{label} is invalid: {error}")))
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
            entry_ramp_id: pair.entry_ramp_id.clone(),
            exit_ramp_id: pair.exit_ramp_id.clone(),
            entry_name: pair.entry_name.clone(),
            exit_name: pair.exit_name.clone(),
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
                observed_base_fare_yen: None,
                observed_distance_meters: None,
                billing_distance_meters: pair.billing_distance_meters,
                effective_from: None,
                effective_to: None,
                prices: pair.prices.clone(),
                assignment_id: pair.assignment_id.clone(),
                rule_id: None,
                evidence_id: None,
                distance_evidence_id: None,
                fare_label: None,
                vehicle_class: None,
                payment_method: None,
                fare_basis: None,
                discounts_excluded: false,
                toll_source: None,
            },
        })
    }

    fn to_legacy(&self) -> Result<BillingPair, RoutingError> {
        validate_tariff_wire_contract(&self.tariff)?;
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
                (!self.tariff.prices.is_empty() && self.tariff.amount_yen.is_some())
                    || (self.tariff.amount_yen.is_some()
                        && self.tariff.billing_distance_meters.is_some()
                        && self.tariff.assignment_id.is_some()
                        && self.tariff.rule_id.is_some()
                        && self.tariff.evidence_id.is_some()
                        && self.tariff.distance_evidence_id.is_some())
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
            || self.entry_ramp_id.as_deref().is_some_and(str::is_empty)
            || self.exit_ramp_id.as_deref().is_some_and(str::is_empty)
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
            assignment_id: self.tariff.assignment_id.clone(),
            status: match self.pair_eligibility.status {
                PairEligibilityStatus::VerifiedOneSectionAhead => VerificationStatus::Verified,
                PairEligibilityStatus::Unverified | PairEligibilityStatus::TopologyOnly => {
                    VerificationStatus::Unverified
                }
            },
            vehicle_profile: self.vehicle_profile.clone(),
            prices: self.tariff.prices.clone(),
            entry_name: self.entry_name.clone(),
            exit_name: self.exit_name.clone(),
            entry_ramp_id: self.entry_ramp_id.clone(),
            exit_ramp_id: self.exit_ramp_id.clone(),
            billing_distance_meters: self.tariff.billing_distance_meters,
        })
    }
}

fn infer_legacy_membership<'a>(
    pair: &BillingPair,
    graph: &Graph,
    memberships: &'a [RouteMembershipIndex],
) -> Result<&'a RouteMembershipIndex, RoutingError> {
    let declared_entry = pair.entry_ramp_id.as_deref().and_then(|id| {
        graph
            .ramps
            .iter()
            .find(|ramp| ramp.id == id)
            .map(|ramp| (ramp.route.as_str(), ramp.direction.as_str()))
    });
    let resolved = resolve_legacy_membership(
        graph,
        memberships,
        &pair.entry_to_anchor_edge_ids,
        &pair.anchor_to_exit_edge_ids,
        &pair.anchor_node_id,
        declared_entry,
        &pair.id,
    )?;
    Ok(resolved)
}

fn relation_segments_match_required(
    segments: &[&RouteMembershipSegment],
    required: &[&str],
) -> bool {
    let [segment] = segments else {
        return false;
    };
    let positions = segment
        .ordered_edge_ids
        .iter()
        .enumerate()
        .map(|(index, edge_id)| (edge_id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let matches = required
        .iter()
        .enumerate()
        .filter_map(|(required_index, edge_id)| {
            positions
                .get(edge_id)
                .copied()
                .map(|position| (required_index, position))
        })
        .collect::<Vec<_>>();
    let Some(&(first_required, first_position)) = matches.first() else {
        return false;
    };
    let Some(&(last_required, _last_position)) = matches.last() else {
        return false;
    };
    if required[first_required..=last_required]
        .iter()
        .enumerate()
        .any(|(offset, edge_id)| positions.get(edge_id) != Some(&(first_position + offset)))
    {
        return false;
    }
    required.iter().enumerate().all(|(index, edge_id)| {
        positions.contains_key(edge_id) || index < first_required || index > last_required
    })
}

#[allow(clippy::too_many_arguments)]
fn resolve_legacy_membership<'a>(
    graph: &Graph,
    memberships: &'a [RouteMembershipIndex],
    entry_to_anchor_edge_ids: &[String],
    anchor_to_exit_edge_ids: &[String],
    anchor_node_id: &str,
    declared_entry: Option<(&str, &str)>,
    pair_id: &str,
) -> Result<&'a RouteMembershipIndex, RoutingError> {
    let edge_map = graph
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let entry = validate_ordered_graph_edges(
        &edge_map,
        entry_to_anchor_edge_ids,
        "legacy entry-to-anchor",
        true,
    )?;
    let exit = validate_ordered_graph_edges(
        &edge_map,
        anchor_to_exit_edge_ids,
        "legacy anchor-to-exit",
        true,
    )?;
    if entry.first().map(|edge| edge.kind) != Some(EdgeKind::Entry)
        || entry
            .iter()
            .skip(1)
            .any(|edge| edge.kind != EdgeKind::Shutoko)
        || exit.last().map(|edge| edge.kind) != Some(EdgeKind::Exit)
        || exit
            .iter()
            .take(exit.len().saturating_sub(1))
            .any(|edge| edge.kind != EdgeKind::Shutoko)
        || entry.last().map(|edge| edge.to.as_str()) != Some(anchor_node_id)
        || exit.first().map(|edge| edge.from.as_str()) != Some(anchor_node_id)
    {
        return Err(invalid(format!(
            "legacy billing pair {pair_id} has invalid entry, anchor, or exit boundaries"
        )));
    }
    let required = entry
        .iter()
        .skip(1)
        .map(|edge| edge.id.as_str())
        .chain(
            exit.iter()
                .take(exit.len().saturating_sub(1))
                .map(|edge| edge.id.as_str()),
        )
        .collect::<Vec<_>>();
    if required.is_empty()
        || required.iter().copied().collect::<HashSet<_>>().len() != required.len()
    {
        return Err(invalid(format!(
            "legacy billing pair {pair_id} has no unique mainline edge sequence"
        )));
    }
    let matches = memberships
        .iter()
        .filter(|membership| {
            !declared_entry.is_some_and(|(route, direction)| {
                membership.route_id != route || membership.direction != direction
            })
        })
        .filter(|membership| {
            let relation_segments = membership
                .segments
                .iter()
                .filter(|segment| {
                    segment.source_kind == RouteMembershipSourceKind::RelationMainline
                })
                .collect::<Vec<_>>();
            relation_segments_match_required(&relation_segments, &required)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(invalid(format!(
            "legacy billing pair {pair_id} must resolve through an ordered mainline segment sequence (matches={})",
            matches.len()
        )));
    }
    Ok(matches[0])
}

fn validate_legacy_ramp_id(
    graph: &Graph,
    ramp_id: Option<&str>,
    expected_edge_id: &str,
    expected_kind: RampKind,
) -> Result<(), RoutingError> {
    let Some(ramp_id) = ramp_id else {
        return Ok(());
    };
    let ramp = graph
        .ramps
        .iter()
        .find(|ramp| ramp.id == ramp_id)
        .ok_or_else(|| invalid("legacyRing references an unknown ramp"))?;
    if ramp.kind != expected_kind || ramp.edge_id != expected_edge_id {
        return Err(invalid(
            "legacyRing ramp ID does not bind its endpoint edge",
        ));
    }
    Ok(())
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
        || wire.entry_ramp_id != normalized.entry_ramp_id
        || wire.exit_ramp_id != normalized.exit_ramp_id
        || wire.entry_name != normalized.entry_name
        || wire.exit_name != normalized.exit_name
        || anchor.node_id != normalized.anchor_node_id
        || wire.vehicle_profile != graph.vehicle_profile
    {
        return Err(invalid("legacyRing normalized identity mismatch"));
    }
    validate_legacy_ramp_id(
        graph,
        wire.entry_ramp_id.as_deref(),
        &wire.entry_id,
        RampKind::GeneralEntry,
    )?;
    validate_legacy_ramp_id(
        graph,
        wire.exit_ramp_id.as_deref(),
        &wire.exit_id,
        RampKind::GeneralExit,
    )?;
    resolve_legacy_membership(
        graph,
        memberships,
        &wire.entry_to_anchor_edge_ids,
        &wire.anchor_to_exit_edge_ids,
        &anchor.node_id,
        Some((&anchor.route_id, &anchor.direction)),
        &wire.id,
    )?;
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
            let relation_metadata_valid = match segment.source_kind {
                RouteMembershipSourceKind::RelationMainline => {
                    segment.member_indexes.as_ref().is_some_and(|indexes| {
                        !indexes.is_empty()
                            && indexes.iter().collect::<HashSet<_>>().len() == indexes.len()
                            && segment.member_order_matches_relation
                                == Some(indexes.windows(2).all(|pair| pair[1] == pair[0] + 1))
                    })
                }
                RouteMembershipSourceKind::BoundRamp => {
                    segment.member_indexes.is_none()
                        && segment.member_order_matches_relation.is_none()
                }
            };
            if !bound || !relation_metadata_valid {
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
    validate_tariff_wire_contract(&pair.tariff)?;
    validate_prices(&pair.tariff.prices)?;
    let priced_contract = (!pair.tariff.prices.is_empty() && pair.tariff.amount_yen.is_some())
        || (pair.tariff.amount_yen.is_some()
            && pair.tariff.billing_distance_meters.is_some()
            && pair.tariff.assignment_id.is_some()
            && pair.tariff.rule_id.is_some()
            && pair.tariff.evidence_id.is_some()
            && pair.tariff.distance_evidence_id.is_some());
    if matches!(pair.tariff.status, TariffStatus::Priced) && !priced_contract
        || matches!(pair.tariff.status, TariffStatus::Unpriced)
            && (!pair.tariff.prices.is_empty()
                || pair.tariff.amount_yen.is_some()
                || pair.tariff.billing_distance_meters.is_some())
    {
        return Err(invalid("radialReturn tariff status is inconsistent"));
    }
    let entry_ramp = validate_endpoint(
        graph,
        &pair.entry_endpoint,
        &pair.entry_id,
        EdgeKind::Entry,
        RampKind::GeneralEntry,
    )?;
    let exit_ramp = validate_endpoint(
        graph,
        &pair.exit_endpoint,
        &pair.exit_id,
        EdgeKind::Exit,
        RampKind::GeneralExit,
    )?;
    let eligibility_consistent = matches!(
        (
            pair.pair_eligibility.status,
            pair.pair_eligibility.one_section_ahead_verified
        ),
        (PairEligibilityStatus::VerifiedOneSectionAhead, true)
            | (PairEligibilityStatus::Unverified, false)
            | (PairEligibilityStatus::TopologyOnly, false)
    );
    if pair.routing_capability != RoutingCapability::Routable
        || !eligibility_consistent
        || pair.loop_validation.status != LoopValidationStatus::DeclaredRouteValidated
    {
        return Err(invalid(
            "radialReturn capability and status contract is inconsistent",
        ));
    }
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
        || entry_membership.route_id != entry_ramp.route
        || entry_membership.direction != entry_ramp.direction
        || return_membership.route_id != exit_ramp.route
        || return_membership.direction != exit_ramp.direction
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
        || entry_edges.last().map(|edge| edge.id.as_str())
            != Some(anchor.merge_terminal_edge_id.as_str())
        || lap_edges.first().map(|edge| edge.from.as_str()) != Some(anchor.merge_node_id.as_str())
        || lap_edges.last().map(|edge| edge.to.as_str()) != Some(anchor.branch_node_id.as_str())
        || return_edges.first().map(|edge| edge.from.as_str())
            != Some(anchor.branch_node_id.as_str())
        || return_edges.first().map(|edge| edge.id.as_str())
            != Some(anchor.branch_initial_edge_id.as_str())
        || lap_edges.first().map(|edge| edge.id.as_str())
            != Some(pair.route_plan.mandatory_lap.first_edge_id.as_str())
        || lap_edges.last().map(|edge| edge.id.as_str())
            != Some(pair.route_plan.mandatory_lap.last_edge_id.as_str())
        || exit_edges.last().map(|edge| edge.kind) != Some(EdgeKind::Exit)
    {
        return Err(invalid("radialReturn resolved segment boundary mismatch"));
    }
    validate_excluded_short_connector(
        &edge_map,
        &anchor.excluded_short_connector,
        &lap.edge_ids,
        &lap_edges,
    )?;
    validate_endpoint_membership_binding(
        memberships,
        &pair.entry_endpoint,
        entry,
        &entry_ramp.route,
        &entry_ramp.direction,
    )?;
    validate_endpoint_membership_binding(
        memberships,
        &pair.exit_endpoint,
        exit,
        &exit_ramp.route,
        &exit_ramp.direction,
    )?;
    validate_ordered_graph_edges(&edge_map, &full_edge_ids, "radialReturn full route", false)?;
    Ok(())
}

fn validate_resolved_segment_sources(
    membership: &RouteMembershipIndex,
    resolved: &ResolvedRouteSegment,
) -> Result<(), RoutingError> {
    let sources = resolved
        .source_segment_ids
        .iter()
        .map(|source_id| {
            membership
                .segments
                .iter()
                .find(|segment| &segment.segment_id == source_id)
                .ok_or_else(|| invalid("resolved route source segment is missing"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ordered_sources_match = if resolved.role == RoutePlanSegmentRole::EntryApproach {
        entry_approach_source_sequences_match(&sources, &resolved.edge_ids)
    } else {
        source_sequences_match(&sources, 0, &resolved.edge_ids, 0)
    };
    if !ordered_sources_match {
        return Err(invalid(
            "resolved route source segments do not contain ordered edgeIds",
        ));
    }
    let source_kinds = sources
        .iter()
        .map(|source| source.source_kind)
        .collect::<Vec<_>>();
    let valid_sources = match resolved.role {
        RoutePlanSegmentRole::EntryApproach => {
            source_kinds.first() == Some(&RouteMembershipSourceKind::BoundRamp)
                && source_kinds[1..]
                    .iter()
                    .all(|kind| *kind == RouteMembershipSourceKind::RelationMainline)
        }
        RoutePlanSegmentRole::MandatoryLap => {
            source_kinds == [RouteMembershipSourceKind::RelationMainline]
        }
        RoutePlanSegmentRole::ReturnCorridor => source_kinds
            .iter()
            .all(|kind| *kind == RouteMembershipSourceKind::RelationMainline),
        RoutePlanSegmentRole::ExitApproach => source_kinds
            .iter()
            .all(|kind| *kind == RouteMembershipSourceKind::BoundRamp),
    };
    if !valid_sources {
        return Err(invalid(
            "resolved route segment source kinds do not match its role",
        ));
    }
    Ok(())
}

fn source_sequences_match(
    sources: &[&RouteMembershipSegment],
    source_index: usize,
    edge_ids: &[String],
    edge_cursor: usize,
) -> bool {
    let Some(source) = sources.get(source_index) else {
        return edge_cursor == edge_ids.len();
    };
    if source.ordered_edge_ids.is_empty() {
        return false;
    }
    (0..source.ordered_edge_ids.len()).any(|start| {
        (1..=source.ordered_edge_ids.len()).any(|length| {
            let end = edge_cursor.saturating_add(length);
            end <= edge_ids.len()
                && (0..length).all(|offset| {
                    source.ordered_edge_ids[(start + offset) % source.ordered_edge_ids.len()]
                        == edge_ids[edge_cursor + offset]
                })
                && source_sequences_match(sources, source_index + 1, edge_ids, end)
        })
    })
}

fn entry_approach_source_sequences_match(
    sources: &[&RouteMembershipSegment],
    edge_ids: &[String],
) -> bool {
    let relation_segments = sources
        .iter()
        .copied()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
        .collect::<Vec<_>>();
    let relation_edge_ids = relation_segments
        .iter()
        .flat_map(|segment| segment.ordered_edge_ids.iter())
        .collect::<HashSet<_>>();
    let Some(relation_start) = edge_ids
        .iter()
        .position(|edge_id| relation_edge_ids.contains(edge_id))
    else {
        return source_sequences_match(sources, 0, edge_ids, 0);
    };
    if relation_start == 0 {
        return source_sequences_match(sources, 0, edge_ids, 0);
    }
    let bound_segments = sources
        .iter()
        .copied()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::BoundRamp)
        .collect::<Vec<_>>();
    let [bound_segment] = bound_segments.as_slice() else {
        return source_sequences_match(sources, 0, edge_ids, 0);
    };
    if edge_ids.get(..bound_segment.ordered_edge_ids.len())
        != Some(bound_segment.ordered_edge_ids.as_slice())
        || edge_ids[relation_start..]
            .iter()
            .any(|edge_id| !relation_edge_ids.contains(edge_id))
    {
        return false;
    }
    source_sequences_match(&relation_segments, 0, &edge_ids[relation_start..], 0)
}

fn validate_endpoint_membership_binding(
    memberships: &[RouteMembershipIndex],
    endpoint: &BillingEndpoint,
    resolved: &ResolvedRouteSegment,
    expected_route_id: &str,
    expected_direction: &str,
) -> Result<(), RoutingError> {
    let membership = memberships
        .iter()
        .find(|membership| membership.membership_id == resolved.membership_id)
        .ok_or_else(|| invalid("resolved endpoint membership is missing"))?;
    if membership.route_id != expected_route_id || membership.direction != expected_direction {
        return Err(invalid(
            "endpoint ramp route/direction does not match its membership",
        ));
    }
    for endpoint_segment in &endpoint.directed_segments {
        let membership_segments = membership
            .segments
            .iter()
            .filter(|segment| {
                segment.source_kind == RouteMembershipSourceKind::BoundRamp
                    && segment.ordered_edge_ids == endpoint_segment.edge_ids
            })
            .collect::<Vec<_>>();
        if membership_segments.len() != 1
            || !resolved
                .source_segment_ids
                .iter()
                .any(|source| source == &membership_segments[0].segment_id)
        {
            return Err(invalid("endpoint boundRamp segment mismatch"));
        }
    }
    Ok(())
}

fn validate_endpoint<'a>(
    graph: &'a Graph,
    endpoint: &BillingEndpoint,
    expected_edge_id: &str,
    expected_edge_kind: EdgeKind,
    expected_ramp_kind: RampKind,
) -> Result<&'a crate::Ramp, RoutingError> {
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
    let edge_map = graph
        .edges
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let ramp = graph
        .ramps
        .iter()
        .find(|ramp| ramp.id == endpoint.ramp_id)
        .ok_or_else(|| invalid("endpoint references unknown ramp"))?;
    if ramp.kind != expected_ramp_kind || ramp.route.is_empty() || ramp.direction.is_empty() {
        return Err(invalid("endpoint ramp kind or route identity mismatch"));
    }
    let pair_edge = edge_map
        .get(expected_edge_id)
        .ok_or_else(|| invalid("endpoint references unknown graph edge"))?;
    let ramp_edge = edge_map
        .get(ramp.edge_id.as_str())
        .ok_or_else(|| invalid("endpoint ramp references unknown graph edge"))?;
    if pair_edge.kind != expected_edge_kind || ramp_edge.kind != expected_edge_kind {
        return Err(invalid("endpoint ramp and graph edge kind mismatch"));
    }
    let mut segment_ids = HashSet::new();
    for segment in &endpoint.directed_segments {
        valid_id(&segment.segment_id, "endpoint segmentId")?;
        valid_id(&segment.from_node_id, "endpoint fromNodeId")?;
        valid_id(&segment.to_node_id, "endpoint toNodeId")?;
        validate_sha256(&segment.edge_ids_sha256, "edgeIdsSha256")?;
        if !segment_ids.insert(segment.segment_id.as_str())
            || segment.osm_way_ids.is_empty()
            || segment.osm_way_ids.iter().any(|way| *way <= 0)
            || segment.osm_node_ids.len() != segment.edge_ids.len() + 1
            || segment.osm_node_ids.iter().any(|node| *node <= 0)
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
        let mut actual_way_ids = Vec::new();
        for edge in &edges {
            let way_id = edge_way_id(edge.id.as_str())
                .ok_or_else(|| invalid("endpoint segment edge has no OSM way identity"))?;
            if actual_way_ids.last() != Some(&way_id) {
                actual_way_ids.push(way_id);
            }
        }
        let graph_osm_node_ids = edges
            .first()
            .map(|edge| edge.from.as_str())
            .into_iter()
            .chain(edges.iter().map(|edge| edge.to.as_str()))
            .map(graph_node_osm_id)
            .collect::<Option<Vec<_>>>();
        if edges.iter().any(|edge| edge.kind != expected_edge_kind)
            || edges.first().map(|edge| edge.from.as_str()) != Some(segment.from_node_id.as_str())
            || edges.last().map(|edge| edge.to.as_str()) != Some(segment.to_node_id.as_str())
            || actual_way_ids != segment.osm_way_ids
            || graph_osm_node_ids
                .as_ref()
                .is_some_and(|node_ids| node_ids != &segment.osm_node_ids)
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
    let bound = match expected_edge_kind {
        EdgeKind::Entry => {
            all_edges_ref.first().map(|edge| edge.from.as_str()) == Some(ramp.node_id.as_str())
                && all_edges_ref.last().map(|edge| edge.to.as_str())
                    == Some(ramp.mainline_node_id.as_str())
        }
        EdgeKind::Exit => {
            all_edges_ref.first().map(|edge| edge.from.as_str())
                == Some(ramp.mainline_node_id.as_str())
                && all_edges_ref.last().map(|edge| edge.to.as_str()) == Some(ramp.node_id.as_str())
        }
        _ => false,
    };
    if !bound {
        return Err(invalid("endpoint direction binding mismatch"));
    }
    let expected_first_or_last = if expected_edge_kind == EdgeKind::Entry {
        all_edges_ref.first().map(|edge| edge.id.as_str())
    } else {
        all_edges_ref.last().map(|edge| edge.id.as_str())
    };
    if all_edges_ref.first().map(|edge| edge.id.as_str()) != Some(ramp.edge_id.as_str())
        || expected_first_or_last != Some(expected_edge_id)
    {
        return Err(invalid("endpoint does not start or end at its ramp edge"));
    }
    Ok(ramp)
}

fn collect_short_connector_paths<'a>(
    edge_map: &HashMap<&'a str, &'a crate::Edge>,
    osm_way_id: i64,
    current: &str,
    target: &str,
    path: &mut Vec<String>,
    visited_nodes: &mut HashSet<String>,
    paths: &mut Vec<Vec<String>>,
) -> Result<(), RoutingError> {
    if path.len() > 20_000 {
        return Err(invalid("short connector search exceeded 20000 edges"));
    }
    let mut outgoing = edge_map
        .values()
        .copied()
        .filter(|edge| {
            edge.kind == EdgeKind::Shutoko
                && edge_way_id(edge.id.as_str()) == Some(osm_way_id)
                && edge.from == current
        })
        .collect::<Vec<_>>();
    outgoing.sort_by(|left, right| left.id.cmp(&right.id));
    for edge in outgoing {
        if edge.to == target {
            let mut candidate = path.clone();
            candidate.push(edge.id.clone());
            paths.push(candidate);
            if paths.len() > 1 {
                return Err(invalid("short connector has multiple directed paths"));
            }
            continue;
        }
        if !visited_nodes.insert(edge.to.clone()) {
            continue;
        }
        path.push(edge.id.clone());
        collect_short_connector_paths(
            edge_map,
            osm_way_id,
            &edge.to,
            target,
            path,
            visited_nodes,
            paths,
        )?;
        path.pop();
        visited_nodes.remove(&edge.to);
    }
    Ok(())
}

fn validate_excluded_short_connector(
    edge_map: &HashMap<&str, &crate::Edge>,
    connector: &ExcludedShortConnector,
    lap_edge_ids: &[String],
    lap_edges: &[&crate::Edge],
) -> Result<(), RoutingError> {
    if connector.from_node_id == connector.to_node_id
        || connector.osm_way_id <= 0
        || connector.edge_count == 0
        || connector.distance_meters == 0
        || lap_edges.first().map(|edge| edge.from.as_str()) != Some(connector.to_node_id.as_str())
        || lap_edges.last().map(|edge| edge.to.as_str()) != Some(connector.from_node_id.as_str())
    {
        return Err(invalid("invalid short connector or lap boundary"));
    }
    let mut paths = Vec::new();
    let mut path = Vec::new();
    let mut visited_nodes = HashSet::from([connector.from_node_id.clone()]);
    collect_short_connector_paths(
        edge_map,
        connector.osm_way_id,
        &connector.from_node_id,
        &connector.to_node_id,
        &mut path,
        &mut visited_nodes,
        &mut paths,
    )?;
    let edge_ids = paths
        .pop()
        .ok_or_else(|| invalid("short connector has no directed graph path"))?;
    let edges = validate_ordered_graph_edges(edge_map, &edge_ids, "short connector", true)?;
    let distance = edges
        .iter()
        .try_fold(0_u64, |total, edge| total.checked_add(edge.distance_meters));
    if edges.len() != connector.edge_count as usize
        || distance != Some(connector.distance_meters)
        || edge_ids
            .iter()
            .any(|edge_id| lap_edge_ids.contains(edge_id))
    {
        return Err(invalid(
            "short connector evidence does not match the graph or mandatory lap",
        ));
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

fn edge_way_id(edge_id: &str) -> Option<i64> {
    let mut parts = edge_id.split(':');
    if parts.next()? != "e" {
        return None;
    }
    parts.next()?.strip_prefix('w')?.parse().ok()
}

fn graph_node_osm_id(node_id: &str) -> Option<i64> {
    node_id.strip_prefix("n:")?.parse().ok()
}

fn validate_tariff_wire_contract(tariff: &Tariff) -> Result<(), RoutingError> {
    let has_v3_fields = tariff.assignment_id.is_some()
        || tariff.rule_id.is_some()
        || tariff.evidence_id.is_some()
        || tariff.distance_evidence_id.is_some()
        || tariff.fare_label.is_some()
        || tariff.vehicle_class.is_some()
        || tariff.payment_method.is_some()
        || tariff.fare_basis.is_some()
        || tariff.discounts_excluded
        || tariff.toll_source.is_some();
    if !has_v3_fields {
        return Ok(());
    }
    if tariff.fare_label.as_deref() != Some(crate::PRODUCT_FARE_LABEL)
        || tariff.vehicle_class.as_deref() != Some(crate::PRODUCT_VEHICLE_CLASS)
        || tariff.payment_method.as_deref() != Some(crate::PRODUCT_PAYMENT_METHOD)
        || tariff.fare_basis.as_deref() != Some(crate::PRODUCT_FARE_BASIS)
        || !tariff.discounts_excluded
    {
        return Err(invalid("tariff scope is inconsistent"));
    }
    match tariff.status {
        TariffStatus::Priced => {
            if tariff.amount_yen.is_none()
                || tariff.billing_distance_meters.is_none()
                || tariff.assignment_id.is_none()
                || tariff.rule_id.is_none()
                || tariff.evidence_id.is_none()
                || tariff.distance_evidence_id.is_none()
                || tariff.toll_source.as_deref() != Some(crate::OFFICIAL_DISTANCE_RULE_SOURCE)
                || (tariff.effective_from.is_none() && tariff.prices.is_empty())
            {
                return Err(invalid("priced tariff provenance is incomplete"));
            }
        }
        TariffStatus::Unpriced | TariffStatus::Expired | TariffStatus::NotApplicable => {
            if tariff.amount_yen.is_some()
                || tariff.billing_distance_meters.is_some()
                || tariff.assignment_id.is_some()
                || tariff.rule_id.is_some()
                || tariff.evidence_id.is_some()
                || tariff.distance_evidence_id.is_some()
                || tariff.toll_source.is_some()
            {
                return Err(invalid("unpriced tariff contains priced fields"));
            }
        }
    }
    Ok(())
}

fn is_false(value: &bool) -> bool {
    !*value
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
