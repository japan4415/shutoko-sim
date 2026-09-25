//! Declarative billing pair seed definitions.
//!
//! # Specification: `data/billing-pairs-seed.json`
//!
//! Because toll eligibility and accurate entrance-to-exit pairings cannot be safely
//! deduced from OSM geometry alone, billing pairs are defined in a declarative seed
//! file curated with human verification and authoritative tariff citations.

use crate::model::VerificationStatus;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairsSeedFile {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub billing_pairs: Vec<BillingPairSeed>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairSeed {
    pub id: String,
    pub entry_osm_way_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_name: Option<String>,
    pub exit_osm_way_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_name: Option<String>,
    pub anchor_osm_node_id: i64,
    pub vehicle_profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_id: Option<String>,
    pub status: VerificationStatus,
    pub one_section_ahead_verified: bool,
    pub provenance: SeedProvenance,
    #[serde(default)]
    pub prices: Vec<SeedPrice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedProvenance {
    pub source: String,
    pub source_date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedPrice {
    pub amount_yen: u64,
    pub effective_from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BillingPairsSeedFileV2 {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub billing_pairs: Vec<BillingPairSeedEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum BillingPairSeedEntry {
    LegacyRing(Box<BillingPairSeed>),
    RadialReturn(Box<RadialReturnBillingPairSeed>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ParsedBillingPairsSeed {
    Schema1(BillingPairsSeedFile),
    Schema2(BillingPairsSeedFileV2),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RadialReturnBillingPairSeed {
    pub id: String,
    pub pair_kind: PairKind,
    pub route_plan_version: RoutePlanVersion,
    pub vehicle_profile: String,
    pub assignment_id: String,
    pub entry_endpoint: DiagnosticEndpoint,
    pub exit_endpoint: DiagnosticEndpoint,
    pub route_plan: DiagnosticRoutePlan,
    pub routing_capability: RoutingCapability,
    pub pair_eligibility: PairEligibility,
    pub loop_validation: LoopValidation,
    pub tariff: DiagnosticTariff,
    pub provenance: SeedProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairKind {
    #[serde(rename = "radialReturn")]
    RadialReturn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePlanVersion {
    V1,
}

impl Serialize for RoutePlanVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::V1 => serializer.serialize_u8(1),
        }
    }
}

impl<'de> Deserialize<'de> for RoutePlanVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self::V1),
            version => Err(D::Error::custom(format!(
                "unsupported routePlanVersion: {}",
                version
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticEndpoint {
    pub ramp_id: String,
    pub name: String,
    pub support_state: EndpointSupportState,
    pub directed_segments: Vec<DirectedEndpointSegment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binding_candidates: Vec<BindingCandidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointSupportState {
    VerifiedBound,
    Unresolved,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingCandidate {
    pub candidate_id: String,
    pub status: BindingCandidateStatus,
    pub directed_segments: Vec<DirectedEndpointSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingCandidateStatus {
    Unresolved,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticRoutePlan {
    pub entry_corridor: EntryCorridor,
    pub anchor: DirectedJunctionAnchor,
    pub mandatory_lap: MandatoryLap,
    pub return_corridor: ReturnCorridor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryCorridor {
    pub membership_id: String,
    pub terminal_edge_id: String,
    pub merge_node_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectedJunctionAnchor {
    pub anchor_kind: AnchorKind,
    pub route_id: String,
    pub direction: String,
    pub merge_node_id: String,
    pub branch_node_id: String,
    pub merge_terminal_edge_id: String,
    pub branch_initial_edge_id: String,
    pub arc_policy: ArcPolicy,
    pub excluded_short_connector: ExcludedShortConnector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorKind {
    DirectedJunction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArcPolicy {
    OrdinaryLongArc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExcludedShortConnector {
    pub from_node_id: String,
    pub to_node_id: String,
    pub osm_way_id: i64,
    pub edge_count: u32,
    pub distance_meters: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MandatoryLap {
    pub membership_id: String,
    pub first_edge_id: String,
    pub last_edge_id: String,
    pub lap_count: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReturnCorridor {
    pub membership_id: String,
    pub start_node_id: String,
    pub initial_edge_id: String,
    pub first_general_exit: FirstGeneralExit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FirstGeneralExit {
    pub rule: FirstGeneralExitRule,
    pub expected_ramp_id: String,
    pub exact_directed_binding: EndpointSupportState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirstGeneralExitRule {
    FirstGeneralExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingCapability {
    Routable,
    StructuralNoLoop,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairEligibility {
    pub status: PairEligibilityStatus,
    pub one_section_ahead_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairEligibilityStatus {
    VerifiedOneSectionAhead,
    Unverified,
    TopologyOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoopValidation {
    pub status: LoopValidationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopValidationStatus {
    DeclaredRouteValidated,
    Unresolved,
    TopologyOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticTariff {
    pub status: TariffStatus,
    pub amount_yen: Option<u64>,
    pub billing_distance_meters: Option<u64>,
    pub prices: Vec<SeedPrice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TariffStatus {
    Priced,
    Unpriced,
    Expired,
    NotApplicable,
}

#[derive(Debug)]
pub enum BillingPairsSeedParseError {
    Json(serde_json::Error),
    UnsupportedSchemaVersion(u32),
    DuplicatePairId(String),
    InvalidPair(String),
}

impl fmt::Display for BillingPairsSeedParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid billing pair seed JSON: {}", error),
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    f,
                    "unsupported billing pair seed schemaVersion: {}",
                    version
                )
            }
            Self::DuplicatePairId(id) => write!(f, "duplicate billing pair id: {}", id),
            Self::InvalidPair(message) => write!(f, "invalid billing pair seed: {}", message),
        }
    }
}

impl Error for BillingPairsSeedParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl BillingPairSeedEntry {
    pub fn id(&self) -> &str {
        match self {
            Self::LegacyRing(seed) => &seed.id,
            Self::RadialReturn(seed) => &seed.id,
        }
    }

    pub fn as_legacy_ring(&self) -> Option<&BillingPairSeed> {
        match self {
            Self::LegacyRing(seed) => Some(seed.as_ref()),
            Self::RadialReturn(_) => None,
        }
    }

    pub fn as_radial_return(&self) -> Option<&RadialReturnBillingPairSeed> {
        match self {
            Self::LegacyRing(_) => None,
            Self::RadialReturn(seed) => Some(seed.as_ref()),
        }
    }
}

impl<'de> Deserialize<'de> for BillingPairSeedEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let pair_kind = value.as_object().and_then(|object| object.get("pairKind"));

        match pair_kind {
            None => serde_json::from_value(value)
                .map(|seed| Self::LegacyRing(Box::new(seed)))
                .map_err(D::Error::custom),
            Some(Value::String(kind)) if kind == "radialReturn" => {
                let seed: RadialReturnBillingPairSeed =
                    serde_json::from_value(value).map_err(D::Error::custom)?;
                seed.validate().map_err(D::Error::custom)?;
                Ok(Self::RadialReturn(Box::new(seed)))
            }
            Some(kind) => Err(D::Error::custom(format!(
                "unsupported billing pair pairKind: {}",
                kind
            ))),
        }
    }
}

impl BillingPairsSeedFileV2 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 2 {
            return Err(format!(
                "schemaVersion must be 2 for BillingPairsSeedFileV2, got {}",
                self.schema_version
            ));
        }

        let mut ids = HashSet::new();
        for entry in &self.billing_pairs {
            if !ids.insert(entry.id()) {
                return Err(format!("duplicate billing pair id: {}", entry.id()));
            }
            if let Some(seed) = entry.as_radial_return() {
                seed.validate()?;
            }
        }
        Ok(())
    }
}

impl RadialReturnBillingPairSeed {
    pub fn validate(&self) -> Result<(), String> {
        self.entry_endpoint.validate()?;
        self.exit_endpoint.validate()?;
        if self.route_plan.anchor.merge_node_id == self.route_plan.anchor.branch_node_id {
            return Err(format!(
                "radial pair {} must keep directed junction M and B as different nodes",
                self.id
            ));
        }
        if self.route_plan.entry_corridor.merge_node_id != self.route_plan.anchor.merge_node_id
            || self.route_plan.entry_corridor.terminal_edge_id
                != self.route_plan.anchor.merge_terminal_edge_id
        {
            return Err(format!(
                "radial pair {} entry corridor does not meet anchor M",
                self.id
            ));
        }
        if self.route_plan.return_corridor.start_node_id != self.route_plan.anchor.branch_node_id {
            return Err(format!(
                "radial pair {} return corridor does not start at anchor B",
                self.id
            ));
        }
        if self.route_plan.mandatory_lap.first_edge_id.is_empty()
            || self.route_plan.mandatory_lap.last_edge_id.is_empty()
        {
            return Err(format!(
                "radial pair {} mandatory lap requires first and last edge IDs",
                self.id
            ));
        }
        if self.route_plan.return_corridor.initial_edge_id.is_empty()
            || self
                .route_plan
                .return_corridor
                .first_general_exit
                .expected_ramp_id
                .is_empty()
        {
            return Err(format!(
                "radial pair {} return corridor requires an initial edge and expected ramp",
                self.id
            ));
        }
        let connector = &self.route_plan.anchor.excluded_short_connector;
        if connector.from_node_id.is_empty()
            || connector.to_node_id.is_empty()
            || connector.osm_way_id <= 0
            || connector.edge_count == 0
            || connector.distance_meters == 0
            || connector.from_node_id == connector.to_node_id
        {
            return Err(format!(
                "radial pair {} has invalid excluded short connector evidence",
                self.id
            ));
        }
        if self.route_plan.mandatory_lap.lap_count != 1 {
            return Err(format!(
                "radial pair {} must declare mandatory lap lapCount=1",
                self.id
            ));
        }
        Ok(())
    }
}

impl DiagnosticEndpoint {
    fn validate(&self) -> Result<(), String> {
        for segment in &self.directed_segments {
            validate_directed_segment(segment)?;
        }

        match self.support_state {
            EndpointSupportState::VerifiedBound => {
                if self.directed_segments.is_empty() {
                    return Err(format!(
                        "endpoint {} with supportState=verified_bound requires non-empty directedSegments",
                        self.ramp_id
                    ));
                }
                if !self.binding_candidates.is_empty() {
                    return Err(format!(
                        "endpoint {} with supportState=verified_bound must not contain bindingCandidates",
                        self.ramp_id
                    ));
                }
            }
            EndpointSupportState::Unresolved | EndpointSupportState::Unsupported => {
                if !self.directed_segments.is_empty() {
                    return Err(format!(
                        "endpoint {} with supportState={} requires empty directedSegments",
                        self.ramp_id,
                        support_state_wire_value(self.support_state)
                    ));
                }
            }
        }

        for candidate in &self.binding_candidates {
            if candidate.directed_segments.is_empty() {
                return Err(format!(
                    "binding candidate {} requires non-empty directedSegments",
                    candidate.candidate_id
                ));
            }
            for segment in &candidate.directed_segments {
                validate_directed_segment(segment)?;
            }
        }
        Ok(())
    }
}

fn validate_directed_segment(segment: &DirectedEndpointSegment) -> Result<(), String> {
    if segment.osm_way_ids.is_empty() || segment.osm_way_ids.iter().any(|way_id| *way_id <= 0) {
        return Err(format!(
            "directed segment {} requires positive OSM way IDs",
            segment.segment_id
        ));
    }
    if segment.edge_ids.is_empty() {
        return Err(format!(
            "directed segment {} requires non-empty edgeIds",
            segment.segment_id
        ));
    }
    if segment.osm_node_ids.len() != segment.edge_ids.len() + 1
        || segment.osm_node_ids.iter().any(|node_id| *node_id <= 0)
    {
        return Err(format!(
            "directed segment {} requires one positive OSM node per Edge boundary",
            segment.segment_id
        ));
    }
    if segment.from_node_id != format!("n:{}", segment.osm_node_ids[0])
        || segment.to_node_id
            != format!("n:{}", segment.osm_node_ids[segment.osm_node_ids.len() - 1])
    {
        return Err(format!(
            "directed segment {} endpoint IDs do not match osmNodeIds order",
            segment.segment_id
        ));
    }
    let mut actual_way_ids = Vec::new();
    for edge_id in &segment.edge_ids {
        let parts = edge_id.split(':').collect::<Vec<_>>();
        let way_id = (parts.first() == Some(&"e"))
            .then_some(())
            .and(parts.get(1))
            .and_then(|value| value.strip_prefix('w'))
            .and_then(|value| value.parse::<i64>().ok())
            .ok_or_else(|| {
                format!(
                    "directed segment {} edge {} has no OSM way identity",
                    segment.segment_id, edge_id
                )
            })?;
        if actual_way_ids.last() != Some(&way_id) {
            actual_way_ids.push(way_id);
        }
    }
    if actual_way_ids != segment.osm_way_ids {
        return Err(format!(
            "directed segment {} OSM way order does not match edgeIds",
            segment.segment_id
        ));
    }
    let encoded = serde_json::to_vec(&segment.edge_ids).map_err(|error| {
        format!(
            "directed segment {} could not be hashed: {}",
            segment.segment_id, error
        )
    })?;
    let expected_hash = format!("{:x}", Sha256::digest(encoded));
    if segment.edge_ids_sha256 != expected_hash {
        return Err(format!(
            "directed segment {} edgeIdsSha256 does not match edgeIds",
            segment.segment_id
        ));
    }
    Ok(())
}

fn support_state_wire_value(state: EndpointSupportState) -> &'static str {
    match state {
        EndpointSupportState::VerifiedBound => "verified_bound",
        EndpointSupportState::Unresolved => "unresolved",
        EndpointSupportState::Unsupported => "unsupported",
    }
}

impl ParsedBillingPairsSeed {
    pub fn schema_version(&self) -> u32 {
        match self {
            Self::Schema1(seed) => seed.schema_version,
            Self::Schema2(seed) => seed.schema_version,
        }
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            Self::Schema1(seed) => seed.description.as_deref(),
            Self::Schema2(seed) => seed.description.as_deref(),
        }
    }

    pub fn legacy_pairs(&self) -> Vec<&BillingPairSeed> {
        match self {
            Self::Schema1(seed) => seed.billing_pairs.iter().collect(),
            Self::Schema2(seed) => seed
                .billing_pairs
                .iter()
                .filter_map(BillingPairSeedEntry::as_legacy_ring)
                .collect(),
        }
    }

    pub fn radial_pairs(&self) -> Vec<&RadialReturnBillingPairSeed> {
        match self {
            Self::Schema1(_) => Vec::new(),
            Self::Schema2(seed) => seed
                .billing_pairs
                .iter()
                .filter_map(BillingPairSeedEntry::as_radial_return)
                .collect(),
        }
    }
}

pub fn parse_billing_pairs_seed(
    raw: &str,
) -> Result<ParsedBillingPairsSeed, BillingPairsSeedParseError> {
    #[derive(Deserialize)]
    struct SchemaVersionHeader {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
    }

    let header: SchemaVersionHeader =
        serde_json::from_str(raw).map_err(BillingPairsSeedParseError::Json)?;
    match header.schema_version {
        1 => {
            let seed: BillingPairsSeedFile =
                serde_json::from_str(raw).map_err(BillingPairsSeedParseError::Json)?;
            validate_unique_ids(seed.billing_pairs.iter().map(|pair| pair.id.as_str()))?;
            Ok(ParsedBillingPairsSeed::Schema1(seed))
        }
        2 => {
            let seed: BillingPairsSeedFileV2 =
                serde_json::from_str(raw).map_err(BillingPairsSeedParseError::Json)?;
            seed.validate()
                .map_err(BillingPairsSeedParseError::InvalidPair)?;
            Ok(ParsedBillingPairsSeed::Schema2(seed))
        }
        version => Err(BillingPairsSeedParseError::UnsupportedSchemaVersion(
            version,
        )),
    }
}

fn validate_unique_ids<'a>(
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), BillingPairsSeedParseError> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(BillingPairsSeedParseError::DuplicatePairId(id.to_string()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const VALID_DIAGNOSTIC_SEED: &str =
        include_str!("../../../fixtures/seed-v2/diagnostic-radial-v2.json");
    const DIAGNOSTIC_SEED_SNAPSHOT: &str =
        include_str!("../../../fixtures/seed-v2/diagnostic-radial-v2.snapshot.json");
    const UNKNOWN_VERSION: &str =
        include_str!("../../../fixtures/seed-v2/invalid-unknown-version.json");
    const LEGACY_UNKNOWN_FIELD: &str =
        include_str!("../../../fixtures/seed-v2/invalid-legacy-unknown-field.json");
    const LEGACY_MISSING_ANCHOR: &str =
        include_str!("../../../fixtures/seed-v2/invalid-legacy-missing-anchor.json");
    const RADIAL_UNKNOWN_FIELD: &str =
        include_str!("../../../fixtures/seed-v2/invalid-radial-unknown-field.json");
    const MISSING_PAIR_KIND: &str =
        include_str!("../../../fixtures/seed-v2/invalid-radial-missing-pair-kind.json");
    const MISSING_ROUTE_PLAN_VERSION: &str =
        include_str!("../../../fixtures/seed-v2/invalid-radial-missing-route-plan-version.json");
    const UNKNOWN_PAIR_KIND: &str =
        include_str!("../../../fixtures/seed-v2/invalid-radial-unknown-pair-kind.json");
    const UNKNOWN_ROUTE_PLAN_VERSION: &str =
        include_str!("../../../fixtures/seed-v2/invalid-radial-unknown-route-plan-version.json");
    const VERIFIED_BOUND_EMPTY: &str =
        include_str!("../../../fixtures/seed-v2/invalid-verified-bound-empty.json");
    const UNRESOLVED_WITH_SEGMENT: &str =
        include_str!("../../../fixtures/seed-v2/invalid-unresolved-directed-segment.json");
    const UNSUPPORTED_WITH_SEGMENT: &str =
        include_str!("../../../fixtures/seed-v2/invalid-unsupported-directed-segment.json");

    #[test]
    fn parses_schema_v2_diagnostic_seed_and_matches_snapshot() {
        let parsed = parse_billing_pairs_seed(VALID_DIAGNOSTIC_SEED).unwrap();
        let seed = match &parsed {
            ParsedBillingPairsSeed::Schema2(seed) => seed,
            ParsedBillingPairsSeed::Schema1(_) => panic!("expected schema 2"),
        };

        assert_eq!(parsed.schema_version(), 2);
        assert_eq!(seed.billing_pairs.len(), 3);
        assert!(matches!(
            seed.billing_pairs[0],
            BillingPairSeedEntry::LegacyRing(_)
        ));
        assert_eq!(
            seed.billing_pairs[1].id(),
            "bp:2-inbound:meguro:c1-inner:tengenji"
        );
        assert_eq!(
            seed.billing_pairs[2].id(),
            "bp:2-inbound:meguro:c1-outer:tengenji"
        );
        assert_eq!(parsed.legacy_pairs().len(), 1);
        assert_eq!(parsed.radial_pairs().len(), 2);

        let mut serialized = serde_json::to_string_pretty(&parsed).unwrap();
        serialized.push('\n');
        assert_eq!(serialized, DIAGNOSTIC_SEED_SNAPSHOT);
    }

    #[test]
    fn rejects_version_kind_required_field_and_unknown_field_fixtures() {
        match parse_billing_pairs_seed(UNKNOWN_VERSION) {
            Err(BillingPairsSeedParseError::UnsupportedSchemaVersion(3)) => {}
            other => panic!("expected unsupported schemaVersion error, got {:?}", other),
        }

        for raw in [
            LEGACY_UNKNOWN_FIELD,
            LEGACY_MISSING_ANCHOR,
            RADIAL_UNKNOWN_FIELD,
            MISSING_PAIR_KIND,
            MISSING_ROUTE_PLAN_VERSION,
            UNKNOWN_PAIR_KIND,
            UNKNOWN_ROUTE_PLAN_VERSION,
        ] {
            assert!(
                parse_billing_pairs_seed(raw).is_err(),
                "invalid fixture was accepted: {}",
                raw
            );
        }
    }

    #[test]
    fn rejects_invalid_endpoint_support_invariants() {
        for raw in [
            VERIFIED_BOUND_EMPTY,
            UNRESOLVED_WITH_SEGMENT,
            UNSUPPORTED_WITH_SEGMENT,
        ] {
            assert!(
                parse_billing_pairs_seed(raw).is_err(),
                "invalid support fixture was accepted: {}",
                raw
            );
        }

        for state in [
            EndpointSupportState::Unresolved,
            EndpointSupportState::Unsupported,
        ] {
            let mut value: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
            value["billingPairs"][1]["exitEndpoint"]["supportState"] =
                json!(support_state_wire_value(state));
            value["billingPairs"][1]["exitEndpoint"]["directedSegments"] = json!([]);
            let raw = serde_json::to_string(&value).unwrap();
            assert!(parse_billing_pairs_seed(&raw).is_ok());
        }
    }

    #[test]
    fn rejects_missing_or_mismatched_osm_node_evidence() {
        let mut missing: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
        missing["billingPairs"][1]["exitEndpoint"]["directedSegments"][0]["osmNodeIds"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(parse_billing_pairs_seed(&missing.to_string()).is_err());

        let mut mismatched: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
        mismatched["billingPairs"][1]["exitEndpoint"]["directedSegments"][0]["fromNodeId"] =
            json!("n:0");
        assert!(parse_billing_pairs_seed(&mismatched.to_string()).is_err());
    }

    #[test]
    fn rejects_directed_junction_boundary_mismatches() {
        let mut value: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
        value["billingPairs"][1]["routePlan"]["anchor"]["mergeNodeId"] =
            json!(value["billingPairs"][1]["routePlan"]["anchor"]["branchNodeId"]);
        assert!(parse_billing_pairs_seed(&value.to_string()).is_err());

        let mut value: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
        value["billingPairs"][1]["routePlan"]["entryCorridor"]["mergeNodeId"] = json!("mismatch");
        assert!(parse_billing_pairs_seed(&value.to_string()).is_err());

        let mut value: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
        value["billingPairs"][1]["routePlan"]["returnCorridor"]["startNodeId"] = json!("mismatch");
        assert!(parse_billing_pairs_seed(&value.to_string()).is_err());
    }

    #[test]
    fn rejects_unknown_fields_in_every_nested_seed_object() {
        let paths: &[&[&str]] = &[
            &["billingPairs", "0", "provenance"],
            &["billingPairs", "0", "prices", "0"],
            &["billingPairs", "1", "entryEndpoint"],
            &[
                "billingPairs",
                "1",
                "entryEndpoint",
                "directedSegments",
                "0",
            ],
            &["billingPairs", "1", "exitEndpoint", "directedSegments", "0"],
            &["billingPairs", "1", "routePlan"],
            &["billingPairs", "1", "routePlan", "entryCorridor"],
            &["billingPairs", "1", "routePlan", "anchor"],
            &[
                "billingPairs",
                "1",
                "routePlan",
                "anchor",
                "excludedShortConnector",
            ],
            &["billingPairs", "1", "routePlan", "mandatoryLap"],
            &["billingPairs", "1", "routePlan", "returnCorridor"],
            &[
                "billingPairs",
                "1",
                "routePlan",
                "returnCorridor",
                "firstGeneralExit",
            ],
            &["billingPairs", "1", "pairEligibility"],
            &["billingPairs", "1", "loopValidation"],
            &["billingPairs", "1", "tariff"],
            &["billingPairs", "1", "provenance"],
        ];

        for path in paths {
            let mut value: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
            add_unknown_field(&mut value, path);
            let raw = serde_json::to_string(&value).unwrap();
            let error = parse_billing_pairs_seed(&raw).unwrap_err().to_string();
            assert!(
                error.contains("unknown field"),
                "path {:?} produced unexpected error: {}",
                path,
                error
            );
        }
    }

    #[test]
    fn rejects_unknown_top_level_fields_and_schema_v2_pair_fields_in_schema_v1() {
        let mut schema1: Value =
            serde_json::from_str(include_str!("../../../data/billing-pairs-seed.json")).unwrap();
        add_unknown_field(&mut schema1, &[]);
        let error = parse_billing_pairs_seed(&schema1.to_string())
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"));

        let mut schema2: Value = serde_json::from_str(VALID_DIAGNOSTIC_SEED).unwrap();
        add_unknown_field(&mut schema2, &[]);
        let error = parse_billing_pairs_seed(&schema2.to_string())
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"));

        let diagnostic_pair = &schema2["billingPairs"][1];
        let schema1_with_radial = json!({
            "schemaVersion": 1,
            "billingPairs": [diagnostic_pair]
        });
        assert!(parse_billing_pairs_seed(&schema1_with_radial.to_string()).is_err());
    }

    #[test]
    fn schema_2_preserves_c1_seed_data_and_integrates_diagnostic_radial_pairs() {
        let raw = include_str!("../../../data/billing-pairs-seed.json");
        let parsed = parse_billing_pairs_seed(raw).unwrap();
        let original: Value = serde_json::from_str(raw).unwrap();
        let serialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(serialized, original);

        let seed = match &parsed {
            ParsedBillingPairsSeed::Schema2(seed) => seed,
            ParsedBillingPairsSeed::Schema1(_) => panic!("expected schema 2"),
        };
        assert_eq!(parsed.schema_version(), 2);
        assert_eq!(seed.billing_pairs.len(), 10);
        let legacy_pairs: Vec<_> = seed
            .billing_pairs
            .iter()
            .filter_map(BillingPairSeedEntry::as_legacy_ring)
            .collect();
        assert_eq!(legacy_pairs.len(), 8);
        assert_eq!(
            legacy_pairs
                .iter()
                .filter(|pair| pair.status == VerificationStatus::Verified)
                .count(),
            7
        );
        assert_eq!(
            legacy_pairs
                .iter()
                .filter(|pair| pair.status == VerificationStatus::Unverified)
                .count(),
            1
        );
        assert!(legacy_pairs
            .iter()
            .filter(|pair| pair.id != "bp:c1-outer:kasumigaseki-daikancho")
            .all(|pair| pair.prices.len() == 2
                && pair.prices.iter().all(|price| price.amount_yen == 300)));
        let corrected_pair = legacy_pairs
            .iter()
            .find(|pair| pair.id == "bp:c1-outer:kasumigaseki-daikancho")
            .unwrap();
        assert_eq!(corrected_pair.prices.len(), 2);
        assert_eq!(corrected_pair.prices[0].amount_yen, 570);
        assert_eq!(
            corrected_pair.prices[0].effective_to.as_deref(),
            Some("2026-09-30T15:00:00Z")
        );
        assert_eq!(corrected_pair.prices[1].amount_yen, 300);
        assert_eq!(
            corrected_pair.prices[1].effective_from,
            "2026-09-30T15:00:00Z"
        );

        let radial_pairs: Vec<_> = seed
            .billing_pairs
            .iter()
            .filter_map(BillingPairSeedEntry::as_radial_return)
            .collect();
        assert_eq!(radial_pairs.len(), 2);
        assert_eq!(
            radial_pairs
                .iter()
                .map(|pair| pair.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "bp:2-inbound:meguro:c1-inner:tengenji",
                "bp:2-inbound:meguro:c1-outer:tengenji"
            ]
        );
        for pair in &radial_pairs {
            assert_eq!(pair.pair_kind, PairKind::RadialReturn);
            assert_eq!(pair.route_plan_version, RoutePlanVersion::V1);
            assert_eq!(
                pair.entry_endpoint.support_state,
                EndpointSupportState::VerifiedBound
            );
            assert_eq!(
                pair.exit_endpoint.support_state,
                EndpointSupportState::VerifiedBound
            );
            assert_eq!(
                pair.pair_eligibility.status,
                PairEligibilityStatus::VerifiedOneSectionAhead
            );
            assert!(pair.pair_eligibility.one_section_ahead_verified);
            assert_eq!(pair.tariff.status, TariffStatus::Priced);
            assert_eq!(pair.tariff.amount_yen, Some(790));
            assert_eq!(pair.tariff.billing_distance_meters, Some(19400));
            assert_eq!(pair.tariff.prices.len(), 2);
            assert_eq!(pair.tariff.prices[1].amount_yen, 860);
            assert_eq!(pair.exit_endpoint.binding_candidates.len(), 0);
            let exit_segment = &pair.exit_endpoint.directed_segments[0];
            assert_eq!(exit_segment.osm_way_ids.len(), 4);
            assert_eq!(exit_segment.osm_node_ids.len(), 17);
            assert_eq!(exit_segment.edge_ids.len(), 16);
            assert_eq!(
                exit_segment.edge_ids_sha256,
                "bb9114f49d64b952b58b5a2ef53679a6007bea48a51671ade34c56b0325fa7cd"
            );
        }
        assert_eq!(
            radial_pairs[0].route_plan.anchor.merge_node_id,
            "n:574460576"
        );
        assert_eq!(
            radial_pairs[0].route_plan.anchor.branch_node_id,
            "n:574460605"
        );
        assert_eq!(
            radial_pairs[1].route_plan.anchor.merge_node_id,
            "n:31297008"
        );
        assert_eq!(
            radial_pairs[1].route_plan.anchor.branch_node_id,
            "n:31297000"
        );
    }

    #[test]
    fn rejects_duplicate_ids_in_both_schema_versions() {
        let pair = json!({
            "id": "fixture:duplicate",
            "entryOsmWayId": 1,
            "exitOsmWayId": 2,
            "anchorOsmNodeId": 3,
            "vehicleProfile": "passenger-car-etc",
            "status": "unverified",
            "oneSectionAheadVerified": false,
            "provenance": {
                "source": "https://example.com",
                "sourceDate": "2026-09-16"
            },
            "prices": []
        });

        let schema1 = json!({
            "schemaVersion": 1,
            "billingPairs": [pair, pair]
        });
        assert!(parse_billing_pairs_seed(&schema1.to_string()).is_err());

        let schema2 = json!({
            "schemaVersion": 2,
            "billingPairs": [pair, pair]
        });
        assert!(parse_billing_pairs_seed(&schema2.to_string()).is_err());
    }

    fn add_unknown_field(value: &mut Value, path: &[&str]) {
        let mut current = value;
        for component in path {
            current = match component.parse::<usize>() {
                Ok(index) => current.as_array_mut().unwrap().get_mut(index).unwrap(),
                Err(_) => current
                    .as_object_mut()
                    .unwrap()
                    .get_mut(*component)
                    .unwrap(),
            };
        }
        current
            .as_object_mut()
            .unwrap()
            .insert("futureField".into(), Value::Bool(true));
    }
}
