//! Manifest model and deterministic generator for release artifacts.
//!
//! # Specification: `manifest.json`
//! Defined in `docs/interfaces.md`. Provides cryptographic integrity, metadata,
//! vehicle profile, attribution, and coverage specifications for downstream consumers.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Root release manifest document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Schema version for manifest format (currently 1).
    pub schema_version: u32,

    /// Unique release identifier (e.g. "release-2026-09-10-c1").
    pub release_id: String,

    /// Routing engine crate semver version.
    pub engine_version: String,

    /// Graph dataset version.
    pub graph_version: String,

    /// Fixed ISO 8601 UTC build timestamp provided externally (never generated from local clock).
    pub built_at: String,

    /// Date the upstream source data was retrieved/captured (e.g. "2026-09-10").
    pub source_date: String,

    /// Geographical coverage area and verified endpoints.
    pub coverage: ManifestCoverage,

    /// Applicable vehicle profile (e.g. "passenger-car-etc").
    pub vehicle_profile: String,

    /// Time model specification identifier (e.g. "v1-static-speeds").
    pub time_model_version: String,

    /// Billing pairs tariff specification identifier (e.g. "v1").
    pub billing_pairs_version: String,

    /// Attribution notice required by OpenStreetMap Open Database License (ODbL).
    pub attribution: String,

    /// URL for upstream ODbL copyright terms.
    pub odbl_license_url: String,

    /// Sections or ramps in the target area known to be unverified or excluded.
    #[serde(default)]
    pub unverified_sections: Vec<String>,

    /// Cryptographic checksums and byte counts for release artifacts.
    pub artifacts: Vec<ManifestArtifact>,
}

/// Coverage scope and list of verified entry/exit points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestCoverage {
    /// Textual summary of the covered region.
    pub area: String,

    /// List of verified entry edge IDs.
    pub verified_entries: Vec<String>,

    /// List of verified exit edge IDs.
    pub verified_exits: Vec<String>,
}

/// Release artifact descriptor containing relative path, SHA-256 digest, and size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestArtifact {
    /// Relative path of artifact within the release bundle (e.g. "graph.json").
    pub path: String,

    /// Lowercase hex-encoded SHA-256 digest of the artifact bytes.
    pub sha256: String,

    /// Byte length of the artifact file.
    pub byte_length: u64,
}

/// Compute the lowercase hex-encoded SHA-256 digest of a byte buffer.
pub fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Configuration parameters for generating a release manifest.
#[derive(Debug, Clone)]
pub struct ManifestConfig {
    pub release_id: String,
    pub engine_version: String,
    pub graph_version: String,
    pub built_at: String,
    pub source_date: String,
    pub coverage_area: String,
    pub vehicle_profile: String,
    pub time_model_version: String,
    pub billing_pairs_version: String,
    pub unverified_sections: Vec<String>,
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            release_id: "default-release".into(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            graph_version: "1.0.0".into(),
            built_at: "2026-09-10T00:00:00Z".into(),
            source_date: "2026-09-10".into(),
            coverage_area: "Tokyo Inner Circular Route (C1) and Metropolitan Expressway".into(),
            vehicle_profile: "passenger-car-etc".into(),
            time_model_version: "v1-static-speeds".into(),
            billing_pairs_version: "v1".into(),
            unverified_sections: Vec::new(),
        }
    }
}

/// Construct a `Manifest` from config, verified entries/exits, and artifact buffers.
pub fn build_manifest(
    config: &ManifestConfig,
    verified_entries: Vec<String>,
    verified_exits: Vec<String>,
    artifacts: Vec<(&str, &[u8])>,
) -> Manifest {
    let mut artifact_records: Vec<ManifestArtifact> = artifacts
        .into_iter()
        .map(|(path, bytes)| ManifestArtifact {
            path: path.to_string(),
            sha256: compute_sha256(bytes),
            byte_length: bytes.len() as u64,
        })
        .collect();
    artifact_records.sort_by(|a, b| a.path.cmp(&b.path));

    let mut sorted_entries = verified_entries;
    sorted_entries.sort();
    sorted_entries.dedup();

    let mut sorted_exits = verified_exits;
    sorted_exits.sort();
    sorted_exits.dedup();

    let mut sorted_unverified = config.unverified_sections.clone();
    sorted_unverified.sort();
    sorted_unverified.dedup();

    Manifest {
        schema_version: 1,
        release_id: config.release_id.clone(),
        engine_version: config.engine_version.clone(),
        graph_version: config.graph_version.clone(),
        built_at: config.built_at.clone(),
        source_date: config.source_date.clone(),
        coverage: ManifestCoverage {
            area: config.coverage_area.clone(),
            verified_entries: sorted_entries,
            verified_exits: sorted_exits,
        },
        vehicle_profile: config.vehicle_profile.clone(),
        time_model_version: config.time_model_version.clone(),
        billing_pairs_version: config.billing_pairs_version.clone(),
        attribution: "© OpenStreetMap contributors".into(),
        odbl_license_url: "https://www.openstreetmap.org/copyright".into(),
        unverified_sections: sorted_unverified,
        artifacts: artifact_records,
    }
}

/// Serialize Manifest deterministically with 2-space indentation and trailing newline.
pub fn manifest_to_deterministic_json(manifest: &Manifest) -> Result<String, serde_json::Error> {
    let mut s = serde_json::to_string_pretty(manifest)?;
    s.push('\n');
    Ok(s)
}
