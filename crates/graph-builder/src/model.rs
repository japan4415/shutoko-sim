//! Graph and data model definitions for the Shutoko graph builder.
//!
//! To eliminate schema drift and guarantee 100% compatibility with
//! `shutoko-routing-core` (which enforces `deny_unknown_fields`), we reuse
//! the core types (`Graph`, `Node`, `Edge`, `EdgeKind`, `BillingPair`, `Price`,
//! `VerificationStatus`) directly from `shutoko-routing-core`.
//!
//! Coordinate snap indices (`SnapIndex`, `SnapNode`) are kept in a separate
//! artifact (`snap-index.json`) as decided in design decision #5 to avoid
//! mutating the strict `Node { id }` schema of `routing-core`.

pub use shutoko_routing_core::{
    BillingPair, Edge, EdgeKind, Graph, Node, Price, VerificationStatus,
};

use serde::{Deserialize, Serialize};

/// Snap index artifact mapping node IDs to geographic coordinates.
/// Decoupled from `Graph` to preserve strict schema compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapIndex {
    pub schema_version: u32,
    pub release_id: String,
    pub nodes: Vec<SnapNode>,
}

/// Geographic coordinate entry for an origin snap candidate node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapNode {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
}
