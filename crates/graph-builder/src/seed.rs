//! Declarative billing pair seed definitions.
//!
//! # Specification: `data/billing-pairs-seed.json`
//!
//! Because toll eligibility and accurate entrance-to-exit pairings cannot be safely
//! deduced from OSM geometry alone, billing pairs are defined in a declarative seed
//! file curated with human verification and authoritative tariff citations.
//!
//! ## JSON Schema Structure
//! ```json
//! {
//!   "schemaVersion": 1,
//!   "description": "Optional human-readable description",
//!   "billingPairs": [
//!     {
//!       "id": "bp:c1-inner:shibakoen-kasumigaseki",
//!       "entryOsmWayId": 12345678,
//!       "exitOsmWayId": 87654321,
//!       "anchorOsmNodeId": 999999,
//!       "vehicleProfile": "passenger-car-etc",
//!       "status": "verified",
//!       "oneSectionAheadVerified": true,
//!       "provenance": {
//!         "source": "https://www.shutoko.jp/fee/fee-info/...",
//!         "sourceDate": "2026-09-10",
//!         "notes": "Verified against 2026 tariff table"
//!       },
//!       "prices": [
//!         {
//!           "amountYen": 300,
//!           "effectiveFrom": "2026-01-01T00:00:00Z",
//!           "effectiveTo": "2026-10-01T00:00:00Z"
//!         }
//!       ]
//!     }
//!   ]
//! }
//! ```

use crate::model::VerificationStatus;
use serde::{Deserialize, Serialize};

/// Top-level structure for the billing pair seed file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingPairsSeedFile {
    /// Schema version for the seed specification (currently 1).
    pub schema_version: u32,
    /// Human-readable description of this seed dataset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// List of declarative billing pair seed entries.
    pub billing_pairs: Vec<BillingPairSeed>,
}

/// A single declarative billing pair seed entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingPairSeed {
    /// Unique identifier for the billing pair (max 256 bytes).
    pub id: String,

    /// OSM way ID representing the entry ramp.
    pub entry_osm_way_id: i64,

    /// OSM way ID representing the exit ramp.
    pub exit_osm_way_id: i64,

    /// OSM node ID representing the loop anchor on the Shutoko mainline.
    pub anchor_osm_node_id: i64,

    /// Target vehicle profile (e.g. "passenger-car-etc").
    pub vehicle_profile: String,

    /// Verification status ("verified" or "unverified").
    pub status: VerificationStatus,

    /// Explicit human verification that this pair is strictly the "1 section ahead" exit.
    pub one_section_ahead_verified: bool,

    /// Provenance and citation data for this entry and tariff.
    pub provenance: SeedProvenance,

    /// Tariff rules applicable to this pair with effective UTC intervals.
    #[serde(default)]
    pub prices: Vec<SeedPrice>,
}

/// Provenance citation verifying the source and date of the billing pair data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedProvenance {
    /// Source reference (e.g. official URL or tariff gazette).
    pub source: String,

    /// Date the source data or tariff was inspected/verified (ISO 8601 YYYY-MM-DD).
    pub source_date: String,

    /// Optional notes regarding the inspection or verification context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Price record within a seed entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedPrice {
    /// Toll amount in JPY (must be > 0).
    pub amount_yen: u64,

    /// RFC 3339 UTC timestamp with "Z" suffix marking the start of validity (inclusive).
    pub effective_from: String,

    /// RFC 3339 UTC timestamp with "Z" suffix marking the end of validity (exclusive),
    /// or `None` if currently valid indefinitely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_to: Option<String>,
}
