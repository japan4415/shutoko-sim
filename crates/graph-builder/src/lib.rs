//! `shutoko-graph-builder` extracts directed road graphs and snap indices from OSM data.
//!
//! Reuses core types from `shutoko-routing-core` to guarantee schema compliance.

pub mod billing;
pub mod manifest;
pub mod model;
pub mod osm;
pub mod seed;
pub mod topology;
pub mod validate;

pub use billing::{
    generate_and_validate_billing_pairs, generate_billing_pair, BillingError,
    BillingGenerationReport, RejectedSeedRecord,
};
pub use manifest::{
    build_manifest, compute_sha256, manifest_to_deterministic_json, Manifest, ManifestArtifact,
    ManifestConfig, ManifestCoverage,
};
pub use model::{
    BillingPair, Edge, EdgeKind, Graph, Node, Price, SnapIndex, SnapNode, VerificationStatus,
};
pub use osm::{OsmElement, OsmMember, OverpassResponse};
pub use seed::{BillingPairSeed, BillingPairsSeedFile, SeedPrice, SeedProvenance};
pub use topology::{
    build_topology, duration_seconds, haversine_distance_meters, is_local_highway,
    is_shutoko_motorway, parse_oneway, snap_index_to_deterministic_json, to_deterministic_json,
    OnewayDirection, TopologyConfig, LOCAL_SPEED_KMH, RAMP_SPEED_KMH, SHUTOKO_SPEED_KMH,
};
pub use validate::{
    contains_forbidden_transition, has_non_empty_shutoko_loop, parse_utc_timestamp,
    validate_billing_pair, ValidationError,
};
