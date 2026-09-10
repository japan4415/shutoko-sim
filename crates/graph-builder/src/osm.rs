//! OSM Overpass API `[out:json]` format deserialization.
//!
//! Handles elements returned by Overpass JSON endpoints. Extra fields such as
//! `timestamp`, `version`, `changeset`, `user`, and `uid` are ignored without
//! strict rejection so that various Overpass queries can be parsed cleanly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level response from Overpass API `[out:json]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverpassResponse {
    #[serde(default)]
    pub version: Option<f64>,
    #[serde(default)]
    pub generator: Option<String>,
    #[serde(default)]
    pub elements: Vec<OsmElement>,
}

/// An individual OSM element (node, way, or relation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmElement {
    #[serde(rename = "type")]
    pub element_type: String,
    pub id: i64,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    #[serde(default)]
    pub nodes: Option<Vec<i64>>,
    #[serde(default)]
    pub tags: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub members: Option<Vec<OsmMember>>,
}

/// Relation member descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmMember {
    #[serde(rename = "type")]
    pub member_type: String,
    #[serde(rename = "ref")]
    pub ref_id: i64,
    pub role: String,
}

impl OsmElement {
    /// Retrieve tag value by key if available.
    pub fn get_tag(&self, key: &str) -> Option<&str> {
        self.tags.as_ref()?.get(key).map(|s| s.as_str())
    }

    /// Check if element is a node.
    pub fn is_node(&self) -> bool {
        self.element_type == "node"
    }

    /// Check if element is a way.
    pub fn is_way(&self) -> bool {
        self.element_type == "way"
    }

    /// Check if element is a relation.
    pub fn is_relation(&self) -> bool {
        self.element_type == "relation"
    }
}
