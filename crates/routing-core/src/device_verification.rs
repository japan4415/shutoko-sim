use crate::handoff::{MapsHandoffError, MapsHandoffLegRole, SplitMapsHandoff, URL_BUILDER_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const DEVICE_VERIFICATION_MANIFEST_SCHEMA_VERSION: u8 = 1;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_TEXT_LENGTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceVerificationReleaseConfig {
    pub manifest_json: String,
    pub evaluated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceVerificationOs {
    Android,
    Ios,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceVerificationClient {
    Web,
    App,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceVerificationResult {
    Passed,
    Failed,
    Missing,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceVerificationGateBlocker {
    ManifestMissing,
    ManifestInvalid,
    EvaluationTimeInvalid,
    HandoffInvalid,
    BindingMismatch,
    VerificationMissing,
    VerificationFailed,
    VerificationNotYetValid,
    VerificationExpired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceVerificationGateBinding {
    route_plan_id: String,
    release_id: String,
    builder_version: String,
    loop_transfer_url_sha256: String,
}

impl DeviceVerificationGateBinding {
    fn new(
        route_plan_id: &str,
        release_id: &str,
        handoff: &SplitMapsHandoff,
    ) -> Result<Self, MapsHandoffError> {
        let legs = handoff.wire_legs()?;
        let loop_transfer = legs
            .iter()
            .find(|leg| leg.role == MapsHandoffLegRole::LoopTransfer)
            .ok_or(MapsHandoffError::InvalidLegOrder)?;
        Ok(Self {
            route_plan_id: route_plan_id.to_owned(),
            release_id: release_id.to_owned(),
            builder_version: handoff.builder_version.clone(),
            loop_transfer_url_sha256: loop_transfer.url_sha256.clone(),
        })
    }

    fn matches(&self, route_plan_id: &str, release_id: &str, handoff: &SplitMapsHandoff) -> bool {
        let Ok(legs) = handoff.wire_legs() else {
            return false;
        };
        let Some(loop_transfer) = legs
            .iter()
            .find(|leg| leg.role == MapsHandoffLegRole::LoopTransfer)
        else {
            return false;
        };
        self.route_plan_id == route_plan_id
            && self.release_id == release_id
            && self.builder_version == handoff.builder_version
            && self.loop_transfer_url_sha256 == loop_transfer.url_sha256
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceVerificationGateDecision {
    public_departure_enabled: bool,
    blocked_by: Option<DeviceVerificationGateBlocker>,
    binding: Option<DeviceVerificationGateBinding>,
}

impl DeviceVerificationGateDecision {
    pub fn public_departure_enabled(&self) -> bool {
        self.public_departure_enabled
    }

    pub fn blocked_by(&self) -> Option<DeviceVerificationGateBlocker> {
        self.blocked_by
    }

    pub(crate) fn matches_binding(
        &self,
        route_plan_id: &str,
        release_id: &str,
        handoff: &SplitMapsHandoff,
    ) -> bool {
        self.binding
            .as_ref()
            .is_some_and(|binding| binding.matches(route_plan_id, release_id, handoff))
    }

    fn open(binding: DeviceVerificationGateBinding) -> Self {
        Self {
            public_departure_enabled: true,
            blocked_by: None,
            binding: Some(binding),
        }
    }

    fn closed(blocked_by: DeviceVerificationGateBlocker) -> Self {
        Self {
            public_departure_enabled: false,
            blocked_by: Some(blocked_by),
            binding: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceVerificationLeg {
    pub role: MapsHandoffLegRole,
    pub url_sha256: String,
    pub expected_road: String,
    pub expected_direction: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceVerificationRecord {
    pub os: DeviceVerificationOs,
    pub os_version: String,
    pub client: DeviceVerificationClient,
    pub client_name: String,
    pub client_version: String,
    pub verified_at: Option<String>,
    pub result: DeviceVerificationResult,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceVerificationManifest {
    pub schema_version: u8,
    pub route_plan_id: String,
    pub release_id: String,
    pub url_builder_version: String,
    pub legs: Vec<DeviceVerificationLeg>,
    pub verifications: Vec<DeviceVerificationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceVerificationManifestError {
    InvalidJson,
    InvalidSchemaVersion,
    InvalidIdentifier,
    InvalidUrlBuilderVersion,
    InvalidLegs,
    InvalidVerificationMatrix,
    InvalidEnvironmentVersion,
    InvalidVerificationResult,
    InvalidVerificationTimestamp,
    RoutePlanIdMismatch,
    ReleaseIdMismatch,
    BuilderVersionMismatch,
    LegUrlHashMismatch,
    InvalidHandoff(MapsHandoffError),
}

pub fn parse_device_verification_manifest(
    input: &str,
) -> Result<DeviceVerificationManifest, DeviceVerificationManifestError> {
    if input.len() > MAX_MANIFEST_BYTES {
        return Err(DeviceVerificationManifestError::InvalidJson);
    }
    let manifest: DeviceVerificationManifest =
        serde_json::from_str(input).map_err(|_| DeviceVerificationManifestError::InvalidJson)?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn evaluate_device_verification_gate(
    manifest_json: Option<&str>,
    route_plan_id: &str,
    release_id: &str,
    handoff: &SplitMapsHandoff,
    evaluated_at: Option<&str>,
) -> DeviceVerificationGateDecision {
    let Some(manifest_json) = manifest_json else {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::ManifestMissing,
        );
    };
    let manifest = match parse_device_verification_manifest(manifest_json) {
        Ok(manifest) => manifest,
        Err(_) => {
            return DeviceVerificationGateDecision::closed(
                DeviceVerificationGateBlocker::ManifestInvalid,
            );
        }
    };
    if handoff.validate().is_err() {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::HandoffInvalid,
        );
    }
    if manifest
        .validate_binding(route_plan_id, release_id, handoff)
        .is_err()
    {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::BindingMismatch,
        );
    }
    let Some(evaluated_at) = evaluated_at.and_then(parse_utc) else {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::EvaluationTimeInvalid,
        );
    };

    let mut missing = false;
    let mut failed = false;
    let mut not_yet_valid = false;
    let mut expired = false;
    for record in &manifest.verifications {
        match record.result {
            DeviceVerificationResult::Missing => {
                missing = true;
                continue;
            }
            DeviceVerificationResult::Failed => {
                failed = true;
                continue;
            }
            DeviceVerificationResult::Expired => {
                expired = true;
                continue;
            }
            DeviceVerificationResult::Passed => {}
        }
        let Some(verified_at) = record.verified_at.as_deref().and_then(parse_utc) else {
            return DeviceVerificationGateDecision::closed(
                DeviceVerificationGateBlocker::ManifestInvalid,
            );
        };
        let Some(expires_at) = record.expires_at.as_deref().and_then(parse_utc) else {
            return DeviceVerificationGateDecision::closed(
                DeviceVerificationGateBlocker::ManifestInvalid,
            );
        };
        if evaluated_at < verified_at {
            not_yet_valid = true;
        }
        if evaluated_at >= expires_at {
            expired = true;
        }
    }
    if missing {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::VerificationMissing,
        );
    }
    if failed {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::VerificationFailed,
        );
    }
    if not_yet_valid {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::VerificationNotYetValid,
        );
    }
    if expired {
        return DeviceVerificationGateDecision::closed(
            DeviceVerificationGateBlocker::VerificationExpired,
        );
    }
    match DeviceVerificationGateBinding::new(route_plan_id, release_id, handoff) {
        Ok(binding) => DeviceVerificationGateDecision::open(binding),
        Err(_) => {
            DeviceVerificationGateDecision::closed(DeviceVerificationGateBlocker::HandoffInvalid)
        }
    }
}

impl DeviceVerificationManifest {
    pub fn validate(&self) -> Result<(), DeviceVerificationManifestError> {
        if self.schema_version != DEVICE_VERIFICATION_MANIFEST_SCHEMA_VERSION {
            return Err(DeviceVerificationManifestError::InvalidSchemaVersion);
        }
        if !valid_text(&self.route_plan_id) || !valid_text(&self.release_id) {
            return Err(DeviceVerificationManifestError::InvalidIdentifier);
        }
        if self.url_builder_version != URL_BUILDER_VERSION {
            return Err(DeviceVerificationManifestError::InvalidUrlBuilderVersion);
        }
        let expected_roles = [
            MapsHandoffLegRole::SurfaceAccess,
            MapsHandoffLegRole::LoopTransfer,
            MapsHandoffLegRole::SurfaceReturn,
        ];
        if self.legs.len() != expected_roles.len() {
            return Err(DeviceVerificationManifestError::InvalidLegs);
        }
        for (leg, expected_role) in self.legs.iter().zip(expected_roles) {
            if leg.role != expected_role
                || !valid_sha256(&leg.url_sha256)
                || !valid_text(&leg.expected_road)
                || !valid_text(&leg.expected_direction)
            {
                return Err(DeviceVerificationManifestError::InvalidLegs);
            }
        }
        let expected_matrix = BTreeSet::from([
            (DeviceVerificationOs::Android, DeviceVerificationClient::Web),
            (DeviceVerificationOs::Android, DeviceVerificationClient::App),
            (DeviceVerificationOs::Ios, DeviceVerificationClient::Web),
            (DeviceVerificationOs::Ios, DeviceVerificationClient::App),
        ]);
        let actual_matrix = self
            .verifications
            .iter()
            .map(|record| (record.os, record.client))
            .collect::<BTreeSet<_>>();
        if self.verifications.len() != expected_matrix.len() || actual_matrix != expected_matrix {
            return Err(DeviceVerificationManifestError::InvalidVerificationMatrix);
        }
        for record in &self.verifications {
            if !valid_text(&record.os_version)
                || !valid_text(&record.client_name)
                || !valid_text(&record.client_version)
                || (record.result == DeviceVerificationResult::Passed
                    && (!valid_verified_version(&record.os_version)
                        || !valid_verified_version(&record.client_version)
                        || !valid_client_name(record)))
            {
                return Err(DeviceVerificationManifestError::InvalidEnvironmentVersion);
            }
            validate_record_timestamps(record)?;
        }
        Ok(())
    }

    pub fn validate_binding(
        &self,
        route_plan_id: &str,
        release_id: &str,
        handoff: &SplitMapsHandoff,
    ) -> Result<(), DeviceVerificationManifestError> {
        self.validate()?;
        if self.route_plan_id != route_plan_id {
            return Err(DeviceVerificationManifestError::RoutePlanIdMismatch);
        }
        if self.release_id != release_id {
            return Err(DeviceVerificationManifestError::ReleaseIdMismatch);
        }
        if self.url_builder_version != handoff.builder_version {
            return Err(DeviceVerificationManifestError::BuilderVersionMismatch);
        }
        let wire_legs = handoff
            .wire_legs()
            .map_err(DeviceVerificationManifestError::InvalidHandoff)?;
        let manifest_loop = self
            .legs
            .iter()
            .find(|leg| leg.role == MapsHandoffLegRole::LoopTransfer)
            .ok_or(DeviceVerificationManifestError::InvalidLegs)?;
        let wire_loop = wire_legs
            .iter()
            .find(|leg| leg.role == MapsHandoffLegRole::LoopTransfer)
            .ok_or(DeviceVerificationManifestError::LegUrlHashMismatch)?;
        if manifest_loop.url_sha256 != wire_loop.url_sha256 {
            return Err(DeviceVerificationManifestError::LegUrlHashMismatch);
        }
        Ok(())
    }
}

fn validate_record_timestamps(
    record: &DeviceVerificationRecord,
) -> Result<(), DeviceVerificationManifestError> {
    if record.result == DeviceVerificationResult::Missing {
        if record.verified_at.is_some() || record.expires_at.is_some() {
            return Err(DeviceVerificationManifestError::InvalidVerificationResult);
        }
        return Ok(());
    }
    let verified_at = record
        .verified_at
        .as_deref()
        .and_then(parse_utc)
        .ok_or(DeviceVerificationManifestError::InvalidVerificationTimestamp)?;
    let expires_at = record
        .expires_at
        .as_deref()
        .and_then(parse_utc)
        .ok_or(DeviceVerificationManifestError::InvalidVerificationTimestamp)?;
    if verified_at >= expires_at {
        return Err(DeviceVerificationManifestError::InvalidVerificationTimestamp);
    }
    Ok(())
}

fn valid_text(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_TEXT_LENGTH
        && !value.chars().any(char::is_control)
}

fn valid_verified_version(value: &str) -> bool {
    valid_text(value)
        && !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "unverified" | "unknown" | "n/a" | "not verified"
        )
}

fn valid_client_name(record: &DeviceVerificationRecord) -> bool {
    match (record.os, record.client) {
        (DeviceVerificationOs::Android, DeviceVerificationClient::Web) => {
            record.client_name == "chrome"
        }
        (DeviceVerificationOs::Ios, DeviceVerificationClient::Web) => {
            record.client_name == "safari"
        }
        (
            DeviceVerificationOs::Android | DeviceVerificationOs::Ios,
            DeviceVerificationClient::App,
        ) => record.client_name == "google_maps",
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn parse_utc(value: &str) -> Option<OffsetDateTime> {
    if value.len() > 40 || !value.ends_with('Z') {
        return None;
    }
    OffsetDateTime::parse(value, &Rfc3339).ok()
}
