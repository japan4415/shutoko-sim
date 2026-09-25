use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub const TARIFF_MODEL_VERSION: u32 = 1;
pub const PRODUCT_VEHICLE_PROFILE: &str = "passenger-car-etc";
pub const PRODUCT_VEHICLE_CLASS: &str = "ordinary";
pub const PRODUCT_PAYMENT_METHOD: &str = "etc";
pub const PRODUCT_FARE_BASIS: &str = "base_toll_excluding_discounts";
pub const PRODUCT_FARE_LABEL: &str = "普通車ETC基本料金（割引適用前）";
pub const OFFICIAL_DISTANCE_RULE_SOURCE: &str = "official_distance_rule";
pub const LEGACY_TARIFF_SOURCE: &str = "table";
pub const DISTANCE_UNIT_METERS: u64 = 100;
pub const DISTANCE_QUANTUM_METERS: u64 = 100;
pub const YEN_MICROS: u128 = 1_000_000;
pub const PRODUCT_DISCOUNTS_EXCLUDED: [&str; 5] = [
    "midnight_discount",
    "central_tokyo_inflow_discount",
    "environmental_road_pricing_discount",
    "etc2_discount",
    "frequent_user_discount",
];

pub type TariffRule = TariffRuleV3;
pub type TariffRuleV1 = TariffRuleV3;
pub type TariffAssignment = TariffAssignmentV3;
pub type TariffAssignmentV1 = TariffAssignmentV3;
pub type TariffPrice = TariffPriceV3;
pub type TariffPriceV1 = TariffPriceV3;
pub type BillingDistanceEvidenceV1 = DistanceEvidenceV3;
pub type BillingDistanceEvidenceV3 = DistanceEvidenceV3;
pub type TariffFileV3 = OdTariffsFileV3;
pub type OdTariffsFile = OdTariffsFileV3;
pub type TariffResolverError = TariffError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TariffError {
    pub code: String,
    pub message: String,
}

impl TariffError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for TariffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TariffError {}

fn error(code: &str, message: impl Into<String>) -> TariffError {
    TariffError::new(code, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TariffResolutionStatus {
    Priced,
    Unpriced,
    Expired,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TariffRoundingV3 {
    pub mode: String,
    pub multiple_yen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TariffSourceRefV3 {
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub status: Option<String>,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TariffDocumentV3 {
    pub document_id: String,
    pub edition: String,
    pub url: String,
    pub cache_path: String,
    pub document_sha256: Option<String>,
    pub status: String,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    #[serde(default, alias = "reviewMethods")]
    pub review_method: Vec<String>,
    #[serde(default)]
    pub manual_acquisition_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TariffRuleV3 {
    pub rule_id: String,
    pub vehicle_class: String,
    pub payment_method: String,
    pub fare_basis: String,
    #[serde(default)]
    pub discounts_excluded: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DistanceEvidenceV3 {
    pub evidence_id: String,
    pub document_id: String,
    pub edition: String,
    pub document_sha256: String,
    pub page: u64,
    pub route_label: String,
    pub row_label: String,
    pub column_label: String,
    pub cell: String,
    pub fare_variant: String,
    pub ink: String,
    pub distance_meters: u64,
    pub distance_label: String,
    pub observed_base_fare_yen: u64,
    pub calculated_base_fare_yen: u64,
    pub entry_ramp_id: String,
    pub exit_ramp_id: String,
    pub url: String,
    pub reviewed_at: String,
    #[serde(default, alias = "reviewMethods")]
    pub review_method: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingEvidenceV3 {
    pub evidence_id: String,
    pub document_id: String,
    pub edition: String,
    pub page: Option<u64>,
    pub row_label: String,
    pub column_label: String,
    pub cell: Option<String>,
    pub fare_variant: String,
    pub status: String,
    pub observed_base_fare_yen: Option<u64>,
    pub observed_distance_meters: Option<u64>,
    pub reviewed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    #[serde(default)]
    pub endpoint_binding_status: Option<String>,
    #[serde(default)]
    pub endpoint_binding_reason: Option<String>,
    pub prices: Vec<TariffPriceV3>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OdTariffsFileV3 {
    pub version: u32,
    pub source: String,
    pub source_date: String,
    pub vehicle_profile: String,
    pub vehicle_class: String,
    pub payment_method: String,
    pub fare_basis: String,
    pub fare_label: String,
    pub discounts_excluded: Vec<String>,
    pub documents: Vec<TariffDocumentV3>,
    pub tariff_rules: Vec<TariffRuleV3>,
    pub distance_evidence: Vec<DistanceEvidenceV3>,
    pub pending_evidence: Vec<PendingEvidenceV3>,
    pub assignments: Vec<TariffAssignmentV3>,
    #[serde(default)]
    pub verified_od_pairs: Vec<crate::OdTariff>,
    #[serde(default)]
    pub deprecated_assignments: Vec<serde_json::Value>,
    #[serde(default)]
    pub migration: Option<serde_json::Value>,
    #[serde(default)]
    pub pending_resolution: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TariffCatalog {
    pub version: u32,
    pub source: String,
    pub source_date: String,
    pub vehicle_profile: String,
    pub vehicle_class: String,
    pub payment_method: String,
    pub fare_basis: String,
    pub fare_label: String,
    pub discounts_excluded: Vec<String>,
    pub documents: Vec<TariffDocumentV3>,
    pub tariff_rules: Vec<TariffRuleV3>,
    pub distance_evidence: Vec<DistanceEvidenceV3>,
    pub pending_evidence: Vec<PendingEvidenceV3>,
    pub assignments: Vec<TariffAssignmentV3>,
}

impl OdTariffsFileV3 {
    pub fn validate(&self) -> Result<(), TariffError> {
        let catalog = TariffCatalog::from_file(self.clone());
        catalog.validate()
    }

    pub fn into_catalog(self) -> TariffCatalog {
        TariffCatalog::from_file(self)
    }
}

impl TariffCatalog {
    pub fn from_file(file: OdTariffsFileV3) -> Self {
        Self {
            version: file.version,
            source: file.source,
            source_date: file.source_date,
            vehicle_profile: file.vehicle_profile,
            vehicle_class: file.vehicle_class,
            payment_method: file.payment_method,
            fare_basis: file.fare_basis,
            fare_label: file.fare_label,
            discounts_excluded: file.discounts_excluded,
            documents: file.documents,
            tariff_rules: file.tariff_rules,
            distance_evidence: file.distance_evidence,
            pending_evidence: file.pending_evidence,
            assignments: file.assignments,
        }
    }

    pub fn validate(&self) -> Result<(), TariffError> {
        if self.version != 3 {
            return Err(error(
                "TARIFF_VERSION_UNSUPPORTED",
                "tariff version must be 3",
            ));
        }
        if self.source.is_empty() || self.source_date.is_empty() {
            return Err(error(
                "TARIFF_ROOT_INVALID",
                "tariff source metadata is required",
            ));
        }
        validate_scope(
            &self.vehicle_profile,
            &self.vehicle_class,
            &self.payment_method,
            &self.fare_basis,
            &self.fare_label,
            &self.discounts_excluded,
        )?;
        if self.tariff_rules.is_empty() {
            return Err(error(
                "TARIFF_RULES_EMPTY",
                "at least one tariff rule is required",
            ));
        }
        if self.assignments.is_empty() {
            return Err(error(
                "TARIFF_ASSIGNMENTS_EMPTY",
                "at least one tariff assignment is required",
            ));
        }
        let documents = self
            .documents
            .iter()
            .map(|document| (document.document_id.as_str(), document))
            .collect::<HashMap<_, _>>();
        if documents.len() != self.documents.len() {
            return Err(error(
                "TARIFF_DOCUMENT_DUPLICATE",
                "tariff documentId is duplicated",
            ));
        }
        for document in &self.documents {
            if document.document_id.is_empty()
                || document.edition.is_empty()
                || document.url.is_empty()
                || document.cache_path.is_empty()
                || document.status.is_empty()
            {
                return Err(error(
                    "TARIFF_DOCUMENT_INVALID",
                    "tariff document metadata is incomplete",
                ));
            }
            if document.status.starts_with("verified") && document.document_sha256.is_none() {
                return Err(error(
                    "TARIFF_DOCUMENT_HASH_MISSING",
                    format!("verified document {} has no SHA-256", document.document_id),
                ));
            }
            if let Some(hash) = &document.document_sha256 {
                validate_sha256(hash)?;
            }
        }

        let mut rule_by_id = HashMap::new();
        let mut rules_by_scope: BTreeMap<(&str, &str, &str), Vec<&TariffRuleV3>> = BTreeMap::new();
        for rule in &self.tariff_rules {
            if rule_by_id.insert(rule.rule_id.as_str(), rule).is_some() {
                return Err(error(
                    "TARIFF_RULE_DUPLICATE",
                    format!("ruleId {} is duplicated", rule.rule_id),
                ));
            }
            validate_rule(rule)?;
            if rule.source_refs.is_empty() {
                return Err(error(
                    "TARIFF_RULE_SOURCE_MISSING",
                    format!("rule {} has no sourceRefs", rule.rule_id),
                ));
            }
            for source in &rule.source_refs {
                if source.location.is_empty() {
                    return Err(error(
                        "TARIFF_RULE_SOURCE_INVALID",
                        format!("rule {} has an empty source location", rule.rule_id),
                    ));
                }
                if let Some(document_id) = &source.document_id {
                    if !documents.contains_key(document_id.as_str()) {
                        return Err(error(
                            "TARIFF_RULE_DOCUMENT_UNKNOWN",
                            format!(
                                "rule {} references unknown document {}",
                                rule.rule_id, document_id
                            ),
                        ));
                    }
                } else if source.source.as_deref().is_none_or(str::is_empty) {
                    return Err(error(
                        "TARIFF_RULE_SOURCE_INVALID",
                        format!("rule {} has no documentId or source", rule.rule_id),
                    ));
                }
            }
            rules_by_scope
                .entry((
                    rule.vehicle_class.as_str(),
                    rule.payment_method.as_str(),
                    rule.fare_basis.as_str(),
                ))
                .or_default()
                .push(rule);
        }
        for rules in rules_by_scope.values_mut() {
            rules.sort_by(|left, right| left.effective_from.cmp(&right.effective_from));
            for pair in rules.windows(2) {
                let previous_end = parse_timestamp(pair[0].effective_to.as_deref().unwrap_or(""))?
                    .ok_or_else(|| {
                        error(
                            "TARIFF_RULE_OPEN_INTERVAL",
                            "an open tariff interval is followed by another rule",
                        )
                    })?;
                let next_start = parse_timestamp(&pair[1].effective_from)?
                    .ok_or_else(|| error("TARIFF_INTERVAL_INVALID", "rule has no effectiveFrom"))?;
                if previous_end == next_start {
                } else if previous_end > next_start {
                    return Err(error(
                        "TARIFF_RULE_OVERLAP",
                        "tariff rule intervals overlap",
                    ));
                } else {
                    return Err(error(
                        "TARIFF_RULE_GAP",
                        "tariff rule intervals contain a gap",
                    ));
                }
            }
            let last = rules.last().expect("non-empty rules");
            if last.effective_to.is_some() {
                return Err(error(
                    "TARIFF_RULE_FINAL_OPEN",
                    "the final tariff interval must be open",
                ));
            }
        }

        let mut evidence_ids = HashSet::new();
        let mut distance_by_id = HashMap::new();
        for evidence in &self.distance_evidence {
            if !evidence_ids.insert(evidence.evidence_id.as_str()) {
                return Err(error(
                    "TARIFF_EVIDENCE_DUPLICATE",
                    format!("evidenceId {} is duplicated", evidence.evidence_id),
                ));
            }
            distance_by_id.insert(evidence.evidence_id.as_str(), evidence);
            let Some(document) = documents.get(evidence.document_id.as_str()) else {
                return Err(error(
                    "TARIFF_EVIDENCE_DOCUMENT_UNKNOWN",
                    format!(
                        "evidence {} references an unknown document",
                        evidence.evidence_id
                    ),
                ));
            };
            if document.edition != evidence.edition
                || document.document_sha256.as_deref() != Some(evidence.document_sha256.as_str())
            {
                return Err(error(
                    "TARIFF_EVIDENCE_DOCUMENT_MISMATCH",
                    format!(
                        "evidence {} does not match its document",
                        evidence.evidence_id
                    ),
                ));
            }
            if evidence.page == 0
                || evidence.route_label.is_empty()
                || evidence.row_label.is_empty()
                || evidence.column_label.is_empty()
                || evidence.cell.is_empty()
                || evidence.fare_variant != "base_etc"
                || evidence.ink.is_empty()
                || evidence.entry_ramp_id.is_empty()
                || evidence.exit_ramp_id.is_empty()
                || evidence.url.is_empty()
                || evidence.reviewed_at.is_empty()
                || evidence.review_method.is_empty()
            {
                return Err(error(
                    "TARIFF_EVIDENCE_INCOMPLETE",
                    format!("evidence {} is incomplete", evidence.evidence_id),
                ));
            }
            if evidence.distance_meters == 0
                || evidence.distance_meters % DISTANCE_QUANTUM_METERS != 0
                || evidence.observed_base_fare_yen == 0
                || evidence.observed_base_fare_yen != evidence.calculated_base_fare_yen
            {
                return Err(error(
                    "TARIFF_EVIDENCE_INVALID",
                    format!(
                        "evidence {} has invalid distance or fare",
                        evidence.evidence_id
                    ),
                ));
            }
            if let Some(rule) = self.tariff_rules.iter().find(|rule| {
                rule.source_refs.iter().any(|source| {
                    source.document_id.as_deref() == Some(evidence.document_id.as_str())
                })
            }) {
                if calculate_tariff_yen(evidence.distance_meters, rule)?
                    != evidence.observed_base_fare_yen
                {
                    return Err(error(
                        "TARIFF_EVIDENCE_CALCULATION_MISMATCH",
                        format!(
                            "evidence {} does not match its tariff rule",
                            evidence.evidence_id
                        ),
                    ));
                }
            }
        }

        let mut pending_by_id = HashMap::new();
        for evidence in &self.pending_evidence {
            if !evidence_ids.insert(evidence.evidence_id.as_str()) {
                return Err(error(
                    "TARIFF_EVIDENCE_DUPLICATE",
                    format!("evidenceId {} is reused", evidence.evidence_id),
                ));
            }
            pending_by_id.insert(evidence.evidence_id.as_str(), evidence);
            let Some(document) = documents.get(evidence.document_id.as_str()) else {
                return Err(error(
                    "TARIFF_EVIDENCE_DOCUMENT_UNKNOWN",
                    format!(
                        "pending evidence {} references an unknown document",
                        evidence.evidence_id
                    ),
                ));
            };
            if document.edition != evidence.edition
                || evidence.status != "pending_manual_pdf_review"
                || evidence.page.is_some()
                || evidence.cell.is_some()
                || evidence.observed_base_fare_yen.is_some()
                || evidence.observed_distance_meters.is_some()
                || evidence.reviewed_at.is_some()
            {
                return Err(error(
                    "TARIFF_PENDING_EVIDENCE_INVALID",
                    format!("pending evidence {} is not unpriced", evidence.evidence_id),
                ));
            }
        }

        let mut assignment_ids = HashSet::new();
        let mut od_keys = HashSet::new();
        for assignment in &self.assignments {
            if !assignment_ids.insert(assignment.assignment_id.as_str())
                || !od_keys.insert(assignment.od_key.as_str())
            {
                return Err(error(
                    "TARIFF_ASSIGNMENT_DUPLICATE",
                    "assignmentId or odKey is duplicated",
                ));
            }
            if assignment.pair_ids.is_empty()
                || assignment.pair_ids.iter().any(String::is_empty)
                || assignment.pair_ids.iter().collect::<HashSet<_>>().len()
                    != assignment.pair_ids.len()
                || assignment.entry_name.is_empty()
                || assignment.exit_name.is_empty()
                || assignment.verification_status.is_empty()
            {
                return Err(error(
                    "TARIFF_ASSIGNMENT_INVALID",
                    format!("assignment {} is invalid", assignment.assignment_id),
                ));
            }
            validate_scope(
                &assignment.vehicle_profile,
                &assignment.vehicle_class,
                &assignment.payment_method,
                &assignment.fare_basis,
                PRODUCT_FARE_LABEL,
                &self.discounts_excluded,
            )?;
            if assignment.billing_distance_meters == 0
                || assignment.billing_distance_meters % DISTANCE_QUANTUM_METERS != 0
            {
                return Err(error(
                    "TARIFF_ASSIGNMENT_DISTANCE_INVALID",
                    format!(
                        "assignment {} has an invalid billing distance",
                        assignment.assignment_id
                    ),
                ));
            }
            let Some(base_evidence) = distance_by_id.get(assignment.distance_evidence_id.as_str())
            else {
                return Err(error(
                    "TARIFF_ASSIGNMENT_EVIDENCE_UNKNOWN",
                    format!(
                        "assignment {} references unknown distance evidence",
                        assignment.assignment_id
                    ),
                ));
            };
            if base_evidence.entry_ramp_id != assignment.entry_ramp_id
                || base_evidence.exit_ramp_id != assignment.exit_ramp_id
                || base_evidence.distance_meters != assignment.billing_distance_meters
            {
                return Err(error(
                    "TARIFF_ASSIGNMENT_EVIDENCE_MISMATCH",
                    format!(
                        "assignment {} does not match its distance evidence",
                        assignment.assignment_id
                    ),
                ));
            }

            let expected_rule_ids = self
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
                return Err(error(
                    "TARIFF_ASSIGNMENT_PRICE_COUNT",
                    format!(
                        "assignment {} does not cover every rule",
                        assignment.assignment_id
                    ),
                ));
            }
            let mut used_rules = HashSet::new();
            for price in &assignment.prices {
                if !used_rules.insert(price.rule_id.as_str()) {
                    return Err(error(
                        "TARIFF_ASSIGNMENT_RULE_DUPLICATE",
                        format!("assignment {} repeats a rule", assignment.assignment_id),
                    ));
                }
                if !expected_rule_ids.contains(price.rule_id.as_str()) {
                    return Err(error(
                        "TARIFF_ASSIGNMENT_RULE_UNKNOWN",
                        format!(
                            "assignment {} references an out-of-scope rule",
                            assignment.assignment_id
                        ),
                    ));
                }
                let Some(rule) = rule_by_id.get(price.rule_id.as_str()).copied() else {
                    return Err(error(
                        "TARIFF_ASSIGNMENT_RULE_UNKNOWN",
                        format!(
                            "assignment {} references an unknown rule",
                            assignment.assignment_id
                        ),
                    ));
                };
                if price.effective_from != rule.effective_from
                    || price.effective_to != rule.effective_to
                {
                    return Err(error(
                        "TARIFF_ASSIGNMENT_INTERVAL_MISMATCH",
                        format!(
                            "assignment {} price interval does not match its rule",
                            assignment.assignment_id
                        ),
                    ));
                }
                match price.status.as_str() {
                    "priced" => {
                        let Some(evidence) =
                            distance_by_id.get(price.evidence_id.as_str()).copied()
                        else {
                            return Err(error(
                                "TARIFF_PRICE_EVIDENCE_UNKNOWN",
                                format!(
                                    "assignment {} references unknown price evidence",
                                    assignment.assignment_id
                                ),
                            ));
                        };
                        if price.evidence_id != price.distance_evidence_id
                            || evidence.evidence_id != price.distance_evidence_id
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
                            || calculate_tariff_yen(
                                price
                                    .observed_distance_meters
                                    .unwrap_or(assignment.billing_distance_meters),
                                rule,
                            )? != evidence.observed_base_fare_yen
                        {
                            return Err(error(
                                "TARIFF_PRICE_EVIDENCE_MISMATCH",
                                format!(
                                    "assignment {} priced record does not match its evidence",
                                    assignment.assignment_id
                                ),
                            ));
                        }
                    }
                    "pending_pdf_review" | "unpriced" => {
                        if price.tariff_status != "unpriced"
                            || price.amount_yen.is_some()
                            || price.observed_base_fare_yen.is_some()
                            || price.observed_distance_meters.is_some()
                        {
                            return Err(error(
                                "TARIFF_PRICE_PENDING_INVALID",
                                format!(
                                    "assignment {} pending record is not fully unpriced",
                                    assignment.assignment_id
                                ),
                            ));
                        }
                        let Some(pending_evidence) =
                            pending_by_id.get(price.evidence_id.as_str()).copied()
                        else {
                            return Err(error(
                                "TARIFF_PRICE_EVIDENCE_UNKNOWN",
                                format!(
                                    "assignment {} pending evidence is missing",
                                    assignment.assignment_id
                                ),
                            ));
                        };
                        if price.evidence_id != price.distance_evidence_id
                            || !pending_evidence_matches_assignment(assignment, pending_evidence)
                            || !rule_references_evidence_document(
                                rule,
                                pending_evidence.document_id.as_str(),
                                pending_evidence.edition.as_str(),
                                None,
                            )
                        {
                            return Err(error(
                                "TARIFF_PRICE_EVIDENCE_MISMATCH",
                                format!(
                                    "assignment {} pending evidence does not match its OD or rule",
                                    assignment.assignment_id
                                ),
                            ));
                        }
                    }
                    _ => {
                        return Err(error(
                            "TARIFF_PRICE_STATUS_UNKNOWN",
                            format!(
                                "assignment {} has an unknown price status",
                                assignment.assignment_id
                            ),
                        ));
                    }
                }
            }
            if expected_rule_ids.difference(&used_rules).next().is_some() {
                return Err(error(
                    "TARIFF_ASSIGNMENT_RULE_MISSING",
                    format!("assignment {} is missing a rule", assignment.assignment_id),
                ));
            }
        }
        Ok(())
    }

    pub fn from_json(input: &str) -> Result<Self, TariffError> {
        read_tariff_v3(input)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &str) -> Result<Self, TariffError> {
        read_tariff_v3(input)
    }

    pub fn resolver(&self) -> Result<TariffResolver, TariffError> {
        TariffResolver::new(self.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TariffScope {
    pub vehicle_profile: String,
    pub vehicle_class: String,
    pub payment_method: String,
    pub fare_basis: String,
    pub discounts_excluded: bool,
}

impl Default for TariffScope {
    fn default() -> Self {
        Self {
            vehicle_profile: PRODUCT_VEHICLE_PROFILE.to_owned(),
            vehicle_class: PRODUCT_VEHICLE_CLASS.to_owned(),
            payment_method: PRODUCT_PAYMENT_METHOD.to_owned(),
            fare_basis: PRODUCT_FARE_BASIS.to_owned(),
            discounts_excluded: true,
        }
    }
}

impl TariffScope {
    pub fn product() -> Self {
        Self::default()
    }

    pub fn validate(&self) -> Result<(), TariffError> {
        if self.vehicle_profile != PRODUCT_VEHICLE_PROFILE
            || self.vehicle_class != PRODUCT_VEHICLE_CLASS
            || self.payment_method != PRODUCT_PAYMENT_METHOD
            || self.fare_basis != PRODUCT_FARE_BASIS
            || !self.discounts_excluded
        {
            return Err(error(
                "TARIFF_SCOPE_MISMATCH",
                "vehicle, payment, fare basis, or discount scope is unsupported",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedTariff {
    pub status: TariffResolutionStatus,
    pub amount_yen: Option<u64>,
    pub billing_distance_meters: Option<u64>,
    pub effective_from: Option<String>,
    pub effective_to: Option<String>,
    pub rule_id: Option<String>,
    pub evidence_id: Option<String>,
    pub distance_evidence_id: Option<String>,
    pub assignment_id: Option<String>,
    pub fare_label: Option<String>,
    pub vehicle_class: Option<String>,
    pub payment_method: Option<String>,
    pub fare_basis: Option<String>,
    pub discounts_excluded: bool,
    pub toll_source: Option<String>,
}

impl ResolvedTariff {
    pub fn unpriced() -> Self {
        Self {
            status: TariffResolutionStatus::Unpriced,
            amount_yen: None,
            billing_distance_meters: None,
            effective_from: None,
            effective_to: None,
            rule_id: None,
            evidence_id: None,
            distance_evidence_id: None,
            assignment_id: None,
            fare_label: Some(PRODUCT_FARE_LABEL.to_owned()),
            vehicle_class: Some(PRODUCT_VEHICLE_CLASS.to_owned()),
            payment_method: Some(PRODUCT_PAYMENT_METHOD.to_owned()),
            fare_basis: Some(PRODUCT_FARE_BASIS.to_owned()),
            discounts_excluded: true,
            toll_source: None,
        }
    }

    pub fn is_priced(&self) -> bool {
        self.status == TariffResolutionStatus::Priced
    }
}

#[derive(Debug, Clone)]
pub struct TariffResolver {
    catalog: TariffCatalog,
}

impl TariffResolver {
    pub fn new(catalog: TariffCatalog) -> Result<Self, TariffError> {
        catalog.validate()?;
        Ok(Self { catalog })
    }

    pub fn from_json(input: &str) -> Result<Self, TariffError> {
        let catalog = read_tariff_v3(input)?;
        Self::new(catalog)
    }

    pub fn catalog(&self) -> &TariffCatalog {
        &self.catalog
    }

    pub fn validate_assignment_selectors(
        &self,
        assignment_id: &str,
        entry_ramp_id: Option<&str>,
        exit_ramp_id: Option<&str>,
        pair_id: Option<&str>,
    ) -> Result<(), TariffError> {
        self.find_assignment(Some(assignment_id), entry_ramp_id, exit_ramp_id, pair_id)?;
        Ok(())
    }

    pub fn resolve_at(
        &self,
        assignment_id: Option<&str>,
        entry_ramp_id: Option<&str>,
        exit_ramp_id: Option<&str>,
        pair_id: Option<&str>,
        pricing_at: OffsetDateTime,
        scope: &TariffScope,
    ) -> Result<ResolvedTariff, TariffError> {
        scope.validate()?;
        let assignment =
            self.find_assignment(assignment_id, entry_ramp_id, exit_ramp_id, pair_id)?;
        let Some(assignment) = assignment else {
            return Ok(ResolvedTariff::unpriced());
        };
        if assignment.vehicle_profile != scope.vehicle_profile
            || assignment.vehicle_class != scope.vehicle_class
            || assignment.payment_method != scope.payment_method
            || assignment.fare_basis != scope.fare_basis
        {
            return Err(error(
                "TARIFF_SCOPE_MISMATCH",
                "assignment scope does not match the request",
            ));
        }
        let mut started = false;
        for price in &assignment.prices {
            let from = parse_timestamp(&price.effective_from)?
                .ok_or_else(|| error("TARIFF_INTERVAL_INVALID", "price has no effectiveFrom"))?;
            let to = parse_timestamp(price.effective_to.as_deref().unwrap_or(""))?;
            if from > pricing_at {
                continue;
            }
            started = true;
            if to.is_some_and(|end| pricing_at >= end) {
                continue;
            }
            if price.status != "priced"
                || price.tariff_status != "priced"
                || price.amount_yen.is_none()
                || price.evidence_id.is_empty()
                || price.distance_evidence_id.is_empty()
            {
                return Ok(ResolvedTariff::unpriced());
            }
            let rule = self
                .catalog
                .tariff_rules
                .iter()
                .find(|rule| rule.rule_id == price.rule_id)
                .ok_or_else(|| error("TARIFF_RULE_UNKNOWN", "price references an unknown rule"))?;
            let billing_distance_meters = price
                .observed_distance_meters
                .unwrap_or(assignment.billing_distance_meters);
            let amount = calculate_tariff_yen(billing_distance_meters, rule)?;
            if amount != price.amount_yen.unwrap_or_default() {
                return Err(error(
                    "TARIFF_PRICE_CALCULATION_MISMATCH",
                    "stored price does not match the versioned rule",
                ));
            }
            return Ok(ResolvedTariff {
                status: TariffResolutionStatus::Priced,
                amount_yen: price.amount_yen,
                billing_distance_meters: Some(billing_distance_meters),
                effective_from: Some(price.effective_from.clone()),
                effective_to: price.effective_to.clone(),
                rule_id: Some(price.rule_id.clone()),
                evidence_id: Some(price.evidence_id.clone()),
                distance_evidence_id: Some(price.distance_evidence_id.clone()),
                assignment_id: Some(assignment.assignment_id.clone()),
                fare_label: Some(self.catalog.fare_label.clone()),
                vehicle_class: Some(assignment.vehicle_class.clone()),
                payment_method: Some(assignment.payment_method.clone()),
                fare_basis: Some(assignment.fare_basis.clone()),
                discounts_excluded: true,
                toll_source: Some(OFFICIAL_DISTANCE_RULE_SOURCE.to_owned()),
            });
        }
        let mut result = ResolvedTariff::unpriced();
        if started {
            result.status = TariffResolutionStatus::Expired;
        }
        Ok(result)
    }

    pub fn resolve(
        &self,
        entry_ramp_id: &str,
        exit_ramp_id: &str,
        pricing_at: &str,
        scope: &TariffScope,
    ) -> Result<ResolvedTariff, TariffError> {
        self.resolve_od(entry_ramp_id, exit_ramp_id, pricing_at, scope)
    }

    pub fn resolve_assignment(
        &self,
        assignment_id: &str,
        pricing_at: &str,
        scope: &TariffScope,
    ) -> Result<ResolvedTariff, TariffError> {
        let at = parse_timestamp(pricing_at)?
            .ok_or_else(|| error("TARIFF_TIMESTAMP_INVALID", "pricing timestamp is required"))?;
        self.resolve_at(Some(assignment_id), None, None, None, at, scope)
    }

    pub fn resolve_od(
        &self,
        entry_ramp_id: &str,
        exit_ramp_id: &str,
        pricing_at: &str,
        scope: &TariffScope,
    ) -> Result<ResolvedTariff, TariffError> {
        let at = parse_timestamp(pricing_at)?
            .ok_or_else(|| error("TARIFF_TIMESTAMP_INVALID", "pricing timestamp is required"))?;
        self.resolve_at(
            None,
            Some(entry_ramp_id),
            Some(exit_ramp_id),
            None,
            at,
            scope,
        )
    }

    pub fn resolve_pair(
        &self,
        pair_id: &str,
        entry_ramp_id: Option<&str>,
        exit_ramp_id: Option<&str>,
        pricing_at: &str,
        scope: &TariffScope,
    ) -> Result<ResolvedTariff, TariffError> {
        let at = parse_timestamp(pricing_at)?
            .ok_or_else(|| error("TARIFF_TIMESTAMP_INVALID", "pricing timestamp is required"))?;
        self.resolve_at(None, entry_ramp_id, exit_ramp_id, Some(pair_id), at, scope)
    }

    fn find_assignment<'a>(
        &'a self,
        assignment_id: Option<&str>,
        entry_ramp_id: Option<&str>,
        exit_ramp_id: Option<&str>,
        pair_id: Option<&str>,
    ) -> Result<Option<&'a TariffAssignmentV3>, TariffError> {
        let assignment = if let Some(assignment_id) = assignment_id {
            self.catalog
                .assignments
                .iter()
                .find(|assignment| assignment.assignment_id == assignment_id)
                .ok_or_else(|| {
                    error(
                        "TARIFF_ASSIGNMENT_UNKNOWN",
                        format!("unknown assignmentId {assignment_id}"),
                    )
                })
                .map(Some)?
        } else {
            let mut matches = self
                .catalog
                .assignments
                .iter()
                .filter(|assignment| {
                    entry_ramp_id.is_none_or(|entry| assignment.entry_ramp_id == entry)
                        && exit_ramp_id.is_none_or(|exit| assignment.exit_ramp_id == exit)
                        && pair_id
                            .is_none_or(|pair| assignment.pair_ids.iter().any(|id| id == pair))
                })
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(error(
                    "TARIFF_ASSIGNMENT_AMBIGUOUS",
                    "tariff assignment lookup is ambiguous",
                ));
            }
            matches.pop()
        };
        let Some(assignment) = assignment else {
            return Ok(None);
        };
        if entry_ramp_id.is_some_and(|entry| assignment.entry_ramp_id != entry)
            || exit_ramp_id.is_some_and(|exit| assignment.exit_ramp_id != exit)
            || pair_id.is_some_and(|pair| !assignment.pair_ids.iter().any(|id| id == pair))
        {
            return Err(error(
                "TARIFF_ASSIGNMENT_MISMATCH",
                format!(
                    "assignment {} does not match the requested entry, exit, or pair",
                    assignment.assignment_id
                ),
            ));
        }
        Ok(Some(assignment))
    }
}

pub fn read_tariff_v3(input: &str) -> Result<TariffCatalog, TariffError> {
    let file: OdTariffsFileV3 = serde_json::from_str(input)
        .map_err(|json_error| error("TARIFF_JSON_INVALID", json_error.to_string()))?;
    let catalog = file.into_catalog();
    catalog.validate()?;
    Ok(catalog)
}

pub fn read_od_tariffs_v3(input: &str) -> Result<TariffCatalog, TariffError> {
    read_tariff_v3(input)
}

pub fn parse_tariff_v3(input: &str) -> Result<TariffCatalog, TariffError> {
    read_tariff_v3(input)
}

pub fn load_tariff_v3(input: &str) -> Result<TariffCatalog, TariffError> {
    read_tariff_v3(input)
}

pub fn validate_od_tariffs(input: &str) -> Result<(), TariffError> {
    read_tariff_v3(input).map(|_| ())
}

pub fn validate_od_tariffs_file(file: &OdTariffsFileV3) -> Result<(), TariffError> {
    file.validate()
}

pub fn calculate_tariff_micros_yen(
    distance_meters: u64,
    rule: &TariffRuleV3,
) -> Result<u128, TariffError> {
    validate_rule(rule)?;
    if rule.distance_unit_meters == 0 || !distance_meters.is_multiple_of(rule.distance_unit_meters)
    {
        return Err(error(
            "TARIFF_DISTANCE_QUANTUM_MISMATCH",
            "billing distance is not an exact tariff quantum",
        ));
    }
    let distance = u128::from(distance_meters);
    if rule
        .minimum_distance_meters
        .is_some_and(|minimum| distance_meters <= minimum)
    {
        return u128::from(rule.minimum_yen)
            .checked_mul(YEN_MICROS)
            .ok_or_else(|| error("TARIFF_OVERFLOW", "minimum fare overflows microyen"));
    }
    let units = distance / u128::from(rule.distance_unit_meters);
    let terminal = u128::from(rule.terminal_charge_yen)
        .checked_mul(YEN_MICROS)
        .ok_or_else(|| error("TARIFF_OVERFLOW", "terminal charge overflows"))?;
    let subtotal = units
        .checked_mul(u128::from(rule.rate_micros_yen_per_unit))
        .and_then(|value| value.checked_add(terminal))
        .ok_or_else(|| error("TARIFF_OVERFLOW", "subtotal overflows microyen"))?;
    let taxed = subtotal
        .checked_mul(u128::from(rule.tax_basis_points))
        .and_then(|value| value.checked_div(10_000))
        .ok_or_else(|| error("TARIFF_OVERFLOW", "tax calculation overflows"))?;
    let multiple = u128::from(rule.rounding.multiple_yen)
        .checked_mul(YEN_MICROS)
        .ok_or_else(|| error("TARIFF_OVERFLOW", "rounding multiple overflows"))?;
    let quotient = taxed / multiple;
    let remainder = taxed % multiple;
    let rounded_yen = if remainder
        .checked_mul(2)
        .is_some_and(|value| value >= multiple)
    {
        quotient
            .checked_add(1)
            .ok_or_else(|| error("TARIFF_OVERFLOW", "rounding overflows"))?
    } else {
        quotient
    };
    let rounded = rounded_yen
        .checked_mul(multiple)
        .ok_or_else(|| error("TARIFF_OVERFLOW", "rounded fare overflows"))?;
    Ok(rounded
        .max(u128::from(rule.minimum_yen).saturating_mul(YEN_MICROS))
        .min(u128::from(rule.maximum_yen).saturating_mul(YEN_MICROS)))
}

pub fn calculate_versioned_tariff_micros_yen(
    distance_meters: u64,
    rule: &TariffRuleV3,
) -> Result<u128, TariffError> {
    calculate_tariff_micros_yen(distance_meters, rule)
}

pub fn calculate_tariff_yen(distance_meters: u64, rule: &TariffRuleV3) -> Result<u64, TariffError> {
    let micros = calculate_tariff_micros_yen(distance_meters, rule)?;
    u64::try_from(micros / YEN_MICROS)
        .map_err(|_| error("TARIFF_OVERFLOW", "calculated fare does not fit u64"))
}

pub fn calculate_versioned_tariff_yen(
    distance_meters: u64,
    rule: &TariffRuleV3,
) -> Result<u64, TariffError> {
    calculate_tariff_yen(distance_meters, rule)
}

pub fn calculate_tariff_yen_checked(
    distance_meters: u64,
    rule: &TariffRuleV3,
) -> Result<u64, TariffError> {
    calculate_tariff_yen(distance_meters, rule)
}

pub fn calculate_tariff_yen_signed(
    distance_meters: i64,
    rule: &TariffRuleV3,
) -> Result<u64, TariffError> {
    let distance = u64::try_from(distance_meters).map_err(|_| {
        error(
            "TARIFF_NEGATIVE_DISTANCE",
            "billing distance cannot be negative",
        )
    })?;
    calculate_tariff_yen(distance, rule)
}

pub struct TariffCalculator;

impl TariffCalculator {
    pub fn calculate_micros_yen(
        distance_meters: u64,
        rule: &TariffRuleV3,
    ) -> Result<u128, TariffError> {
        calculate_tariff_micros_yen(distance_meters, rule)
    }

    pub fn calculate_yen(distance_meters: u64, rule: &TariffRuleV3) -> Result<u64, TariffError> {
        calculate_tariff_yen(distance_meters, rule)
    }
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

fn validate_rule(rule: &TariffRuleV3) -> Result<(), TariffError> {
    if rule.rule_id.is_empty()
        || rule.vehicle_class != PRODUCT_VEHICLE_CLASS
        || rule.payment_method != PRODUCT_PAYMENT_METHOD
        || rule.fare_basis != PRODUCT_FARE_BASIS
        || rule.distance_unit_meters != DISTANCE_UNIT_METERS
        || rule.rate_micros_yen_per_unit == 0
        || rule.terminal_charge_yen == 0
        || rule.tax_basis_points == 0
        || rule.minimum_yen == 0
        || rule.maximum_yen < rule.minimum_yen
        || rule.rounding.mode != "half_up"
        || rule.rounding.multiple_yen != 10
        || rule.minimum_distance_meters.is_none()
        || !discounts_match(&rule.discounts_excluded)
    {
        return Err(error(
            "TARIFF_RULE_INVALID",
            "tariff rule parameters are invalid",
        ));
    }
    let from = parse_timestamp(&rule.effective_from)?
        .ok_or_else(|| error("TARIFF_INTERVAL_INVALID", "rule has no effectiveFrom"))?;
    let to = parse_timestamp(rule.effective_to.as_deref().unwrap_or(""))?;
    if to.is_some_and(|end| end <= from) {
        return Err(error(
            "TARIFF_INTERVAL_INVALID",
            "rule effectiveTo must be after effectiveFrom",
        ));
    }
    Ok(())
}

fn discounts_match(values: &[String]) -> bool {
    values.len() == PRODUCT_DISCOUNTS_EXCLUDED.len()
        && values
            .iter()
            .zip(PRODUCT_DISCOUNTS_EXCLUDED)
            .all(|(value, expected)| value == expected)
}

fn validate_scope(
    vehicle_profile: &str,
    vehicle_class: &str,
    payment_method: &str,
    fare_basis: &str,
    fare_label: &str,
    discounts_excluded: &[String],
) -> Result<(), TariffError> {
    if vehicle_profile != PRODUCT_VEHICLE_PROFILE
        || vehicle_class != PRODUCT_VEHICLE_CLASS
        || payment_method != PRODUCT_PAYMENT_METHOD
        || fare_basis != PRODUCT_FARE_BASIS
        || fare_label != PRODUCT_FARE_LABEL
        || !discounts_match(discounts_excluded)
    {
        return Err(error(
            "TARIFF_SCOPE_MISMATCH",
            "tariff scope is not the ordinary ETC base-fare scope",
        ));
    }
    Ok(())
}

fn parse_timestamp(value: &str) -> Result<Option<OffsetDateTime>, TariffError> {
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 40 || !value.ends_with('Z') {
        return Err(error(
            "TARIFF_TIMESTAMP_INVALID",
            "tariff timestamp must be UTC RFC3339 with Z suffix",
        ));
    }
    OffsetDateTime::parse(value, &Rfc3339)
        .map(Some)
        .map_err(|_| error("TARIFF_TIMESTAMP_INVALID", "tariff timestamp is invalid"))
}

fn validate_sha256(value: &str) -> Result<(), TariffError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(error(
            "TARIFF_HASH_INVALID",
            "SHA-256 must be lowercase hexadecimal",
        ));
    }
    Ok(())
}
