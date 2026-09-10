//! `shutoko-graph-builder` extracts directed road graphs and snap indices from OSM data.
//!
//! Reuses core types from `shutoko-routing-core` to guarantee schema compliance.

pub mod model;
pub mod osm;
pub mod topology;

pub use model::{
    BillingPair, Edge, EdgeKind, Graph, Node, Price, SnapIndex, SnapNode, VerificationStatus,
};
pub use osm::{OsmElement, OsmMember, OverpassResponse};
pub use topology::{
    build_topology, duration_seconds, haversine_distance_meters, is_local_highway,
    is_shutoko_motorway, parse_oneway, snap_index_to_deterministic_json, to_deterministic_json,
    OnewayDirection, TopologyConfig, LOCAL_SPEED_KMH, RAMP_SPEED_KMH, SHUTOKO_SPEED_KMH,
};
