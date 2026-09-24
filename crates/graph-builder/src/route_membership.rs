use crate::inventory::{
    CanonicalRampInventoryItem, OsmRampBinding, OsmRampBindingsFile, RampInventoryFile,
};
use crate::model::{Edge, EdgeKind, Graph, Node, OdTariff, Ramp, RampKind};
use crate::osm::{OsmElement, OsmMember, OverpassResponse};
use crate::seed::{
    DiagnosticEndpoint, DiagnosticRoutePlan, DirectedEndpointSegment, DirectedJunctionAnchor,
    EndpointSupportState, MandatoryLap, RadialReturnBillingPairSeed,
};
use crate::validate::contains_forbidden_transition;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteMembershipError {
    InvalidInput(String),
    Relation(String),
    RampBinding(String),
    Segment(String),
    Validation(String),
    BudgetExceeded(String),
    ExitNotFound(String),
}

impl RouteMembershipError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "ROUTE_MEMBERSHIP_INVALID_INPUT",
            Self::Relation(_) => "ROUTE_MEMBERSHIP_RELATION_INVALID",
            Self::RampBinding(_) => "ROUTE_MEMBERSHIP_RAMP_BINDING_INVALID",
            Self::Segment(_) => "ROUTE_MEMBERSHIP_SEGMENT_INVALID",
            Self::Validation(_) => "ROUTE_MEMBERSHIP_VALIDATION_FAILED",
            Self::BudgetExceeded(_) => "ROUTE_MEMBERSHIP_BUDGET_EXCEEDED",
            Self::ExitNotFound(_) => "ROUTE_MEMBERSHIP_EXIT_NOT_FOUND",
        }
    }
}

impl fmt::Display for RouteMembershipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (prefix, message) = match self {
            Self::InvalidInput(message)
            | Self::Relation(message)
            | Self::RampBinding(message)
            | Self::Segment(message)
            | Self::Validation(message)
            | Self::BudgetExceeded(message)
            | Self::ExitNotFound(message) => (self.code(), message),
        };
        write!(f, "{}: {}", prefix, message)
    }
}

impl std::error::Error for RouteMembershipError {}

pub use shutoko_routing_core::{
    RouteMembershipIndex, RouteMembershipSegment, RouteMembershipSourceKind,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundRampEvidence {
    pub binding_evidence_id: String,
    pub ramp_id: String,
    pub route_id: String,
    pub direction: String,
    pub osm_way_ids: Vec<i64>,
    pub osm_node_ids: Vec<i64>,
    pub edge_ids: Vec<String>,
    pub from_node_id: String,
    pub to_node_id: String,
    pub edge_ids_sha256: String,
}

impl BoundRampEvidence {
    pub fn from_directed_segment(
        binding_evidence_id: impl Into<String>,
        ramp_id: impl Into<String>,
        route_id: impl Into<String>,
        direction: impl Into<String>,
        segment: &DirectedEndpointSegment,
    ) -> Self {
        Self {
            binding_evidence_id: binding_evidence_id.into(),
            ramp_id: ramp_id.into(),
            route_id: route_id.into(),
            direction: direction.into(),
            osm_way_ids: segment.osm_way_ids.clone(),
            osm_node_ids: segment.osm_node_ids.clone(),
            edge_ids: segment.edge_ids.clone(),
            from_node_id: segment.from_node_id.clone(),
            to_node_id: segment.to_node_id.clone(),
            edge_ids_sha256: segment.edge_ids_sha256.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RouteMembershipBuildOptions {
    pub source_snapshot_sha256: String,
    pub relation_ids: Option<Vec<i64>>,
    pub bound_ramp_evidence: Vec<BoundRampEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GraphSchemaV4BillingPair {
    LegacyRing(Box<shutoko_routing_core::LegacyRingBillingPair>),
    RadialReturn(Box<shutoko_routing_core::RadialReturnBillingPair>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphSchemaV4 {
    pub schema_version: u32,
    pub release_id: String,
    pub vehicle_profile: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub billing_pairs: Vec<GraphSchemaV4BillingPair>,
    #[serde(default)]
    pub forbidden_transitions: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ramps: Vec<Ramp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub od_tariffs: Vec<OdTariff>,
    pub route_memberships: Vec<RouteMembershipIndex>,
}

impl GraphSchemaV4 {
    pub fn from_graph(graph: &Graph, route_memberships: Vec<RouteMembershipIndex>) -> Self {
        Self::try_from_graph(graph, route_memberships)
            .expect("schema 4 graph must resolve every legacy billing pair")
    }

    pub fn try_from_graph(
        graph: &Graph,
        route_memberships: Vec<RouteMembershipIndex>,
    ) -> Result<Self, RouteMembershipError> {
        Self::try_from_graph_with_radial(graph, route_memberships, Vec::new())
    }

    pub fn try_from_graph_with_radial(
        graph: &Graph,
        route_memberships: Vec<RouteMembershipIndex>,
        radial_billing_pairs: Vec<shutoko_routing_core::RadialReturnBillingPair>,
    ) -> Result<Self, RouteMembershipError> {
        let mut billing_pairs = graph
            .billing_pairs
            .iter()
            .map(|pair| {
                shutoko_routing_core::LegacyRingBillingPair::from_legacy(
                    pair,
                    graph,
                    &route_memberships,
                )
                .map(|pair| GraphSchemaV4BillingPair::LegacyRing(Box::new(pair)))
                .map_err(|error| RouteMembershipError::InvalidInput(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        billing_pairs.extend(
            radial_billing_pairs
                .into_iter()
                .map(|pair| GraphSchemaV4BillingPair::RadialReturn(Box::new(pair))),
        );
        Ok(Self {
            schema_version: 4,
            release_id: graph.release_id.clone(),
            vehicle_profile: graph.vehicle_profile.clone(),
            nodes: graph.nodes.clone(),
            edges: graph.edges.clone(),
            billing_pairs,
            forbidden_transitions: graph.forbidden_transitions.clone(),
            ramps: graph.ramps.clone(),
            od_tariffs: graph.od_tariffs.clone(),
            route_memberships,
        })
    }
}

pub fn graph_schema_v4_to_deterministic_json(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
) -> Result<String, serde_json::Error> {
    graph_schema_v4_to_deterministic_json_with_radial(graph, route_memberships, Vec::new())
}

pub fn graph_schema_v4_to_deterministic_json_with_radial(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    radial_billing_pairs: Vec<shutoko_routing_core::RadialReturnBillingPair>,
) -> Result<String, serde_json::Error> {
    let document = GraphSchemaV4::try_from_graph_with_radial(
        graph,
        route_memberships.to_vec(),
        radial_billing_pairs,
    )
    .map_err(|error| serde_json::Error::io(std::io::Error::other(error.to_string())))?;
    let mut output = serde_json::to_string_pretty(&document)?;
    output.push('\n');
    Ok(output)
}

pub const CORRIDOR_EXIT_STATE_BUDGET: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorridorExit {
    pub distance_meters: u64,
    pub exit_edge_id: String,
    pub ramp_id: String,
    pub edge_ids: Vec<String>,
    pub mainline_edge_ids: Vec<String>,
    pub mainline_source_segment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutePlanLapV1 {
    pub merge_node_id: String,
    pub branch_node_id: String,
    pub route_id: String,
    pub direction: String,
    pub first_edge_id: String,
    pub last_edge_id: String,
    pub lap_count: u8,
    pub source_segment_id: String,
    pub edge_ids: Vec<String>,
    pub edge_ids_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoutePlanSegmentRole {
    EntryApproach,
    MandatoryLap,
    ReturnCorridor,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorridorFirstExitResolution {
    pub exact_directed_binding: EndpointSupportState,
    pub exit: Option<CorridorExit>,
    pub blocked_exit_edge_id: Option<String>,
    pub blocked_ramp_id: Option<String>,
    pub mainline_edge_ids: Vec<String>,
    pub mainline_source_segment_ids: Vec<String>,
    pub distance_meters: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectedRoutePlanResolution {
    pub lap: RoutePlanLapV1,
    pub first_exit: CorridorFirstExitResolution,
}

pub fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn ordered_edge_ids_sha256(edge_ids: &[String]) -> Result<String, serde_json::Error> {
    let encoded = serde_json::to_vec(edge_ids)?;
    Ok(compute_sha256(&encoded))
}

pub fn route_memberships_sha256(
    route_memberships: &[RouteMembershipIndex],
) -> Result<String, RouteMembershipError> {
    let encoded = serde_json::to_vec(route_memberships)
        .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
    Ok(compute_sha256(&encoded))
}

fn validate_sha256(value: &str, field: &str) -> Result<(), RouteMembershipError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(RouteMembershipError::InvalidInput(format!(
            "{} must be a lowercase 64-character SHA-256 value",
            field
        )));
    }
    Ok(())
}

pub const ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION: &str = "osm-relation-role/v1";

fn normalized(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn direction_is_reverse(direction: &str) -> bool {
    matches!(
        normalized(direction).as_str(),
        "outer" | "backward" | "reverse" | "west" | "south"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationMemberRole {
    Inner,
    Outer,
    Backward,
    Forward,
    East,
    West,
    North,
    South,
}

fn relation_member_role(role: &str) -> Option<RelationMemberRole> {
    let role = normalized(role);
    if role.contains("inner") {
        Some(RelationMemberRole::Inner)
    } else if role.contains("outer") {
        Some(RelationMemberRole::Outer)
    } else if role.contains("backward") || role.contains("reverse") {
        Some(RelationMemberRole::Backward)
    } else if role.contains("forward") {
        Some(RelationMemberRole::Forward)
    } else if role == "east" {
        Some(RelationMemberRole::East)
    } else if role == "west" {
        Some(RelationMemberRole::West)
    } else if role == "north" {
        Some(RelationMemberRole::North)
    } else if role == "south" {
        Some(RelationMemberRole::South)
    } else {
        None
    }
}

fn inferred_blank_member_direction(
    members: &[OsmMember],
    index: usize,
    route_id: &str,
) -> Option<String> {
    let mut previous = None;
    for member in members[..index].iter().rev() {
        if member.member_type != "way" {
            continue;
        }
        if let Some(role) = relation_member_role(&member.role) {
            previous = Some(canonical_relation_direction(route_id, role));
            break;
        }
    }
    let mut next = None;
    for member in members.iter().skip(index + 1) {
        if member.member_type != "way" {
            continue;
        }
        if let Some(role) = relation_member_role(&member.role) {
            next = Some(canonical_relation_direction(route_id, role));
            break;
        }
    }
    match (previous, next) {
        (Some(previous), Some(next)) if previous == next => Some(previous),
        (Some(direction), None) | (None, Some(direction)) => Some(direction),
        _ => None,
    }
}

fn canonical_relation_direction(route_id: &str, role: RelationMemberRole) -> String {
    match role {
        RelationMemberRole::Inner => "inner".into(),
        RelationMemberRole::Outer => "outer".into(),
        RelationMemberRole::Backward if route_id == "2" => "inbound".into(),
        RelationMemberRole::Forward if route_id == "2" => "outbound".into(),
        RelationMemberRole::Backward => "backward".into(),
        RelationMemberRole::Forward => "forward".into(),
        RelationMemberRole::East => "east".into(),
        RelationMemberRole::West => "west".into(),
        RelationMemberRole::North => "north".into(),
        RelationMemberRole::South => "south".into(),
    }
}

fn canonicalize_declared_direction(
    route_id: &str,
    direction: &str,
) -> Result<String, RouteMembershipError> {
    let direction = normalized(direction);
    let canonical = match direction.as_str() {
        "forward" if route_id == "2" => "outbound",
        "backward" if route_id == "2" => "inbound",
        "forward" | "backward" | "inner" | "outer" | "east" | "west" | "north" | "south" => {
            direction.as_str()
        }
        _ => {
            return Err(RouteMembershipError::Relation(format!(
                "relation {} has unsupported direction {:?}",
                route_id, direction
            )))
        }
    };
    Ok(canonical.into())
}

fn relation_directions(relation: &OsmElement) -> Result<Vec<String>, RouteMembershipError> {
    let route_id = relation_route_id(relation)?;
    if let Some(direction) = relation
        .get_tag("direction")
        .or_else(|| relation.get_tag("route_direction"))
    {
        let values: Vec<String> = direction
            .split([',', ';'])
            .map(normalized)
            .filter(|value| !value.is_empty())
            .map(|value| canonicalize_declared_direction(&route_id, &value))
            .collect::<Result<_, _>>()?;
        if !values.is_empty() {
            return Ok(values);
        }
    }

    let mut directions = BTreeSet::new();
    for member in relation.members.iter().flatten() {
        if member.member_type != "way" {
            continue;
        }
        if let Some(role) = relation_member_role(&member.role) {
            directions.insert(canonical_relation_direction(&route_id, role));
        }
    }
    if route_id == "C1" && (directions.contains("inner") || directions.contains("outer")) {
        directions.retain(|direction| matches!(direction.as_str(), "inner" | "outer"));
    }
    if directions.is_empty() {
        Ok(vec!["forward".into()])
    } else {
        Ok(directions.into_iter().collect())
    }
}

fn is_route_relation(element: &OsmElement) -> bool {
    element.is_relation()
        && element.get_tag("type") == Some("route")
        && (element.get_tag("route").is_some() || element.get_tag("ref").is_some())
}

fn relation_route_id(relation: &OsmElement) -> Result<String, RouteMembershipError> {
    relation
        .get_tag("ref")
        .or_else(|| relation.get_tag("route"))
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} has no non-empty ref or route tag",
                relation.id
            ))
        })
}

fn edge_way_id(edge: &Edge) -> Option<i64> {
    let mut parts = edge.id.split(':');
    if parts.next()? != "e" {
        return None;
    }
    parts.next()?.strip_prefix('w')?.parse().ok()
}

fn graph_node_osm_id(node_id: &str) -> Option<i64> {
    node_id.strip_prefix("n:")?.parse().ok()
}

fn edge_map(graph: &Graph) -> HashMap<&str, &Edge> {
    graph
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect()
}

fn way_by_id_from_response(response: &OverpassResponse, way_id: i64) -> Option<&OsmElement> {
    response
        .elements
        .iter()
        .find(|element| element.is_way() && element.id == way_id)
}

#[derive(Debug, Clone)]
struct RelationMemberEdges {
    member_index: usize,
    reverse: bool,
    way_id: i64,
    is_link: bool,
    from_node_id: String,
    to_node_id: String,
    ordered_edge_ids: Vec<String>,
}

fn route_tag_conflicts(tags: Option<&BTreeMap<String, String>>, route_id: &str) -> bool {
    let Some(tags) = tags else {
        return false;
    };
    ["ref", "nat_ref"].iter().any(|key| {
        let Some(value) = tags.get(*key) else {
            return false;
        };
        let tokens = value
            .split([';', ','])
            .map(normalized)
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        !tokens.is_empty() && !tokens.iter().any(|token| token == &normalized(route_id))
    })
}

fn way_name_matches_relation(relation: &OsmElement, way: &OsmElement) -> bool {
    ["name", "name:en"]
        .iter()
        .filter_map(|key| relation.get_tag(key).map(normalized))
        .filter(|value| !value.is_empty())
        .any(|relation_name| {
            ["name", "name:en"]
                .iter()
                .filter_map(|key| way.get_tag(key).map(normalized))
                .any(|way_name| way_name == relation_name)
        })
}

fn relation_member_is_mainline(
    relation: &OsmElement,
    way: &OsmElement,
    graph: &Graph,
) -> Result<bool, RouteMembershipError> {
    let route_id = relation_route_id(relation)?;
    let Some(highway) = way.get_tag("highway") else {
        return Ok(false);
    };
    if !matches!(highway, "motorway" | "motorway_link") {
        return Ok(false);
    }
    if route_tag_conflicts(way.tags.as_ref(), &route_id) {
        return Ok(false);
    }
    let is_named_ramp = ["name", "name:en", "destination"]
        .iter()
        .filter_map(|key| way.get_tag(key))
        .any(|value| {
            let value = normalized(value);
            value.contains("入口")
                || value.contains("出口")
                || value.contains("entry")
                || value.contains("exit")
        });
    if highway == "motorway_link" && is_named_ramp {
        return Ok(false);
    }
    if highway == "motorway_link"
        && !graph
            .edges
            .iter()
            .any(|edge| edge_way_id(edge) == Some(way.id) && edge.kind == EdgeKind::Shutoko)
    {
        return Err(RouteMembershipError::Relation(format!(
            "relation {} includes motorway_link way {} without a Shutoko graph edge",
            relation.id, way.id
        )));
    }
    if highway == "motorway"
        || way.get_tag("ref").is_some()
        || way_name_matches_relation(relation, way)
        || graph
            .edges
            .iter()
            .any(|edge| edge_way_id(edge) == Some(way.id) && edge.kind == EdgeKind::Shutoko)
    {
        Ok(true)
    } else {
        Err(RouteMembershipError::Relation(format!(
            "relation {} includes motorway_link way {} without matching route identity",
            relation.id, way.id
        )))
    }
}

fn build_relation_segments(
    relation: &OsmElement,
    response: &OverpassResponse,
    graph: &Graph,
    direction: &str,
    source_snapshot_sha256: &str,
) -> Result<Vec<RouteMembershipSegment>, RouteMembershipError> {
    let relation_members = relation.members.as_ref().ok_or_else(|| {
        RouteMembershipError::Relation(format!("relation {} has no members", relation.id))
    })?;
    let route_id = relation_route_id(relation)?;
    let has_role_direction = relation_members
        .iter()
        .filter(|member| member.member_type == "way")
        .any(|member| relation_member_role(&member.role).is_some());
    let edges = edge_map(graph);
    let mut mapped_members = Vec::new();
    let mut seen_way_ids = HashSet::new();
    for (member_index, member) in relation_members.iter().enumerate() {
        if member.member_type != "way" {
            continue;
        }
        let member_role = relation_member_role(&member.role);
        if !member.role.trim().is_empty() && member_role.is_none() {
            return Err(RouteMembershipError::Relation(format!(
                "relation {} member {} has unsupported role {:?}",
                relation.id, member.ref_id, member.role
            )));
        }
        let member_direction = member_role
            .map(|role| canonical_relation_direction(&route_id, role))
            .or_else(|| {
                has_role_direction
                    .then(|| {
                        inferred_blank_member_direction(relation_members, member_index, &route_id)
                    })
                    .flatten()
            });
        let way = way_by_id_from_response(response, member.ref_id).ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} references missing way {}",
                relation.id, member.ref_id
            ))
        })?;
        if !relation_member_is_mainline(relation, way, graph)? {
            continue;
        }
        if has_role_direction && member_direction.as_deref() != Some(direction) {
            if member_direction.is_none() {
                return Err(RouteMembershipError::Relation(format!(
                    "relation {} has an ambiguous directionless mainline member {}",
                    relation.id, member.ref_id
                )));
            }
            continue;
        }
        if !seen_way_ids.insert(member.ref_id) {
            return Err(RouteMembershipError::Relation(format!(
                "relation {} repeats way {}",
                relation.id, member.ref_id
            )));
        }
        mapped_members.extend(map_relation_way_from_response(
            relation,
            member,
            member_index,
            direction,
            response,
            &edges,
        )?);
    }
    if mapped_members.is_empty() {
        return Ok(Vec::new());
    }

    let paths = build_relation_member_paths(graph, relation.id, direction, mapped_members)?;

    let mut result = Vec::with_capacity(paths.len());
    for (index, path) in paths.into_iter().enumerate() {
        let member_indexes = path
            .iter()
            .map(|member| member.member_index)
            .collect::<Vec<_>>();
        let member_order_matches_relation =
            member_indexes.windows(2).all(|pair| pair[1] == pair[0] + 1);
        let ordered_edge_ids = path
            .iter()
            .flat_map(|member| member.ordered_edge_ids.iter().cloned())
            .collect::<Vec<_>>();
        validate_ordered_edges(graph, &ordered_edge_ids, "ordered relation member path")?;
        let ordered_edge_ids_sha256 = ordered_edge_ids_sha256(&ordered_edge_ids)
            .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
        result.push(RouteMembershipSegment {
            segment_id: format!("relation:{}:{}:{}", relation.id, direction, index),
            source_kind: RouteMembershipSourceKind::RelationMainline,
            source_relation_id: Some(relation.id.to_string()),
            source_snapshot_sha256: source_snapshot_sha256.to_string(),
            binding_evidence_id: None,
            ordered_edge_ids,
            ordered_edge_ids_sha256,
            member_indexes: Some(member_indexes),
            member_order_matches_relation: Some(member_order_matches_relation),
        });
    }
    Ok(result)
}

fn way_has_directed_edges(
    edges: &HashMap<&str, &Edge>,
    way_id: i64,
    nodes: &[i64],
    reverse: bool,
) -> bool {
    (0..nodes.len().saturating_sub(1)).all(|index| {
        let (from, to) = if reverse {
            (nodes[index + 1], nodes[index])
        } else {
            (nodes[index], nodes[index + 1])
        };
        let from = format!("n:{}", from);
        let to = format!("n:{}", to);
        edges.values().any(|edge| {
            edge.kind == EdgeKind::Shutoko
                && edge_way_id(edge) == Some(way_id)
                && edge.from == from
                && edge.to == to
        })
    })
}

fn map_relation_way_edges(
    relation: &OsmElement,
    member: &OsmMember,
    member_index: usize,
    response: &OverpassResponse,
    edges: &HashMap<&str, &Edge>,
    reverse: bool,
) -> Result<Option<RelationMemberEdges>, RouteMembershipError> {
    let way = way_by_id_from_response(response, member.ref_id).ok_or_else(|| {
        RouteMembershipError::Relation(format!(
            "relation {} references missing way {}",
            relation.id, member.ref_id
        ))
    })?;
    let nodes = way.nodes.as_ref().ok_or_else(|| {
        RouteMembershipError::Relation(format!(
            "relation {} way {} has no nodes",
            relation.id, member.ref_id
        ))
    })?;
    if nodes.len() < 2 {
        return Err(RouteMembershipError::Relation(format!(
            "relation {} way {} has fewer than two nodes",
            relation.id, member.ref_id
        )));
    }
    if !way_has_directed_edges(edges, member.ref_id, nodes, reverse) {
        return Ok(None);
    }
    let mut ordered_edge_ids = Vec::with_capacity(nodes.len() - 1);
    for index in 0..nodes.len() - 1 {
        let (from, to) = if reverse {
            (nodes[index + 1], nodes[index])
        } else {
            (nodes[index], nodes[index + 1])
        };
        let from = format!("n:{}", from);
        let to = format!("n:{}", to);
        let candidates = edges
            .values()
            .copied()
            .filter(|edge| {
                edge.kind == EdgeKind::Shutoko
                    && edge_way_id(edge) == Some(member.ref_id)
                    && edge.from == from
                    && edge.to == to
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err(RouteMembershipError::Relation(format!(
                "relation {} way {} segment {} has {} exact graph edges for {} -> {}",
                relation.id,
                member.ref_id,
                index,
                candidates.len(),
                from,
                to
            )));
        }
        ordered_edge_ids.push(candidates[0].id.clone());
    }
    let first = edges
        .get(
            ordered_edge_ids
                .first()
                .map(String::as_str)
                .unwrap_or_default(),
        )
        .ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} way {} produced an unknown edge",
                relation.id, member.ref_id
            ))
        })?;
    let last = edges
        .get(
            ordered_edge_ids
                .last()
                .map(String::as_str)
                .unwrap_or_default(),
        )
        .ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} way {} produced an unknown edge",
                relation.id, member.ref_id
            ))
        })?;
    Ok(Some(RelationMemberEdges {
        member_index,
        reverse,
        way_id: member.ref_id,
        is_link: way.get_tag("highway") == Some("motorway_link"),
        from_node_id: first.from.clone(),
        to_node_id: last.to.clone(),
        ordered_edge_ids,
    }))
}

fn map_relation_way_from_response(
    relation: &OsmElement,
    member: &OsmMember,
    member_index: usize,
    direction: &str,
    response: &OverpassResponse,
    edges: &HashMap<&str, &Edge>,
) -> Result<Vec<RelationMemberEdges>, RouteMembershipError> {
    let topology_direction = matches!(
        normalized(direction).as_str(),
        "inner" | "outer" | "inbound" | "outbound"
    );
    let orientations = if topology_direction {
        vec![false, true]
    } else {
        vec![direction_is_reverse(direction)]
    };
    let mut mapped = Vec::new();
    for reverse in orientations {
        if let Some(orientation) =
            map_relation_way_edges(relation, member, member_index, response, edges, reverse)?
        {
            mapped.push(orientation);
        }
    }
    if mapped.is_empty() {
        return Err(RouteMembershipError::Relation(format!(
            "relation {} way {} has no graph edges in requested direction {}",
            relation.id, member.ref_id, direction
        )));
    }
    Ok(mapped)
}

fn build_relation_member_paths(
    graph: &Graph,
    relation_id: i64,
    direction: &str,
    mapped_members: Vec<RelationMemberEdges>,
) -> Result<Vec<Vec<RelationMemberEdges>>, RouteMembershipError> {
    let mut groups = Vec::<Vec<RelationMemberEdges>>::new();
    for member in mapped_members {
        if let Some(group) = groups.last_mut() {
            if group[0].member_index == member.member_index {
                group.push(member);
                continue;
            }
        }
        groups.push(vec![member]);
    }
    let topology_direction = matches!(
        normalized(direction).as_str(),
        "inner" | "outer" | "inbound" | "outbound"
    );
    let preferred_reverse = direction_is_reverse(direction);
    let mut candidates = Vec::new();
    for group in groups {
        if topology_direction {
            candidates.extend(group);
        } else {
            let Some(selected) = group
                .iter()
                .find(|member| member.reverse == preferred_reverse)
                .cloned()
            else {
                return Err(RouteMembershipError::Relation(format!(
                    "relation {relation_id} direction {direction} has no requested-direction edges for member {}",
                    group[0].way_id
                )));
            };
            candidates.push(selected);
        }
    }
    let group_count = candidates
        .iter()
        .map(|candidate| candidate.member_index)
        .collect::<HashSet<_>>()
        .len();
    let mut used_groups = HashSet::new();
    let mut paths = Vec::new();
    while used_groups.len() < group_count {
        let unused = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| !used_groups.contains(&candidate.member_index))
            .collect::<Vec<_>>();
        let roots = unused
            .iter()
            .filter(|(_, candidate)| {
                !unused.iter().any(|(_, other)| {
                    other.member_index != candidate.member_index
                        && other.to_node_id == candidate.from_node_id
                })
            })
            .map(|(index, _)| *index)
            .collect::<Vec<_>>();
        let start = roots
            .into_iter()
            .min_by_key(|index| (candidates[*index].member_index, *index))
            .or_else(|| {
                unused
                    .iter()
                    .map(|(index, _)| *index)
                    .min_by_key(|index| (candidates[*index].member_index, *index))
            })
            .ok_or_else(|| {
                RouteMembershipError::Relation(format!(
                    "relation {relation_id} direction {direction} has no unused member"
                ))
            })?;
        let mut current = start;
        let mut path = Vec::new();
        loop {
            let current_member = &candidates[current];
            if !used_groups.insert(current_member.member_index) {
                return Err(RouteMembershipError::Relation(format!(
                    "relation {relation_id} direction {direction} reuses member {}",
                    current_member.way_id
                )));
            }
            path.push(current_member.clone());
            let successors = candidates
                .iter()
                .enumerate()
                .filter(|(index, candidate)| {
                    *index != current
                        && candidate.member_index != current_member.member_index
                        && !used_groups.contains(&candidate.member_index)
                        && candidate.from_node_id == current_member.to_node_id
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if successors.len() > 1 {
                if !successors.iter().any(|index| candidates[*index].is_link) {
                    return Err(RouteMembershipError::Relation(format!(
                        "relation {relation_id} direction {direction} has an ambiguous directed branch after member {}",
                        current_member.way_id
                    )));
                }
                break;
            }
            let Some(next) = successors.first().copied() else {
                let has_external_successor = graph.edges.iter().any(|edge| {
                    edge.kind == EdgeKind::Shutoko
                        && edge.from == current_member.to_node_id
                        && candidates.iter().any(|candidate| {
                            candidate.member_index != current_member.member_index
                                && !used_groups.contains(&candidate.member_index)
                                && candidate.from_node_id == edge.to
                        })
                });
                if has_external_successor {
                    return Err(RouteMembershipError::Relation(format!(
                        "relation {relation_id} direction {direction} would traverse a way outside the relation after member {}",
                        current_member.way_id
                    )));
                }
                break;
            };
            current = next;
        }
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(RouteMembershipError::Relation(format!(
            "relation {relation_id} direction {direction} has no directed member path"
        )));
    }
    Ok(paths)
}

fn merge_membership(indices: &mut Vec<RouteMembershipIndex>, incoming: RouteMembershipIndex) {
    if let Some(existing) = indices
        .iter_mut()
        .find(|index| index.route_id == incoming.route_id && index.direction == incoming.direction)
    {
        existing.segments.extend(incoming.segments);
    } else {
        indices.push(incoming);
    }
}

pub fn build_relation_memberships(
    response: &OverpassResponse,
    graph: &Graph,
    source_snapshot_sha256: &str,
    relation_ids: Option<&[i64]>,
) -> Result<Vec<RouteMembershipIndex>, RouteMembershipError> {
    validate_sha256(source_snapshot_sha256, "source_snapshot_sha256")?;
    let selected_ids = relation_ids.map(|ids| ids.iter().copied().collect::<HashSet<_>>());
    let mut result = Vec::new();
    let mut relations = response
        .elements
        .iter()
        .filter(|element| is_route_relation(element))
        .filter(|element| {
            selected_ids
                .as_ref()
                .is_none_or(|ids| ids.contains(&element.id))
        })
        .collect::<Vec<_>>();
    relations.sort_by_key(|relation| relation.id);

    for relation in relations {
        let route_id = relation_route_id(relation)?;
        for direction in relation_directions(relation)? {
            let segments = build_relation_segments(
                relation,
                response,
                graph,
                &direction,
                source_snapshot_sha256,
            )?;
            if segments.is_empty() {
                continue;
            }
            merge_membership(
                &mut result,
                RouteMembershipIndex {
                    membership_id: format!("route:{}:{}", route_id, direction),
                    route_id: route_id.clone(),
                    direction,
                    direction_mapping_version: ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION.into(),
                    segments,
                },
            );
        }
    }
    Ok(result)
}

fn validate_ordered_edges<'a>(
    graph: &'a Graph,
    edge_ids: &[String],
    segment_id: &str,
) -> Result<Vec<&'a Edge>, RouteMembershipError> {
    if edge_ids.is_empty() {
        return Err(RouteMembershipError::Segment(format!(
            "{} has an empty ordered edge list",
            segment_id
        )));
    }
    let mut seen = HashSet::new();
    let mut edges = Vec::with_capacity(edge_ids.len());
    for edge_id in edge_ids {
        if !seen.insert(edge_id.as_str()) {
            return Err(RouteMembershipError::Segment(format!(
                "{} repeats edge {} inside one segment",
                segment_id, edge_id
            )));
        }
        let edge = graph
            .edges
            .iter()
            .find(|edge| edge.id == *edge_id)
            .ok_or_else(|| {
                RouteMembershipError::Segment(format!(
                    "{} references missing graph edge {}",
                    segment_id, edge_id
                ))
            })?;
        edges.push(edge);
    }
    for pair in edges.windows(2) {
        if pair[0].to != pair[1].from {
            return Err(RouteMembershipError::Segment(format!(
                "{} is disconnected between {} and {}",
                segment_id, pair[0].id, pair[1].id
            )));
        }
    }
    Ok(edges)
}

fn collect_connector_paths(
    graph: &Graph,
    way_id: i64,
    current: &str,
    target: &str,
    path: &mut Vec<String>,
    visited_nodes: &mut HashSet<String>,
    paths: &mut Vec<Vec<String>>,
) -> Result<(), RouteMembershipError> {
    if path.len() > 10_000 {
        return Err(RouteMembershipError::BudgetExceeded(
            "excluded short connector search exceeded 10000 edges".into(),
        ));
    }
    let mut outgoing: Vec<&Edge> = graph
        .edges
        .iter()
        .filter(|edge| {
            edge.kind == EdgeKind::Shutoko
                && edge_way_id(edge) == Some(way_id)
                && edge.from == current
        })
        .collect();
    outgoing.sort_by(|left, right| left.id.cmp(&right.id));
    for edge in outgoing {
        if edge.to == target {
            let mut candidate = path.clone();
            candidate.push(edge.id.clone());
            paths.push(candidate);
            if paths.len() > 1 {
                return Err(RouteMembershipError::Validation(
                    "excluded short connector has multiple directed paths".into(),
                ));
            }
            continue;
        }
        if !visited_nodes.insert(edge.to.clone()) {
            continue;
        }
        path.push(edge.id.clone());
        collect_connector_paths(graph, way_id, &edge.to, target, path, visited_nodes, paths)?;
        path.pop();
        visited_nodes.remove(&edge.to);
    }
    Ok(())
}

fn resolve_excluded_short_connector(
    graph: &Graph,
    connector: &crate::seed::ExcludedShortConnector,
    expected_from_node_id: &str,
    expected_to_node_id: &str,
) -> Result<Vec<String>, RouteMembershipError> {
    if connector.from_node_id == connector.to_node_id
        || connector.from_node_id.is_empty()
        || connector.to_node_id.is_empty()
        || connector.edge_count == 0
        || connector.osm_way_id <= 0
        || connector.distance_meters == 0
    {
        return Err(RouteMembershipError::Validation(
            "excluded short connector has invalid endpoints or evidence".into(),
        ));
    }
    if connector.from_node_id != expected_from_node_id
        || connector.to_node_id != expected_to_node_id
    {
        return Err(RouteMembershipError::Validation(format!(
            "excluded short connector endpoints {}-{} do not match anchor B-to-M {}-{}",
            connector.from_node_id,
            connector.to_node_id,
            expected_from_node_id,
            expected_to_node_id
        )));
    }
    let mut paths = Vec::new();
    let mut path = Vec::new();
    let mut visited_nodes = HashSet::from([connector.from_node_id.clone()]);
    collect_connector_paths(
        graph,
        connector.osm_way_id,
        &connector.from_node_id,
        &connector.to_node_id,
        &mut path,
        &mut visited_nodes,
        &mut paths,
    )?;
    let Some(edge_ids) = paths.into_iter().next() else {
        return Err(RouteMembershipError::Validation(
            "excluded short connector has no directed graph path".into(),
        ));
    };
    let edges = validate_ordered_edges(graph, &edge_ids, "excluded short connector")?;
    let distance = edges
        .iter()
        .map(|edge| edge.distance_meters)
        .fold(0_u64, u64::saturating_add);
    if edge_ids.len() != connector.edge_count as usize || distance != connector.distance_meters {
        return Err(RouteMembershipError::Validation(format!(
            "excluded short connector evidence does not match graph (edges={}, distance={})",
            edge_ids.len(),
            distance
        )));
    }
    Ok(edge_ids)
}

fn relation_lap_edge_ids(
    graph: &Graph,
    segment: &RouteMembershipSegment,
    first_edge_id: &str,
    last_edge_id: &str,
    merge_node_id: &str,
    branch_node_id: &str,
) -> Result<Vec<String>, RouteMembershipError> {
    let sequence = &segment.ordered_edge_ids;
    let first_positions: Vec<usize> = sequence
        .iter()
        .enumerate()
        .filter_map(|(index, edge_id)| (edge_id == first_edge_id).then_some(index))
        .collect();
    let last_positions: Vec<usize> = sequence
        .iter()
        .enumerate()
        .filter_map(|(index, edge_id)| (edge_id == last_edge_id).then_some(index))
        .collect();
    if first_positions.len() != 1 || last_positions.len() != 1 {
        return Err(RouteMembershipError::Validation(
            "mandatory lap boundaries must each occur once in a relationMainline segment".into(),
        ));
    }
    let first = first_positions[0];
    let last = last_positions[0];
    let mut edge_ids = Vec::new();
    if first <= last {
        edge_ids.extend_from_slice(&sequence[first..=last]);
    } else {
        edge_ids.extend_from_slice(&sequence[first..]);
        edge_ids.extend_from_slice(&sequence[..=last]);
    }
    let edges = validate_ordered_edges(graph, &edge_ids, &segment.segment_id)?;
    if edges.first().map(|edge| edge.from.as_str()) != Some(merge_node_id)
        || edges.last().map(|edge| edge.to.as_str()) != Some(branch_node_id)
    {
        return Err(RouteMembershipError::Validation(
            "mandatory lap does not run from M to B on the declared arm".into(),
        ));
    }
    Ok(edge_ids)
}

fn find_matching_membership<'a>(
    route_memberships: &'a [RouteMembershipIndex],
    route_id: &str,
    direction: &str,
) -> Result<&'a RouteMembershipIndex, RouteMembershipError> {
    let matches: Vec<&RouteMembershipIndex> = route_memberships
        .iter()
        .filter(|membership| membership.route_id == route_id && membership.direction == direction)
        .collect();
    if matches.len() != 1 {
        return Err(RouteMembershipError::Validation(format!(
            "route {} direction {} must resolve to exactly one membership (matches={})",
            route_id,
            direction,
            matches.len()
        )));
    }
    Ok(matches[0])
}

fn validate_ordered_edge_sequence_allowing_repeats<'a>(
    graph: &'a Graph,
    edge_ids: &[String],
    segment_id: &str,
) -> Result<Vec<&'a Edge>, RouteMembershipError> {
    if edge_ids.is_empty() {
        return Err(RouteMembershipError::Segment(format!(
            "{} has an empty ordered edge list",
            segment_id
        )));
    }
    let mut edges = Vec::with_capacity(edge_ids.len());
    for edge_id in edge_ids {
        let edge = graph
            .edges
            .iter()
            .find(|edge| edge.id == *edge_id)
            .ok_or_else(|| {
                RouteMembershipError::Segment(format!(
                    "{} references missing graph edge {}",
                    segment_id, edge_id
                ))
            })?;
        edges.push(edge);
    }
    for pair in edges.windows(2) {
        if pair[0].to != pair[1].from {
            return Err(RouteMembershipError::Segment(format!(
                "{} is disconnected between {} and {}",
                segment_id, pair[0].id, pair[1].id
            )));
        }
    }
    Ok(edges)
}

fn validate_segment_hash(segment: &RouteMembershipSegment) -> Result<(), RouteMembershipError> {
    validate_sha256(&segment.source_snapshot_sha256, "sourceSnapshotSha256")?;
    validate_sha256(&segment.ordered_edge_ids_sha256, "orderedEdgeIdsSha256")?;
    let expected = ordered_edge_ids_sha256(&segment.ordered_edge_ids)
        .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
    if expected != segment.ordered_edge_ids_sha256 {
        return Err(RouteMembershipError::Segment(format!(
            "{} orderedEdgeIdsSha256 does not match the ordered edge list",
            segment.segment_id
        )));
    }
    Ok(())
}

pub fn build_bound_ramp_memberships(
    graph: &Graph,
    source_snapshot_sha256: &str,
    evidence: &[BoundRampEvidence],
) -> Result<Vec<RouteMembershipIndex>, RouteMembershipError> {
    validate_sha256(source_snapshot_sha256, "source_snapshot_sha256")?;
    let mut result = Vec::new();
    let mut evidence_ids = HashSet::new();
    for item in evidence {
        if item.binding_evidence_id.is_empty()
            || item.ramp_id.is_empty()
            || item.route_id.is_empty()
            || item.direction.is_empty()
            || item.from_node_id.is_empty()
            || item.to_node_id.is_empty()
        {
            return Err(RouteMembershipError::RampBinding(
                "bound ramp evidence has an empty identity field".into(),
            ));
        }
        if !evidence_ids.insert(item.binding_evidence_id.as_str()) {
            return Err(RouteMembershipError::RampBinding(format!(
                "duplicate binding evidence id {}",
                item.binding_evidence_id
            )));
        }
        let segment_id = format!("binding:{}:segment:0", item.binding_evidence_id);
        validate_sha256(&item.edge_ids_sha256, "edge_ids_sha256")?;
        let evidence_hash = ordered_edge_ids_sha256(&item.edge_ids)
            .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
        if evidence_hash != item.edge_ids_sha256 {
            return Err(RouteMembershipError::RampBinding(format!(
                "{} edge_ids_sha256 does not match the ordered edge list",
                segment_id
            )));
        }
        let edges = validate_ordered_edges(graph, &item.edge_ids, &segment_id)?;
        if edges.first().map(|edge| &edge.from) != Some(&item.from_node_id)
            || edges.last().map(|edge| &edge.to) != Some(&item.to_node_id)
        {
            return Err(RouteMembershipError::RampBinding(format!(
                "{} endpoints do not match its ordered graph edges",
                segment_id
            )));
        }
        validate_way_order(&item.osm_way_ids, &edges, &segment_id)?;
        if item.osm_node_ids.len() != edges.len() + 1
            || item.osm_node_ids.first().copied() != graph_node_osm_id(&item.from_node_id)
            || item.osm_node_ids.last().copied() != graph_node_osm_id(&item.to_node_id)
        {
            return Err(RouteMembershipError::RampBinding(format!(
                "{} osm node order does not match its ordered graph edges",
                segment_id
            )));
        }
        if edges
            .iter()
            .any(|edge| !matches!(edge.kind, EdgeKind::Entry | EdgeKind::Exit))
        {
            return Err(RouteMembershipError::RampBinding(format!(
                "{} contains an edge that is not an Entry or Exit ramp edge",
                segment_id
            )));
        }
        let ramp = graph
            .ramps
            .iter()
            .find(|ramp| ramp.id == item.ramp_id)
            .ok_or_else(|| {
                RouteMembershipError::RampBinding(format!(
                    "ramp {} is not present in the graph ramp bindings",
                    item.ramp_id
                ))
            })?;
        let expected_kind = match ramp.kind {
            RampKind::GeneralEntry => EdgeKind::Entry,
            RampKind::GeneralExit => EdgeKind::Exit,
            _ => {
                return Err(RouteMembershipError::RampBinding(format!(
                    "ramp {} is not a general ramp",
                    item.ramp_id
                )))
            }
        };
        if ramp.route != item.route_id || ramp.direction != item.direction {
            return Err(RouteMembershipError::RampBinding(format!(
                "ramp {} route/direction conflicts with its evidence",
                item.ramp_id
            )));
        }
        if edges.iter().any(|edge| edge.kind != expected_kind) {
            return Err(RouteMembershipError::RampBinding(format!(
                "ramp {} edge kind conflicts with its inventory kind",
                item.ramp_id
            )));
        }
        if edges.first().map(|edge| edge.id.as_str()) != Some(ramp.edge_id.as_str()) {
            return Err(RouteMembershipError::RampBinding(format!(
                "ramp {} exact graph edge {} is not the first edge of its evidence",
                item.ramp_id, ramp.edge_id
            )));
        }
        merge_membership(
            &mut result,
            RouteMembershipIndex {
                membership_id: format!("route:{}:{}", item.route_id, item.direction),
                route_id: item.route_id.clone(),
                direction: item.direction.clone(),
                direction_mapping_version: ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION.into(),
                segments: vec![RouteMembershipSegment {
                    segment_id,
                    source_kind: RouteMembershipSourceKind::BoundRamp,
                    source_relation_id: None,
                    source_snapshot_sha256: source_snapshot_sha256.to_string(),
                    binding_evidence_id: Some(item.binding_evidence_id.clone()),
                    ordered_edge_ids: item.edge_ids.clone(),
                    ordered_edge_ids_sha256: evidence_hash,
                    member_indexes: None,
                    member_order_matches_relation: None,
                }],
            },
        );
    }
    Ok(result)
}

fn validate_way_order(
    declared_way_ids: &[i64],
    edges: &[&Edge],
    segment_id: &str,
) -> Result<(), RouteMembershipError> {
    if declared_way_ids.is_empty() {
        return Err(RouteMembershipError::RampBinding(format!(
            "{} has no declared OSM way IDs",
            segment_id
        )));
    }
    let mut actual_way_ids = Vec::new();
    for edge in edges {
        let actual = edge_way_id(edge).ok_or_else(|| {
            RouteMembershipError::RampBinding(format!(
                "{} contains edge {} without an OSM way ID",
                segment_id, edge.id
            ))
        })?;
        if actual_way_ids.last() != Some(&actual) {
            actual_way_ids.push(actual);
        }
    }
    if actual_way_ids != declared_way_ids {
        return Err(RouteMembershipError::RampBinding(format!(
            "{} OSM way order {:?} does not match edge order {:?}",
            segment_id, declared_way_ids, actual_way_ids
        )));
    }
    Ok(())
}

pub fn bound_ramp_evidence_from_inventory(
    graph: &Graph,
    inventory: &RampInventoryFile,
    bindings: &OsmRampBindingsFile,
) -> Result<Vec<BoundRampEvidence>, RouteMembershipError> {
    let binding_by_ramp: HashMap<&str, &OsmRampBinding> = bindings
        .bindings
        .iter()
        .map(|binding| (binding.ramp_id.as_str(), binding))
        .collect();
    let mut result = Vec::new();
    for item in &inventory.ramps {
        if !is_verified_general_ramp(item) {
            continue;
        }
        let binding = binding_by_ramp.get(item.ramp_id.as_str()).ok_or_else(|| {
            RouteMembershipError::RampBinding(format!(
                "verified ramp {} has no exact binding",
                item.ramp_id
            ))
        })?;
        let expected_kind = match item.kind {
            RampKind::GeneralEntry => EdgeKind::Entry,
            RampKind::GeneralExit => EdgeKind::Exit,
            _ => unreachable!(),
        };
        let ground = format!("n:{}", binding.osm_node_id);
        let motorway = format!("n:{}", binding.motorway_node_id);
        let (from, to) = if item.kind == RampKind::GeneralEntry {
            (ground, motorway)
        } else {
            (motorway, ground)
        };
        let mut edge_ids = Vec::new();
        let first_osm_node_id = graph_node_osm_id(&from).ok_or_else(|| {
            RouteMembershipError::RampBinding(format!(
                "ramp {} evidence start {} is not an OSM node ID",
                item.ramp_id, from
            ))
        })?;
        let mut osm_node_ids = vec![first_osm_node_id];
        let mut current_node = from.clone();
        let mut visited = HashSet::new();
        while current_node != to {
            let next_edges = graph
                .edges
                .iter()
                .filter(|edge| {
                    edge.kind == expected_kind
                        && edge_way_id(edge) == Some(binding.osm_way_id)
                        && edge.from == current_node
                })
                .collect::<Vec<_>>();
            if next_edges.len() != 1 {
                return Err(RouteMembershipError::RampBinding(format!(
                    "ramp {} has {} exact next graph edges at {}",
                    item.ramp_id,
                    next_edges.len(),
                    current_node
                )));
            }
            let next = next_edges[0];
            if !visited.insert(next.id.as_str()) {
                return Err(RouteMembershipError::RampBinding(format!(
                    "ramp {} binding path is cyclic",
                    item.ramp_id
                )));
            }
            edge_ids.push(next.id.clone());
            let Some(next_osm_node_id) = graph_node_osm_id(&next.to) else {
                return Err(RouteMembershipError::RampBinding(format!(
                    "ramp {} evidence node {} is not an OSM node ID",
                    item.ramp_id, next.to
                )));
            };
            osm_node_ids.push(next_osm_node_id);
            current_node = next.to.clone();
        }
        if edge_ids.is_empty() {
            return Err(RouteMembershipError::RampBinding(format!(
                "ramp {} binding has an empty directed path",
                item.ramp_id
            )));
        }
        let edge_ids_sha256 = ordered_edge_ids_sha256(&edge_ids)
            .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
        result.push(BoundRampEvidence {
            binding_evidence_id: format!("osm-ramp-binding:{}", item.ramp_id),
            ramp_id: item.ramp_id.clone(),
            route_id: item.route.clone(),
            direction: item.direction.clone(),
            osm_way_ids: vec![binding.osm_way_id],
            osm_node_ids,
            from_node_id: from,
            to_node_id: to,
            edge_ids,
            edge_ids_sha256,
        });
    }
    Ok(result)
}

fn is_verified_general_ramp(item: &CanonicalRampInventoryItem) -> bool {
    item.status == "active"
        && matches!(item.kind, RampKind::GeneralEntry | RampKind::GeneralExit)
        && item.support_state.as_deref() == Some("verified_bound")
}

pub fn build_route_memberships(
    response: &OverpassResponse,
    graph: &Graph,
    options: &RouteMembershipBuildOptions,
) -> Result<Vec<RouteMembershipIndex>, RouteMembershipError> {
    build_route_membership_indices(response, graph, options)
}

pub fn build_route_membership_indices(
    response: &OverpassResponse,
    graph: &Graph,
    options: &RouteMembershipBuildOptions,
) -> Result<Vec<RouteMembershipIndex>, RouteMembershipError> {
    let mut result = build_relation_memberships(
        response,
        graph,
        &options.source_snapshot_sha256,
        options.relation_ids.as_deref(),
    )?;
    let bound = build_bound_ramp_memberships(
        graph,
        &options.source_snapshot_sha256,
        &options.bound_ramp_evidence,
    )?;
    for membership in bound {
        merge_membership(&mut result, membership);
    }
    validate_route_memberships(
        &result,
        graph,
        response,
        &options.source_snapshot_sha256,
        &options.bound_ramp_evidence,
    )?;
    Ok(result)
}

fn relation_from_source_id<'a>(
    response: &'a OverpassResponse,
    source_relation_id: &str,
) -> Option<&'a OsmElement> {
    response.elements.iter().find(|element| {
        if !is_route_relation(element) {
            return false;
        }
        element.id.to_string() == source_relation_id
            || source_relation_id
                .strip_prefix("osm:relation:")
                .is_some_and(|value| value == element.id.to_string())
    })
}

fn relation_expected_segments(
    relation: &OsmElement,
    response: &OverpassResponse,
    graph: &Graph,
    direction: &str,
    source_snapshot_sha256: &str,
) -> Result<Vec<RouteMembershipSegment>, RouteMembershipError> {
    build_relation_segments(relation, response, graph, direction, source_snapshot_sha256)
}

pub fn validate_route_membership_structure(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    source_snapshot_sha256: &str,
) -> Result<(), RouteMembershipError> {
    validate_sha256(source_snapshot_sha256, "source_snapshot_sha256")?;
    let mut membership_ids = HashSet::new();
    let mut segment_ids = HashSet::new();
    let mut binding_evidence_ids = HashSet::new();
    for membership in route_memberships {
        if membership.membership_id.is_empty()
            || membership.route_id.is_empty()
            || membership.direction.is_empty()
            || membership.segments.is_empty()
            || membership.membership_id
                != format!("route:{}:{}", membership.route_id, membership.direction)
            || membership.direction_mapping_version != ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION
        {
            return Err(RouteMembershipError::Validation(
                "route membership has an empty identity or no segments".into(),
            ));
        }
        if !membership_ids.insert(membership.membership_id.as_str()) {
            return Err(RouteMembershipError::Validation(format!(
                "duplicate membership id {}",
                membership.membership_id
            )));
        }
        for segment in &membership.segments {
            if segment.segment_id.is_empty() {
                return Err(RouteMembershipError::Validation(
                    "route membership segment ID must not be empty".into(),
                ));
            }
            if !segment_ids.insert(segment.segment_id.as_str()) {
                return Err(RouteMembershipError::Validation(format!(
                    "duplicate segment id {}",
                    segment.segment_id
                )));
            }
            validate_segment_hash(segment)?;
            if segment.source_snapshot_sha256 != source_snapshot_sha256 {
                return Err(RouteMembershipError::Validation(format!(
                    "{} references a different source snapshot",
                    segment.segment_id
                )));
            }
            let edges =
                validate_ordered_edges(graph, &segment.ordered_edge_ids, &segment.segment_id)?;
            match segment.source_kind {
                RouteMembershipSourceKind::RelationMainline => {
                    let member_metadata_valid =
                        segment.member_indexes.as_ref().is_some_and(|indexes| {
                            !indexes.is_empty()
                                && indexes.iter().collect::<HashSet<_>>().len() == indexes.len()
                                && segment.member_order_matches_relation
                                    == Some(indexes.windows(2).all(|pair| pair[1] == pair[0] + 1))
                        });
                    if segment
                        .source_relation_id
                        .as_deref()
                        .is_none_or(str::is_empty)
                        || segment.binding_evidence_id.is_some()
                        || !member_metadata_valid
                        || edges.iter().any(|edge| edge.kind != EdgeKind::Shutoko)
                    {
                        return Err(RouteMembershipError::Validation(format!(
                            "{} has invalid relationMainline provenance or edge kind",
                            segment.segment_id
                        )));
                    }
                }
                RouteMembershipSourceKind::BoundRamp => {
                    if segment.source_relation_id.is_some()
                        || segment
                            .binding_evidence_id
                            .as_deref()
                            .is_none_or(str::is_empty)
                        || segment.member_indexes.is_some()
                        || segment.member_order_matches_relation.is_some()
                        || !binding_evidence_ids
                            .insert(segment.binding_evidence_id.as_deref().unwrap_or_default())
                        || edges
                            .iter()
                            .any(|edge| !matches!(edge.kind, EdgeKind::Entry | EdgeKind::Exit))
                    {
                        return Err(RouteMembershipError::Validation(format!(
                            "{} has invalid boundRamp provenance or edge kind",
                            segment.segment_id
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn validate_route_memberships(
    route_memberships: &[RouteMembershipIndex],
    graph: &Graph,
    response: &OverpassResponse,
    source_snapshot_sha256: &str,
    bound_ramp_evidence: &[BoundRampEvidence],
) -> Result<(), RouteMembershipError> {
    validate_route_membership_structure(graph, route_memberships, source_snapshot_sha256)?;
    let expected_bound =
        build_bound_ramp_memberships(graph, source_snapshot_sha256, bound_ramp_evidence)?;
    let expected_bound_segments = expected_bound
        .iter()
        .flat_map(|membership| membership.segments.iter())
        .filter_map(|segment| {
            segment
                .binding_evidence_id
                .as_deref()
                .map(|evidence_id| (evidence_id, segment))
        })
        .collect::<HashMap<_, _>>();
    let mut actual_bound_segment_ids = HashSet::new();
    let mut relation_segment_keys = HashSet::new();
    for membership in route_memberships {
        let relation_segments: Vec<&RouteMembershipSegment> = membership
            .segments
            .iter()
            .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
            .collect();
        if !relation_segments.is_empty() {
            let mut relation_groups: BTreeMap<&str, Vec<&RouteMembershipSegment>> = BTreeMap::new();
            for segment in &relation_segments {
                let source_relation_id =
                    segment.source_relation_id.as_deref().ok_or_else(|| {
                        RouteMembershipError::Relation(
                            "relation segment has no source relation".into(),
                        )
                    })?;
                relation_groups
                    .entry(source_relation_id)
                    .or_default()
                    .push(segment);
            }
            for (source_relation_id, grouped_segments) in relation_groups {
                let relation =
                    relation_from_source_id(response, source_relation_id).ok_or_else(|| {
                        RouteMembershipError::Relation(format!(
                            "source relation {} is not present in the OSM snapshot",
                            source_relation_id
                        ))
                    })?;
                if relation_route_id(relation)? != membership.route_id
                    || !relation_directions(relation)?
                        .iter()
                        .any(|direction| direction == &membership.direction)
                {
                    return Err(RouteMembershipError::Relation(format!(
                        "membership {} does not match relation {} route/direction",
                        membership.membership_id, source_relation_id
                    )));
                }
                let expected = relation_expected_segments(
                    relation,
                    response,
                    graph,
                    &membership.direction,
                    source_snapshot_sha256,
                )?;
                let expected_keys: Vec<(String, Vec<String>)> = expected
                    .iter()
                    .map(|segment| {
                        (
                            segment.ordered_edge_ids_sha256.clone(),
                            segment.ordered_edge_ids.clone(),
                        )
                    })
                    .collect();
                let actual_keys: Vec<(String, Vec<String>)> = grouped_segments
                    .iter()
                    .map(|segment| {
                        (
                            segment.ordered_edge_ids_sha256.clone(),
                            segment.ordered_edge_ids.clone(),
                        )
                    })
                    .collect();
                if expected_keys != actual_keys {
                    return Err(RouteMembershipError::Relation(format!(
                        "membership {} does not exactly preserve relation {} ordered members",
                        membership.membership_id, source_relation_id
                    )));
                }
                for segment in grouped_segments {
                    let key = format!(
                        "{}:{}:{}",
                        source_relation_id, membership.direction, segment.ordered_edge_ids_sha256
                    );
                    if !relation_segment_keys.insert(key.clone()) {
                        return Err(RouteMembershipError::Relation(format!(
                            "relation segment {} is assigned more than once (key={})",
                            segment.segment_id, key
                        )));
                    }
                }
            }
        }
        for segment in membership
            .segments
            .iter()
            .filter(|segment| segment.source_kind == RouteMembershipSourceKind::BoundRamp)
        {
            let evidence_id = segment.binding_evidence_id.as_deref().ok_or_else(|| {
                RouteMembershipError::RampBinding("bound ramp segment has no evidence id".into())
            })?;
            let expected_segment = expected_bound_segments
                .get(evidence_id)
                .copied()
                .ok_or_else(|| {
                    RouteMembershipError::RampBinding(format!(
                        "{} references unknown binding evidence {}",
                        segment.segment_id, evidence_id
                    ))
                })?;
            if expected_segment != segment {
                return Err(RouteMembershipError::RampBinding(format!(
                    "{} does not match its exact binding evidence",
                    segment.segment_id
                )));
            }
            actual_bound_segment_ids.insert(segment.segment_id.as_str());
        }
    }
    if actual_bound_segment_ids.len() != expected_bound_segments.len() {
        return Err(RouteMembershipError::RampBinding(format!(
            "route memberships contain {} bound ramp segments but evidence yields {}",
            actual_bound_segment_ids.len(),
            expected_bound_segments.len()
        )));
    }
    Ok(())
}

pub fn validate_mandatory_lap(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    edge_ids: &[String],
    excluded_edge_ids: &[String],
    excluded_way_id: Option<i64>,
) -> Result<(), RouteMembershipError> {
    if edge_ids.is_empty() {
        return Err(RouteMembershipError::Segment(
            "mandatory lap edge list is empty".into(),
        ));
    }
    let edges = validate_ordered_edges(graph, edge_ids, "mandatory lap")?;
    if edge_ids.iter().collect::<HashSet<_>>().len() != edge_ids.len() {
        return Err(RouteMembershipError::Segment(
            "mandatory lap repeats an edge inside one route-plan segment".into(),
        ));
    }
    if contains_forbidden_transition(edge_ids, &graph.forbidden_transitions) {
        return Err(RouteMembershipError::Validation(
            "mandatory lap contains a forbidden transition".into(),
        ));
    }
    let excluded: HashSet<&str> = excluded_edge_ids.iter().map(String::as_str).collect();
    let membership = route_memberships
        .iter()
        .find(|membership| membership.membership_id == membership_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!("unknown membership {}", membership_id))
        })?;
    if edge_ids.iter().any(|edge_id| {
        excluded.contains(edge_id.as_str())
            || excluded_way_id.is_some_and(|way_id| {
                edges
                    .iter()
                    .find(|edge| edge.id == *edge_id)
                    .copied()
                    .and_then(edge_way_id)
                    == Some(way_id)
            })
    }) {
        return Err(RouteMembershipError::Segment(
            "mandatory lap uses an excluded short connector".into(),
        ));
    }
    let mut matches = 0usize;
    for segment in membership
        .segments
        .iter()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
    {
        validate_segment_hash(segment)?;
        let source = &segment.ordered_edge_ids;
        let is_contiguous = edge_ids.len() <= source.len()
            && (0..source.len()).any(|start| {
                (0..edge_ids.len())
                    .all(|offset| source[(start + offset) % source.len()] == edge_ids[offset])
            });
        if is_contiguous {
            matches += 1;
        }
    }
    if matches != 1 {
        return Err(RouteMembershipError::Validation(format!(
            "mandatory lap is not one contiguous relationMainline subsequence of {} (matches={})",
            membership_id, matches
        )));
    }
    Ok(())
}

pub fn validate_directed_junction_mandatory_lap(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    anchor: &DirectedJunctionAnchor,
    mandatory_lap: &MandatoryLap,
) -> Result<(), RouteMembershipError> {
    if anchor.merge_node_id == anchor.branch_node_id {
        return Err(RouteMembershipError::Validation(
            "directed junction M and B must be different nodes".into(),
        ));
    }
    if mandatory_lap.lap_count != 1 {
        return Err(RouteMembershipError::Validation(
            "directed mandatory lap must have lapCount=1".into(),
        ));
    }
    let membership = route_memberships
        .iter()
        .find(|membership| membership.membership_id == mandatory_lap.membership_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "unknown mandatory lap membership {}",
                mandatory_lap.membership_id
            ))
        })?;
    if membership.route_id != anchor.route_id || membership.direction != anchor.direction {
        return Err(RouteMembershipError::Validation(format!(
            "mandatory lap membership {} does not match anchor {}/{}",
            mandatory_lap.membership_id, anchor.route_id, anchor.direction
        )));
    }
    let merge_terminal = graph
        .edges
        .iter()
        .find(|edge| edge.id == anchor.merge_terminal_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "merge terminal edge {} is absent from graph",
                anchor.merge_terminal_edge_id
            ))
        })?;
    let branch_initial = graph
        .edges
        .iter()
        .find(|edge| edge.id == anchor.branch_initial_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "branch initial edge {} is absent from graph",
                anchor.branch_initial_edge_id
            ))
        })?;
    if merge_terminal.kind != EdgeKind::Shutoko
        || branch_initial.kind != EdgeKind::Shutoko
        || merge_terminal.to != anchor.merge_node_id
        || branch_initial.from != anchor.branch_node_id
    {
        return Err(RouteMembershipError::Validation(
            "directed junction M/B boundaries do not match their terminal edges".into(),
        ));
    }

    let connector_edge_ids = resolve_excluded_short_connector(
        graph,
        &anchor.excluded_short_connector,
        &anchor.branch_node_id,
        &anchor.merge_node_id,
    )?;
    let connector_set: HashSet<&str> = connector_edge_ids.iter().map(String::as_str).collect();
    let mut matches = Vec::new();
    for segment in membership
        .segments
        .iter()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
    {
        validate_segment_hash(segment)?;
        let Ok(lap_edge_ids) = relation_lap_edge_ids(
            graph,
            segment,
            &mandatory_lap.first_edge_id,
            &mandatory_lap.last_edge_id,
            &anchor.merge_node_id,
            &anchor.branch_node_id,
        ) else {
            continue;
        };
        if lap_edge_ids
            .iter()
            .any(|edge_id| connector_set.contains(edge_id.as_str()))
        {
            return Err(RouteMembershipError::Segment(
                "mandatory lap uses an excluded short connector".into(),
            ));
        }
        matches.push((segment, lap_edge_ids));
    }
    if matches.len() != 1 {
        return Err(RouteMembershipError::Validation(format!(
            "directed mandatory lap boundaries must resolve to one M-to-B relationMainline path (matches={})",
            matches.len()
        )));
    }
    let (_segment, lap_edge_ids) = &matches[0];
    validate_mandatory_lap(
        graph,
        route_memberships,
        &mandatory_lap.membership_id,
        lap_edge_ids,
        &connector_edge_ids,
        None,
    )?;
    if lap_edge_ids.len() <= connector_edge_ids.len() {
        return Err(RouteMembershipError::Segment(
            "ordinary long arc is not longer than the excluded short connector".into(),
        ));
    }
    let lap_distance = graph
        .edges
        .iter()
        .filter(|edge| lap_edge_ids.contains(&edge.id))
        .map(|edge| edge.distance_meters)
        .sum::<u64>();
    if lap_distance <= anchor.excluded_short_connector.distance_meters {
        return Err(RouteMembershipError::Segment(
            "ordinary long arc is not longer than the excluded short connector".into(),
        ));
    }
    if contains_forbidden_transition(lap_edge_ids, &graph.forbidden_transitions) {
        return Err(RouteMembershipError::Validation(
            "mandatory lap contains a forbidden transition".into(),
        ));
    }
    Ok(())
}

pub fn generate_route_plan_lap_v1(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    anchor: &DirectedJunctionAnchor,
) -> Result<RoutePlanLapV1, RouteMembershipError> {
    let membership =
        find_matching_membership(route_memberships, &anchor.route_id, &anchor.direction)?;
    let connector_edge_ids = resolve_excluded_short_connector(
        graph,
        &anchor.excluded_short_connector,
        &anchor.branch_node_id,
        &anchor.merge_node_id,
    )?;
    let connector_set: HashSet<&str> = connector_edge_ids.iter().map(String::as_str).collect();
    let mut candidates: Vec<(&RouteMembershipSegment, Vec<String>, u64)> = Vec::new();
    for segment in membership
        .segments
        .iter()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
    {
        let edges = validate_ordered_edges(graph, &segment.ordered_edge_ids, &segment.segment_id)?;
        let first_edges: Vec<&Edge> = edges
            .iter()
            .filter(|edge| edge.from == anchor.merge_node_id)
            .copied()
            .collect();
        let last_edges: Vec<&Edge> = edges
            .iter()
            .filter(|edge| edge.to == anchor.branch_node_id)
            .copied()
            .collect();
        for first in &first_edges {
            for last in &last_edges {
                let Ok(edge_ids) = relation_lap_edge_ids(
                    graph,
                    segment,
                    &first.id,
                    &last.id,
                    &anchor.merge_node_id,
                    &anchor.branch_node_id,
                ) else {
                    continue;
                };
                if edge_ids
                    .iter()
                    .any(|edge_id| connector_set.contains(edge_id.as_str()))
                    || edge_ids.len() <= connector_edge_ids.len()
                    || contains_forbidden_transition(&edge_ids, &graph.forbidden_transitions)
                {
                    continue;
                }
                let distance = edge_ids
                    .iter()
                    .filter_map(|edge_id| {
                        graph
                            .edges
                            .iter()
                            .find(|edge| edge.id == *edge_id)
                            .map(|edge| edge.distance_meters)
                    })
                    .sum();
                candidates.push((segment, edge_ids, distance));
            }
        }
    }
    if candidates.is_empty() {
        return Err(RouteMembershipError::Validation(
            "no ordinary long relationMainline arc resolves from M to B".into(),
        ));
    }
    candidates.sort_by(|left, right| {
        right
            .1
            .len()
            .cmp(&left.1.len())
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.segment_id.cmp(&right.0.segment_id))
    });
    if candidates.len() > 1
        && candidates[0].1.len() == candidates[1].1.len()
        && candidates[0].2 == candidates[1].2
    {
        return Err(RouteMembershipError::Validation(
            "M-to-B ordinary long arc is ambiguous".into(),
        ));
    }
    let (segment, edge_ids, _) = &candidates[0];
    let mandatory_lap = MandatoryLap {
        membership_id: membership.membership_id.clone(),
        first_edge_id: edge_ids.first().cloned().unwrap_or_default(),
        last_edge_id: edge_ids.last().cloned().unwrap_or_default(),
        lap_count: 1,
    };
    validate_directed_junction_mandatory_lap(graph, route_memberships, anchor, &mandatory_lap)?;
    let edge_ids_sha256 = ordered_edge_ids_sha256(edge_ids)
        .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
    Ok(RoutePlanLapV1 {
        merge_node_id: anchor.merge_node_id.clone(),
        branch_node_id: anchor.branch_node_id.clone(),
        route_id: anchor.route_id.clone(),
        direction: anchor.direction.clone(),
        first_edge_id: mandatory_lap.first_edge_id,
        last_edge_id: mandatory_lap.last_edge_id,
        lap_count: 1,
        source_segment_id: segment.segment_id.clone(),
        edge_ids: edge_ids.clone(),
        edge_ids_sha256,
    })
}

pub fn generate_directed_mandatory_lap(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    anchor: &DirectedJunctionAnchor,
) -> Result<RoutePlanLapV1, RouteMembershipError> {
    generate_route_plan_lap_v1(graph, route_memberships, anchor)
}

fn source_sequences_match(
    source_segments: &[&RouteMembershipSegment],
    source_index: usize,
    edge_ids: &[String],
    edge_cursor: usize,
) -> bool {
    if source_index == source_segments.len() {
        return edge_cursor == edge_ids.len();
    }
    let source = &source_segments[source_index].ordered_edge_ids;
    if source.is_empty() {
        return false;
    }
    for start in 0..source.len() {
        for length in 1..=source.len() {
            let end = edge_cursor.saturating_add(length);
            if end > edge_ids.len() {
                continue;
            }
            if (0..length).all(|offset| {
                source[(start + offset) % source.len()] == edge_ids[edge_cursor + offset]
            }) && source_sequences_match(source_segments, source_index + 1, edge_ids, end)
            {
                return true;
            }
        }
    }
    false
}

fn source_segment_by_id<'a>(
    membership: &'a RouteMembershipIndex,
    segment_id: &str,
) -> Result<&'a RouteMembershipSegment, RouteMembershipError> {
    membership
        .segments
        .iter()
        .find(|segment| segment.segment_id == segment_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "route plan references unknown source segment {}",
                segment_id
            ))
        })
}

pub fn validate_resolved_route_plan_segments(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    resolved_segments: &[ResolvedRouteSegment],
) -> Result<(), RouteMembershipError> {
    let mut resolved_ids = HashSet::new();
    for resolved in resolved_segments {
        if resolved.resolved_segment_id.is_empty()
            || !resolved_ids.insert(resolved.resolved_segment_id.as_str())
        {
            return Err(RouteMembershipError::Validation(
                "resolved route segment IDs must be non-empty and unique".into(),
            ));
        }
        if resolved.source_segment_ids.is_empty() || resolved.edge_ids.is_empty() {
            return Err(RouteMembershipError::Segment(
                "resolved route segment requires source segment IDs and edge IDs".into(),
            ));
        }
        let expected_hash = ordered_edge_ids_sha256(&resolved.edge_ids)
            .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
        if resolved.edge_ids_sha256 != expected_hash {
            return Err(RouteMembershipError::Segment(format!(
                "{} edgeIdsSha256 does not match its edge list",
                resolved.resolved_segment_id
            )));
        }
        let edges =
            validate_ordered_edges(graph, &resolved.edge_ids, &resolved.resolved_segment_id)?;
        if edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<HashSet<_>>()
            .len()
            != resolved.edge_ids.len()
        {
            return Err(RouteMembershipError::Segment(format!(
                "{} repeats an edge inside one resolved segment",
                resolved.resolved_segment_id
            )));
        }
        let membership = route_memberships
            .iter()
            .find(|membership| membership.membership_id == resolved.membership_id)
            .ok_or_else(|| {
                RouteMembershipError::Validation(format!(
                    "resolved route segment {} references unknown membership {}",
                    resolved.resolved_segment_id, resolved.membership_id
                ))
            })?;
        let source_segments = resolved
            .source_segment_ids
            .iter()
            .map(|segment_id| source_segment_by_id(membership, segment_id))
            .collect::<Result<Vec<_>, _>>()?;
        for source in &source_segments {
            validate_segment_hash(source)?;
        }
        if !source_sequences_match(&source_segments, 0, &resolved.edge_ids, 0) {
            return Err(RouteMembershipError::Segment(format!(
                "{} is not an ordered subpath of its source segments",
                resolved.resolved_segment_id
            )));
        }
        if resolved.role == RoutePlanSegmentRole::MandatoryLap
            && (source_segments.len() != 1
                || source_segments[0].source_kind != RouteMembershipSourceKind::RelationMainline)
        {
            return Err(RouteMembershipError::Validation(
                "mandatory lap must resolve from exactly one relationMainline segment".into(),
            ));
        }
    }
    Ok(())
}

pub fn validate_resolved_route_plan(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    resolved_segments: &[ResolvedRouteSegment],
) -> Result<(), RouteMembershipError> {
    validate_resolved_route_plan_segments(graph, route_memberships, resolved_segments)?;
    let expected_roles = [
        RoutePlanSegmentRole::EntryApproach,
        RoutePlanSegmentRole::MandatoryLap,
        RoutePlanSegmentRole::ReturnCorridor,
        RoutePlanSegmentRole::ExitApproach,
    ];
    if resolved_segments.len() != expected_roles.len()
        || resolved_segments
            .iter()
            .zip(expected_roles)
            .any(|(resolved, expected)| resolved.role != expected)
    {
        return Err(RouteMembershipError::Validation(
            "resolved route plan must contain entry, lap, return, and exit legs in order".into(),
        ));
    }
    for pair in resolved_segments.windows(2) {
        let left = validate_ordered_edges(graph, &pair[0].edge_ids, &pair[0].resolved_segment_id)?;
        let right = validate_ordered_edges(graph, &pair[1].edge_ids, &pair[1].resolved_segment_id)?;
        if left.last().map(|edge| edge.to.as_str()) != right.first().map(|edge| edge.from.as_str())
        {
            return Err(RouteMembershipError::Segment(
                "resolved route plan legs are not continuous at a segment boundary".into(),
            ));
        }
    }
    Ok(())
}

pub fn validate_route_plan_segments(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    resolved_segments: &[ResolvedRouteSegment],
) -> Result<(), RouteMembershipError> {
    validate_resolved_route_plan_segments(graph, route_memberships, resolved_segments)
}

fn bound_ramp_segment_for_exit<'a>(
    graph: &'a Graph,
    route_memberships: &'a [RouteMembershipIndex],
    membership: &RouteMembershipIndex,
    expected_ramp_id: &str,
    exit_edge_id: &str,
) -> Result<(&'a RouteMembershipSegment, &'a Ramp), RouteMembershipError> {
    let ramps = graph
        .ramps
        .iter()
        .filter(|ramp| {
            ramp.id == expected_ramp_id
                && ramp.kind == RampKind::GeneralExit
                && ramp.route == membership.route_id
                && ramp.direction == membership.direction
        })
        .collect::<Vec<_>>();
    if ramps.len() != 1 {
        return Err(RouteMembershipError::RampBinding(format!(
            "expected exit ramp {} must resolve to exactly one route/direction binding",
            expected_ramp_id
        )));
    }
    let ramp = ramps[0];
    if ramp.edge_id != exit_edge_id {
        return Err(RouteMembershipError::RampBinding(format!(
            "first exit edge {} does not equal exact ramp edge {}",
            exit_edge_id, ramp.edge_id
        )));
    }
    let segments = route_memberships
        .iter()
        .filter(|candidate| {
            candidate.route_id == membership.route_id && candidate.direction == membership.direction
        })
        .flat_map(|candidate| candidate.segments.iter())
        .filter(|segment| {
            segment.source_kind == RouteMembershipSourceKind::BoundRamp
                && segment.ordered_edge_ids.first() == Some(&ramp.edge_id)
        })
        .collect::<Vec<_>>();
    if segments.len() != 1 {
        return Err(RouteMembershipError::RampBinding(format!(
            "ramp {} must have exactly one ordered boundRamp segment (matches={})",
            expected_ramp_id,
            segments.len()
        )));
    }
    Ok((segments[0], ramp))
}

fn relation_sequences(membership: &RouteMembershipIndex) -> Vec<&RouteMembershipSegment> {
    membership
        .segments
        .iter()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
        .collect()
}

fn corridor_membership<'a>(
    route_memberships: &'a [RouteMembershipIndex],
    membership_id: &str,
) -> Result<&'a RouteMembershipIndex, RouteMembershipError> {
    route_memberships
        .iter()
        .find(|membership| membership.membership_id == membership_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!("unknown route membership {}", membership_id))
        })
}

fn route_membership_source_snapshot_sha256(
    route_memberships: &[RouteMembershipIndex],
) -> Result<&str, RouteMembershipError> {
    let source_snapshot_sha256 = route_memberships
        .first()
        .and_then(|membership| membership.segments.first())
        .map(|segment| segment.source_snapshot_sha256.as_str())
        .ok_or_else(|| {
            RouteMembershipError::Validation(
                "route memberships must contain at least one segment".into(),
            )
        })?;
    if route_memberships.iter().any(|membership| {
        membership
            .segments
            .iter()
            .any(|segment| segment.source_snapshot_sha256 != source_snapshot_sha256)
    }) {
        return Err(RouteMembershipError::Validation(
            "route memberships reference different source snapshots".into(),
        ));
    }
    Ok(source_snapshot_sha256)
}

fn corridor_sequence_for_initial_edge(
    membership: &RouteMembershipIndex,
    initial_edge_id: &str,
) -> Result<Vec<String>, RouteMembershipError> {
    let sequences = relation_sequences(membership);
    let matching: Vec<&&RouteMembershipSegment> = sequences
        .iter()
        .filter(|segment| {
            segment
                .ordered_edge_ids
                .iter()
                .any(|edge_id| edge_id == initial_edge_id)
        })
        .collect();
    if matching.len() != 1 {
        return Err(RouteMembershipError::Validation(format!(
            "initial edge {} must occur in exactly one relationMainline segment of {}",
            initial_edge_id, membership.membership_id
        )));
    }
    let sequence = &matching[0].ordered_edge_ids;
    let start = sequence
        .iter()
        .position(|edge_id| edge_id == initial_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(
                "initial edge is absent from its relation segment".into(),
            )
        })?;
    Ok(sequence[start..].to_vec())
}

fn sequence_source_segment_ids(
    membership: &RouteMembershipIndex,
    sequence: &[String],
) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for edge_id in sequence {
        let source = relation_sequences(membership)
            .into_iter()
            .find(|segment| segment.ordered_edge_ids.contains(edge_id))
            .map(|segment| segment.segment_id.clone());
        if let Some(source) = source {
            if result.last() != Some(&source) {
                result.push(source);
            }
        }
    }
    result
}

fn corridor_sequence_from_source_segments(
    membership: &RouteMembershipIndex,
    source_segment_ids: &[String],
    initial_edge_id: &str,
) -> Result<Vec<String>, RouteMembershipError> {
    if source_segment_ids.is_empty() {
        return Err(RouteMembershipError::InvalidInput(
            "corridor source segment IDs must not be empty".into(),
        ));
    }
    let mut result = Vec::new();
    for (source_index, source_segment_id) in source_segment_ids.iter().enumerate() {
        let segment = source_segment_by_id(membership, source_segment_id)?;
        if segment.source_kind != RouteMembershipSourceKind::RelationMainline {
            return Err(RouteMembershipError::Validation(format!(
                "corridor source segment {} is not relationMainline",
                source_segment_id
            )));
        }
        if source_index == 0 {
            let start = segment
                .ordered_edge_ids
                .iter()
                .position(|edge_id| edge_id == initial_edge_id)
                .ok_or_else(|| {
                    RouteMembershipError::Validation(format!(
                        "initial edge {} is absent from first corridor source segment",
                        initial_edge_id
                    ))
                })?;
            result.extend_from_slice(&segment.ordered_edge_ids[start..]);
        } else {
            result.extend(segment.ordered_edge_ids.iter().cloned());
        }
    }
    if result.is_empty() {
        return Err(RouteMembershipError::Segment(
            "corridor source segments produce an empty edge sequence".into(),
        ));
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RelationPathEdge {
    edge_id: String,
    source_segment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelationPathSearchState {
    distance_meters: u64,
    path: Vec<RelationPathEdge>,
}

impl Ord for RelationPathSearchState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .distance_meters
            .cmp(&self.distance_meters)
            .then_with(|| other.path.iter().cmp(&self.path))
    }
}

impl PartialOrd for RelationPathSearchState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn relation_path_adjacency(
    graph: &Graph,
    membership: &RouteMembershipIndex,
) -> Result<BTreeMap<String, Vec<RelationPathEdge>>, RouteMembershipError> {
    let graph_edges = edge_map(graph);
    let mut adjacency: BTreeMap<String, Vec<RelationPathEdge>> = BTreeMap::new();
    for segment in relation_sequences(membership) {
        for edge_id in &segment.ordered_edge_ids {
            let edge = graph_edges.get(edge_id.as_str()).ok_or_else(|| {
                RouteMembershipError::Segment(format!(
                    "{} references missing graph edge {}",
                    segment.segment_id, edge_id
                ))
            })?;
            if edge.kind != EdgeKind::Shutoko {
                return Err(RouteMembershipError::Segment(format!(
                    "{} contains non-mainline edge {}",
                    segment.segment_id, edge.id
                )));
            }
            adjacency
                .entry(edge.from.clone())
                .or_default()
                .push(RelationPathEdge {
                    edge_id: edge.id.clone(),
                    source_segment_id: segment.segment_id.clone(),
                });
        }
    }
    for edges in adjacency.values_mut() {
        edges.sort_by(|left, right| {
            left.edge_id
                .cmp(&right.edge_id)
                .then_with(|| left.source_segment_id.cmp(&right.source_segment_id))
        });
    }
    Ok(adjacency)
}

fn find_relation_path_to_node(
    graph: &Graph,
    membership: &RouteMembershipIndex,
    start_node_id: &str,
    initial_edge_id: Option<&str>,
    target_node_id: &str,
    state_budget: usize,
) -> Result<Vec<RelationPathEdge>, RouteMembershipError> {
    if state_budget == 0 {
        return Err(RouteMembershipError::InvalidInput(
            "corridor state budget must be positive".into(),
        ));
    }
    let adjacency = relation_path_adjacency(graph, membership)?;
    if let Some(initial_edge_id) = initial_edge_id {
        let initial_edge = graph
            .edges
            .iter()
            .find(|edge| edge.id == initial_edge_id)
            .ok_or_else(|| {
                RouteMembershipError::Validation(format!(
                    "initial edge {} is absent from graph",
                    initial_edge_id
                ))
            })?;
        if initial_edge.kind != EdgeKind::Shutoko || initial_edge.from != start_node_id {
            return Err(RouteMembershipError::Validation(format!(
                "initial edge {} is not a mainline edge leaving {}",
                initial_edge.id, start_node_id
            )));
        }
        if !adjacency
            .get(initial_edge.from.as_str())
            .is_some_and(|edges| edges.iter().any(|edge| edge.edge_id == initial_edge_id))
        {
            return Err(RouteMembershipError::Validation(format!(
                "initial edge {} is absent from relationMainline membership {}",
                initial_edge_id, membership.membership_id
            )));
        }
    }
    let mut heap = BinaryHeap::new();
    heap.push(RelationPathSearchState {
        distance_meters: 0,
        path: Vec::new(),
    });
    let mut states = 0_usize;
    while let Some(state) = heap.pop() {
        states += 1;
        if states > state_budget {
            return Err(RouteMembershipError::BudgetExceeded(format!(
                "corridor search exceeded {} states before reaching declared Exit evidence",
                state_budget
            )));
        }
        let current_node_id = if let Some(last) = state.path.last() {
            graph
                .edges
                .iter()
                .find(|edge| edge.id == last.edge_id)
                .map(|edge| edge.to.as_str())
                .ok_or_else(|| {
                    RouteMembershipError::Validation(format!(
                        "corridor edge {} is absent from graph",
                        last.edge_id
                    ))
                })?
        } else {
            start_node_id
        };
        if current_node_id == target_node_id {
            return Ok(state.path);
        }
        let outgoing = adjacency
            .get(current_node_id)
            .into_iter()
            .flat_map(|edges| edges.iter())
            .filter(|candidate| {
                initial_edge_id.is_none_or(|initial_edge_id| {
                    !state.path.is_empty() || candidate.edge_id == initial_edge_id
                })
            })
            .filter(|candidate| {
                !state
                    .path
                    .iter()
                    .any(|edge| edge.edge_id == candidate.edge_id)
            });
        for candidate in outgoing {
            let edge = graph
                .edges
                .iter()
                .find(|edge| edge.id == candidate.edge_id)
                .ok_or_else(|| {
                    RouteMembershipError::Validation(format!(
                        "corridor edge {} is absent from graph",
                        candidate.edge_id
                    ))
                })?;
            let mut path = state.path.clone();
            path.push(candidate.clone());
            let edge_ids = path
                .iter()
                .map(|edge| edge.edge_id.clone())
                .collect::<Vec<_>>();
            if contains_forbidden_transition(&edge_ids, &graph.forbidden_transitions) {
                return Err(RouteMembershipError::Validation(
                    "return corridor contains a forbidden transition".into(),
                ));
            }
            let next_distance = state.distance_meters.saturating_add(edge.distance_meters);
            heap.push(RelationPathSearchState {
                distance_meters: next_distance,
                path,
            });
        }
    }
    Err(RouteMembershipError::ExitNotFound(format!(
        "relationMainline does not reach target node {} from {}",
        target_node_id, start_node_id
    )))
}

#[allow(clippy::too_many_arguments)]
fn resolve_first_exit_to_declared_candidates(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership: &RouteMembershipIndex,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
    exact_directed_binding: EndpointSupportState,
    declared_candidates: &[DirectedEndpointSegment],
    state_budget: usize,
) -> Result<CorridorFirstExitResolution, RouteMembershipError> {
    if declared_candidates.is_empty() {
        return Err(RouteMembershipError::RampBinding(
            "declared Exit evidence is required to resolve a radial route plan".into(),
        ));
    }
    for candidate in declared_candidates {
        validate_declared_endpoint_candidate(
            graph,
            candidate,
            exact_directed_binding == EndpointSupportState::VerifiedBound,
        )?;
    }
    let mut paths = Vec::new();
    for target_node_id in declared_candidates
        .iter()
        .map(|candidate| candidate.from_node_id.as_str())
        .collect::<BTreeSet<_>>()
    {
        let path = find_relation_path_to_node(
            graph,
            membership,
            start_node_id,
            Some(initial_edge_id),
            target_node_id,
            state_budget,
        )?;
        let distance_meters = path
            .iter()
            .map(|edge| {
                graph
                    .edges
                    .iter()
                    .find(|candidate| candidate.id == edge.edge_id)
                    .map(|candidate| candidate.distance_meters)
                    .unwrap_or(0)
            })
            .sum::<u64>();
        paths.push((distance_meters, path));
    }
    paths.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.iter().cmp(&right.1))
    });
    let (distance_meters, path) = paths
        .into_iter()
        .next()
        .ok_or_else(|| RouteMembershipError::ExitNotFound("no declared Exit path".into()))?;
    let sequence = path
        .iter()
        .map(|edge| edge.edge_id.clone())
        .collect::<Vec<_>>();
    let mut source_segment_ids = Vec::new();
    for edge in &path {
        if source_segment_ids.last() != Some(&edge.source_segment_id) {
            source_segment_ids.push(edge.source_segment_id.clone());
        }
    }
    let mut resolution = resolve_first_exit_on_sequence_with_budget(
        graph,
        route_memberships,
        membership,
        &sequence,
        start_node_id,
        expected_ramp_id,
        exact_directed_binding,
        declared_candidates,
        state_budget,
    )?;
    resolution.mainline_source_segment_ids = source_segment_ids;
    if let Some(exit) = &mut resolution.exit {
        exit.mainline_source_segment_ids = resolution.mainline_source_segment_ids.clone();
    }
    if resolution.distance_meters == 0 {
        resolution.distance_meters = distance_meters;
    }
    Ok(resolution)
}

fn exit_ramp_candidates<'a>(
    graph: &'a Graph,
    membership: &RouteMembershipIndex,
    split_node_id: &str,
) -> Vec<(&'a Edge, &'a Ramp)> {
    let mut candidates = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::Exit && edge.from == split_node_id)
        .flat_map(|edge| {
            graph.ramps.iter().filter_map(move |ramp| {
                (ramp.kind == RampKind::GeneralExit
                    && ramp.edge_id == edge.id
                    && ramp.mainline_node_id == edge.from
                    && ramp.node_id == edge.to
                    && ramp.route == membership.route_id
                    && ramp.direction == membership.direction)
                    .then_some((edge, ramp))
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.0
            .id
            .cmp(&right.0.id)
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    candidates
}

fn support_state_label(state: EndpointSupportState) -> &'static str {
    match state {
        EndpointSupportState::VerifiedBound => "verified_bound",
        EndpointSupportState::Unresolved => "unresolved",
        EndpointSupportState::Unsupported => "unsupported",
    }
}

fn validate_declared_endpoint_candidate(
    graph: &Graph,
    candidate: &DirectedEndpointSegment,
    require_exit_edges: bool,
) -> Result<(), RouteMembershipError> {
    if candidate.segment_id.is_empty()
        || candidate.from_node_id.is_empty()
        || candidate.to_node_id.is_empty()
        || candidate.osm_way_ids.is_empty()
        || candidate.osm_node_ids.len() != candidate.edge_ids.len() + 1
        || candidate.osm_node_ids.iter().any(|node_id| *node_id <= 0)
        || candidate.edge_ids.is_empty()
    {
        return Err(RouteMembershipError::RampBinding(
            "declared endpoint candidate has incomplete evidence".into(),
        ));
    }
    let mut osm_way_ids = HashSet::new();
    if candidate
        .osm_way_ids
        .iter()
        .any(|way_id| *way_id <= 0 || !osm_way_ids.insert(way_id))
    {
        return Err(RouteMembershipError::RampBinding(format!(
            "{} has invalid or duplicate OSM way IDs",
            candidate.segment_id
        )));
    }
    let edges = validate_ordered_edges(graph, &candidate.edge_ids, &candidate.segment_id)?;
    let graph_osm_node_ids = edges
        .first()
        .map(|edge| edge.from.as_str())
        .into_iter()
        .chain(edges.iter().map(|edge| edge.to.as_str()))
        .map(graph_node_osm_id)
        .collect::<Option<Vec<_>>>();
    if edges.first().map(|edge| edge.from.as_str()) != Some(candidate.from_node_id.as_str())
        || edges.last().map(|edge| edge.to.as_str()) != Some(candidate.to_node_id.as_str())
        || candidate.osm_node_ids.first().copied() != graph_node_osm_id(&candidate.from_node_id)
        || candidate.osm_node_ids.last().copied() != graph_node_osm_id(&candidate.to_node_id)
        || graph_osm_node_ids
            .as_ref()
            .is_some_and(|node_ids| node_ids != &candidate.osm_node_ids)
    {
        return Err(RouteMembershipError::RampBinding(format!(
            "{} endpoints do not match its declared endpoint candidate edges",
            candidate.segment_id
        )));
    }
    if require_exit_edges && edges.iter().any(|edge| edge.kind != EdgeKind::Exit) {
        return Err(RouteMembershipError::RampBinding(format!(
            "{} contains an edge that is not an Exit ramp edge",
            candidate.segment_id
        )));
    }
    validate_way_order(&candidate.osm_way_ids, &edges, &candidate.segment_id)?;
    let expected_hash = ordered_edge_ids_sha256(&candidate.edge_ids)
        .map_err(|error| RouteMembershipError::RampBinding(error.to_string()))?;
    if candidate.edge_ids_sha256 != expected_hash {
        return Err(RouteMembershipError::RampBinding(format!(
            "{} edgeIdsSha256 does not match its declared endpoint candidate",
            candidate.segment_id
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn resolve_first_exit_on_sequence_with_budget(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership: &RouteMembershipIndex,
    sequence: &[String],
    start_node_id: &str,
    expected_ramp_id: &str,
    exact_directed_binding: EndpointSupportState,
    declared_candidates: &[DirectedEndpointSegment],
    state_budget: usize,
) -> Result<CorridorFirstExitResolution, RouteMembershipError> {
    if state_budget == 0 {
        return Err(RouteMembershipError::InvalidInput(
            "corridor state budget must be positive".into(),
        ));
    }
    if expected_ramp_id.is_empty() {
        return Err(RouteMembershipError::InvalidInput(
            "expected ramp id must not be empty".into(),
        ));
    }
    if sequence.is_empty() {
        return Err(RouteMembershipError::Segment(
            "corridor edge sequence is empty".into(),
        ));
    }
    let mut declared_candidate_ids = HashSet::new();
    for candidate in declared_candidates {
        validate_declared_endpoint_candidate(
            graph,
            candidate,
            exact_directed_binding == EndpointSupportState::VerifiedBound,
        )?;
        if !declared_candidate_ids.insert(candidate.segment_id.as_str()) {
            return Err(RouteMembershipError::RampBinding(format!(
                "duplicate declared endpoint candidate {}",
                candidate.segment_id
            )));
        }
    }
    let first_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == sequence[0])
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "initial edge {} is absent from graph",
                sequence[0]
            ))
        })?;
    if first_edge.kind != EdgeKind::Shutoko || first_edge.from != start_node_id {
        return Err(RouteMembershipError::Validation(format!(
            "initial edge {} is not a mainline edge leaving {}",
            first_edge.id, start_node_id
        )));
    }
    validate_ordered_edge_sequence_allowing_repeats(graph, sequence, "return corridor")?;
    let mainline_source_segment_ids = sequence_source_segment_ids(membership, sequence);
    let mut path = Vec::new();
    let mut mainline = Vec::new();
    let mut distance = 0_u64;
    for (state, edge_id) in sequence.iter().enumerate() {
        if state >= state_budget {
            return Err(RouteMembershipError::BudgetExceeded(format!(
                "corridor search exceeded {} states before finding a general Exit",
                state_budget
            )));
        }
        let edge = graph
            .edges
            .iter()
            .find(|edge| edge.id == *edge_id)
            .ok_or_else(|| {
                RouteMembershipError::Validation(format!(
                    "corridor edge {} is absent from graph",
                    edge_id
                ))
            })?;
        if edge.kind != EdgeKind::Shutoko {
            return Err(RouteMembershipError::Validation(format!(
                "corridor edge {} is not Shutoko mainline",
                edge.id
            )));
        }
        path.push(edge.id.clone());
        mainline.push(edge.id.clone());
        distance = distance.saturating_add(edge.distance_meters);
        if contains_forbidden_transition(&path, &graph.forbidden_transitions) {
            return Err(RouteMembershipError::Validation(
                "return corridor contains a forbidden transition".into(),
            ));
        }

        let candidates = exit_ramp_candidates(graph, membership, &edge.to);
        let declared_hits: Vec<&DirectedEndpointSegment> = declared_candidates
            .iter()
            .filter(|candidate| candidate.from_node_id == edge.to)
            .collect();
        if declared_hits.len() > 1 {
            return Err(RouteMembershipError::RampBinding(format!(
                "return corridor has {} declared endpoint candidates at {}",
                declared_hits.len(),
                edge.to
            )));
        }
        if candidates.is_empty() {
            if let Some(candidate) = declared_hits.first() {
                if exact_directed_binding == EndpointSupportState::VerifiedBound {
                    return Err(RouteMembershipError::RampBinding(format!(
                        "declared endpoint candidate {} is not a verified binding",
                        candidate.segment_id
                    )));
                }
                return Ok(CorridorFirstExitResolution {
                    exact_directed_binding,
                    exit: None,
                    blocked_exit_edge_id: candidate.edge_ids.first().cloned(),
                    blocked_ramp_id: Some(expected_ramp_id.to_string()),
                    mainline_edge_ids: mainline,
                    mainline_source_segment_ids,
                    distance_meters: distance,
                });
            }
            continue;
        }
        if candidates.len() != 1 {
            return Err(RouteMembershipError::RampBinding(format!(
                "return corridor has {} same-route general Exit bindings at {}",
                candidates.len(),
                edge.to
            )));
        }
        let (exit_edge, ramp) = candidates[0];
        if ramp.id != expected_ramp_id {
            if exact_directed_binding == EndpointSupportState::VerifiedBound {
                return Err(RouteMembershipError::RampBinding(format!(
                    "first general Exit is {} at {}, expected {}",
                    ramp.id, exit_edge.id, expected_ramp_id
                )));
            }
            return Ok(CorridorFirstExitResolution {
                exact_directed_binding,
                exit: None,
                blocked_exit_edge_id: Some(exit_edge.id.clone()),
                blocked_ramp_id: Some(ramp.id.clone()),
                mainline_edge_ids: mainline,
                mainline_source_segment_ids,
                distance_meters: distance,
            });
        }
        if exact_directed_binding != EndpointSupportState::VerifiedBound {
            return Ok(CorridorFirstExitResolution {
                exact_directed_binding,
                exit: None,
                blocked_exit_edge_id: Some(exit_edge.id.clone()),
                blocked_ramp_id: Some(ramp.id.clone()),
                mainline_edge_ids: mainline,
                mainline_source_segment_ids,
                distance_meters: distance,
            });
        }
        let (bound_segment, exact_ramp) = bound_ramp_segment_for_exit(
            graph,
            route_memberships,
            membership,
            expected_ramp_id,
            &exit_edge.id,
        )?;
        if !declared_candidates.is_empty() {
            let declared_candidate = declared_hits.first().ok_or_else(|| {
                RouteMembershipError::RampBinding(format!(
                    "verified ramp {} has no declared directed Exit candidate",
                    expected_ramp_id
                ))
            })?;
            if declared_candidate.edge_ids != bound_segment.ordered_edge_ids {
                return Err(RouteMembershipError::RampBinding(format!(
                    "declared endpoint candidate {} does not match the exact boundRamp segment {}",
                    declared_candidate.segment_id, bound_segment.segment_id
                )));
            }
        }
        let ramp_edges = validate_ordered_edges(
            graph,
            &bound_segment.ordered_edge_ids,
            &bound_segment.segment_id,
        )?;
        let mut full_path = path.clone();
        full_path.extend(bound_segment.ordered_edge_ids.iter().cloned());
        if contains_forbidden_transition(&full_path, &graph.forbidden_transitions) {
            return Err(RouteMembershipError::Validation(
                "first corridor Exit contains a forbidden transition".into(),
            ));
        }
        distance = distance.saturating_add(
            ramp_edges
                .iter()
                .map(|edge| edge.distance_meters)
                .sum::<u64>(),
        );
        return Ok(CorridorFirstExitResolution {
            exact_directed_binding,
            exit: Some(CorridorExit {
                distance_meters: distance,
                exit_edge_id: exact_ramp.edge_id.clone(),
                ramp_id: exact_ramp.id.clone(),
                edge_ids: full_path,
                mainline_edge_ids: mainline.clone(),
                mainline_source_segment_ids: mainline_source_segment_ids.clone(),
            }),
            blocked_exit_edge_id: None,
            blocked_ramp_id: None,
            mainline_edge_ids: mainline,
            mainline_source_segment_ids,
            distance_meters: distance,
        });
    }
    Err(RouteMembershipError::ExitNotFound(format!(
        "corridor ended without reaching declared Exit evidence for {} (exactDirectedBinding={})",
        expected_ramp_id,
        support_state_label(exact_directed_binding)
    )))
}

#[allow(clippy::too_many_arguments)]
pub fn find_first_exit_on_corridor_with_binding_budget(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
    exact_directed_binding: EndpointSupportState,
    state_budget: usize,
) -> Result<CorridorFirstExitResolution, RouteMembershipError> {
    let membership = corridor_membership(route_memberships, membership_id)?;
    let source_snapshot_sha256 = membership
        .segments
        .first()
        .map(|segment| segment.source_snapshot_sha256.as_str())
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "route membership {} has no segments",
                membership_id
            ))
        })?;
    validate_route_membership_structure(graph, route_memberships, source_snapshot_sha256)?;
    let sequence = corridor_sequence_for_initial_edge(membership, initial_edge_id)?;
    resolve_first_exit_on_sequence_with_budget(
        graph,
        route_memberships,
        membership,
        &sequence,
        start_node_id,
        expected_ramp_id,
        exact_directed_binding,
        &[],
        state_budget,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn find_first_exit_on_corridor_on_segments_with_binding_budget(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
    exact_directed_binding: EndpointSupportState,
    source_segment_ids: &[String],
    state_budget: usize,
) -> Result<CorridorFirstExitResolution, RouteMembershipError> {
    let membership = corridor_membership(route_memberships, membership_id)?;
    let source_snapshot_sha256 = membership
        .segments
        .first()
        .map(|segment| segment.source_snapshot_sha256.as_str())
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "route membership {} has no segments",
                membership_id
            ))
        })?;
    validate_route_membership_structure(graph, route_memberships, source_snapshot_sha256)?;
    let sequence =
        corridor_sequence_from_source_segments(membership, source_segment_ids, initial_edge_id)?;
    resolve_first_exit_on_sequence_with_budget(
        graph,
        route_memberships,
        membership,
        &sequence,
        start_node_id,
        expected_ramp_id,
        exact_directed_binding,
        &[],
        state_budget,
    )
}

pub fn find_first_exit_on_corridor_with_budget(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
    state_budget: usize,
) -> Result<CorridorExit, RouteMembershipError> {
    let resolution = find_first_exit_on_corridor_with_binding_budget(
        graph,
        route_memberships,
        membership_id,
        start_node_id,
        initial_edge_id,
        expected_ramp_id,
        EndpointSupportState::VerifiedBound,
        state_budget,
    )?;
    resolution.exit.ok_or_else(|| {
        RouteMembershipError::RampBinding(format!(
            "first general Exit {} has exactDirectedBinding={}",
            expected_ramp_id,
            support_state_label(resolution.exact_directed_binding)
        ))
    })
}

pub fn find_first_exit_on_corridor_with_binding(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
    exact_directed_binding: EndpointSupportState,
) -> Result<CorridorFirstExitResolution, RouteMembershipError> {
    find_first_exit_on_corridor_with_binding_budget(
        graph,
        route_memberships,
        membership_id,
        start_node_id,
        initial_edge_id,
        expected_ramp_id,
        exact_directed_binding,
        CORRIDOR_EXIT_STATE_BUDGET,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn find_first_exit_on_corridor_on_segments_with_binding(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
    exact_directed_binding: EndpointSupportState,
    source_segment_ids: &[String],
) -> Result<CorridorFirstExitResolution, RouteMembershipError> {
    find_first_exit_on_corridor_on_segments_with_binding_budget(
        graph,
        route_memberships,
        membership_id,
        start_node_id,
        initial_edge_id,
        expected_ramp_id,
        exact_directed_binding,
        source_segment_ids,
        CORRIDOR_EXIT_STATE_BUDGET,
    )
}

pub fn find_first_exit_on_corridor(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
) -> Result<CorridorExit, RouteMembershipError> {
    find_first_exit_on_corridor_with_budget(
        graph,
        route_memberships,
        membership_id,
        start_node_id,
        initial_edge_id,
        expected_ramp_id,
        CORRIDOR_EXIT_STATE_BUDGET,
    )
}

fn resolve_directed_route_plan_with_candidates(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    route_plan: &DiagnosticRoutePlan,
    declared_candidates: &[DirectedEndpointSegment],
) -> Result<DirectedRoutePlanResolution, RouteMembershipError> {
    let source_snapshot_sha256 = route_membership_source_snapshot_sha256(route_memberships)?;
    validate_route_membership_structure(graph, route_memberships, source_snapshot_sha256)?;
    let lap = generate_route_plan_lap_v1(graph, route_memberships, &route_plan.anchor)?;
    if route_plan.mandatory_lap.membership_id
        != route_memberships
            .iter()
            .find(|membership| {
                membership.route_id == route_plan.anchor.route_id
                    && membership.direction == route_plan.anchor.direction
            })
            .map(|membership| membership.membership_id.as_str())
            .unwrap_or_default()
        || route_plan.mandatory_lap.first_edge_id != lap.first_edge_id
        || route_plan.mandatory_lap.last_edge_id != lap.last_edge_id
        || route_plan.mandatory_lap.lap_count != 1
    {
        return Err(RouteMembershipError::Validation(
            "declared mandatory lap does not match the generated routePlanLapV1".into(),
        ));
    }
    if route_plan.entry_corridor.merge_node_id != route_plan.anchor.merge_node_id
        || route_plan.entry_corridor.terminal_edge_id != route_plan.anchor.merge_terminal_edge_id
        || route_plan.return_corridor.start_node_id != route_plan.anchor.branch_node_id
        || route_plan.return_corridor.initial_edge_id != route_plan.anchor.branch_initial_edge_id
    {
        return Err(RouteMembershipError::Validation(
            "route plan corridors do not meet the directed junction M/B boundaries".into(),
        ));
    }
    let entry_membership =
        corridor_membership(route_memberships, &route_plan.entry_corridor.membership_id)?;
    if !entry_membership.segments.iter().any(|segment| {
        segment
            .ordered_edge_ids
            .iter()
            .any(|edge_id| edge_id == &route_plan.entry_corridor.terminal_edge_id)
    }) {
        return Err(RouteMembershipError::Validation(
            "entry corridor terminal edge is not present in its membership".into(),
        ));
    }
    let return_membership =
        corridor_membership(route_memberships, &route_plan.return_corridor.membership_id)?;
    let initial_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == route_plan.return_corridor.initial_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "return initial edge {} is absent from graph",
                route_plan.return_corridor.initial_edge_id
            ))
        })?;
    if initial_edge.kind != EdgeKind::Shutoko
        || initial_edge.from != route_plan.anchor.branch_node_id
    {
        return Err(RouteMembershipError::Validation(
            "return corridor initial edge does not leave branch node B".into(),
        ));
    }
    let expected_ramp_id = &route_plan
        .return_corridor
        .first_general_exit
        .expected_ramp_id;
    let exact_directed_binding = route_plan
        .return_corridor
        .first_general_exit
        .exact_directed_binding;
    let first_exit = if declared_candidates.is_empty() {
        let return_sequence = corridor_sequence_for_initial_edge(
            return_membership,
            &route_plan.return_corridor.initial_edge_id,
        )?;
        if return_sequence.first() != Some(&route_plan.return_corridor.initial_edge_id) {
            return Err(RouteMembershipError::Validation(
                "return corridor initial edge is not the first resolved edge".into(),
            ));
        }
        resolve_first_exit_on_sequence_with_budget(
            graph,
            route_memberships,
            return_membership,
            &return_sequence,
            &route_plan.anchor.branch_node_id,
            expected_ramp_id,
            exact_directed_binding,
            declared_candidates,
            CORRIDOR_EXIT_STATE_BUDGET,
        )?
    } else {
        resolve_first_exit_to_declared_candidates(
            graph,
            route_memberships,
            return_membership,
            &route_plan.anchor.branch_node_id,
            &route_plan.return_corridor.initial_edge_id,
            expected_ramp_id,
            exact_directed_binding,
            declared_candidates,
            CORRIDOR_EXIT_STATE_BUDGET,
        )?
    };
    Ok(DirectedRoutePlanResolution { lap, first_exit })
}

pub fn resolve_directed_route_plan(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    route_plan: &DiagnosticRoutePlan,
) -> Result<DirectedRoutePlanResolution, RouteMembershipError> {
    resolve_directed_route_plan_with_candidates(graph, route_memberships, route_plan, &[])
}

pub fn resolve_diagnostic_radial_route_plan(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    seed: &RadialReturnBillingPairSeed,
) -> Result<DirectedRoutePlanResolution, RouteMembershipError> {
    if seed.entry_endpoint.support_state != EndpointSupportState::VerifiedBound {
        return Err(RouteMembershipError::RampBinding(
            "entry endpoint is not verified_bound".into(),
        ));
    }
    for segment in &seed.entry_endpoint.directed_segments {
        validate_declared_endpoint_candidate(graph, segment, false)?;
        let edges = validate_ordered_edges(graph, &segment.edge_ids, &segment.segment_id)?;
        if edges.iter().any(|edge| edge.kind != EdgeKind::Entry) {
            return Err(RouteMembershipError::RampBinding(format!(
                "{} contains an edge that is not an Entry ramp edge",
                segment.segment_id
            )));
        }
    }
    let first_exit = &seed.route_plan.return_corridor.first_general_exit;
    if seed.exit_endpoint.ramp_id != first_exit.expected_ramp_id {
        return Err(RouteMembershipError::RampBinding(format!(
            "exit endpoint ramp {} does not match first-general-exit expectation {}",
            seed.exit_endpoint.ramp_id, first_exit.expected_ramp_id
        )));
    }
    if seed.exit_endpoint.support_state != first_exit.exact_directed_binding {
        return Err(RouteMembershipError::RampBinding(format!(
            "exit endpoint supportState {} does not match first-general-exit exactDirectedBinding {}",
            support_state_label(seed.exit_endpoint.support_state),
            support_state_label(first_exit.exact_directed_binding)
        )));
    }
    if seed.exit_endpoint.support_state == EndpointSupportState::VerifiedBound
        && seed.exit_endpoint.directed_segments.is_empty()
    {
        return Err(RouteMembershipError::RampBinding(
            "verified exit endpoint has no declared directed Exit candidate".into(),
        ));
    }
    let declared_candidates = declared_exit_candidates(&seed.exit_endpoint);
    resolve_directed_route_plan_with_candidates(
        graph,
        route_memberships,
        &seed.route_plan,
        &declared_candidates,
    )
}

fn declared_exit_candidates(endpoint: &DiagnosticEndpoint) -> Vec<DirectedEndpointSegment> {
    match endpoint.support_state {
        EndpointSupportState::VerifiedBound => endpoint.directed_segments.clone(),
        EndpointSupportState::Unresolved | EndpointSupportState::Unsupported => endpoint
            .binding_candidates
            .iter()
            .flat_map(|candidate| candidate.directed_segments.iter().cloned())
            .collect(),
    }
}

fn core_endpoint_support_state(
    state: EndpointSupportState,
) -> shutoko_routing_core::EndpointSupportState {
    match state {
        EndpointSupportState::VerifiedBound => {
            shutoko_routing_core::EndpointSupportState::VerifiedBound
        }
        EndpointSupportState::Unresolved => shutoko_routing_core::EndpointSupportState::Unresolved,
        EndpointSupportState::Unsupported => {
            shutoko_routing_core::EndpointSupportState::Unsupported
        }
    }
}

fn core_binding_candidate_status(
    status: crate::seed::BindingCandidateStatus,
) -> shutoko_routing_core::EndpointSupportState {
    match status {
        crate::seed::BindingCandidateStatus::Unresolved => {
            shutoko_routing_core::EndpointSupportState::Unresolved
        }
        crate::seed::BindingCandidateStatus::Unsupported => {
            shutoko_routing_core::EndpointSupportState::Unsupported
        }
    }
}

fn core_billing_endpoint(endpoint: &DiagnosticEndpoint) -> shutoko_routing_core::BillingEndpoint {
    shutoko_routing_core::BillingEndpoint {
        ramp_id: endpoint.ramp_id.clone(),
        name: endpoint.name.clone(),
        support_state: core_endpoint_support_state(endpoint.support_state),
        directed_segments: endpoint
            .directed_segments
            .iter()
            .map(|segment| shutoko_routing_core::DirectedEndpointSegment {
                segment_id: segment.segment_id.clone(),
                osm_way_ids: segment.osm_way_ids.clone(),
                osm_node_ids: segment.osm_node_ids.clone(),
                edge_ids: segment.edge_ids.clone(),
                from_node_id: segment.from_node_id.clone(),
                to_node_id: segment.to_node_id.clone(),
                edge_ids_sha256: segment.edge_ids_sha256.clone(),
            })
            .collect(),
        binding_candidates: endpoint
            .binding_candidates
            .iter()
            .map(|candidate| shutoko_routing_core::BindingCandidate {
                candidate_id: candidate.candidate_id.clone(),
                status: core_binding_candidate_status(candidate.status),
                directed_segments: candidate
                    .directed_segments
                    .iter()
                    .map(|segment| shutoko_routing_core::DirectedEndpointSegment {
                        segment_id: segment.segment_id.clone(),
                        osm_way_ids: segment.osm_way_ids.clone(),
                        osm_node_ids: segment.osm_node_ids.clone(),
                        edge_ids: segment.edge_ids.clone(),
                        from_node_id: segment.from_node_id.clone(),
                        to_node_id: segment.to_node_id.clone(),
                        edge_ids_sha256: segment.edge_ids_sha256.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn endpoint_bound_source_segments(
    graph: &Graph,
    membership: &RouteMembershipIndex,
    endpoint: &DiagnosticEndpoint,
    expected_kind: EdgeKind,
) -> Result<(Vec<String>, Vec<String>), RouteMembershipError> {
    let mut source_segment_ids = Vec::new();
    let mut edge_ids = Vec::new();
    for endpoint_segment in &endpoint.directed_segments {
        validate_declared_endpoint_candidate(
            graph,
            endpoint_segment,
            expected_kind == EdgeKind::Exit,
        )?;
        let edges = validate_ordered_edges(
            graph,
            &endpoint_segment.edge_ids,
            &endpoint_segment.segment_id,
        )?;
        if edges.iter().any(|edge| edge.kind != expected_kind) {
            return Err(RouteMembershipError::RampBinding(format!(
                "{} has an edge kind inconsistent with its endpoint role",
                endpoint_segment.segment_id
            )));
        }
        let matches = membership
            .segments
            .iter()
            .filter(|segment| {
                segment.source_kind == RouteMembershipSourceKind::BoundRamp
                    && segment.ordered_edge_ids == endpoint_segment.edge_ids
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(RouteMembershipError::RampBinding(format!(
                "{} must match exactly one boundRamp membership segment (matches={})",
                endpoint_segment.segment_id,
                matches.len()
            )));
        }
        if source_segment_ids.contains(&matches[0].segment_id) {
            return Err(RouteMembershipError::RampBinding(format!(
                "endpoint segment {} is referenced more than once",
                endpoint_segment.segment_id
            )));
        }
        source_segment_ids.push(matches[0].segment_id.clone());
        edge_ids.extend(endpoint_segment.edge_ids.iter().cloned());
    }
    if source_segment_ids.is_empty() || edge_ids.is_empty() {
        return Err(RouteMembershipError::RampBinding(format!(
            "endpoint {} has no resolved directed segments",
            endpoint.ramp_id
        )));
    }
    validate_ordered_edges(graph, &edge_ids, &format!("endpoint:{}", endpoint.ramp_id))?;
    Ok((source_segment_ids, edge_ids))
}

fn core_resolved_route_segment(
    resolved_segment_id: String,
    role: shutoko_routing_core::RoutePlanSegmentRole,
    membership_id: String,
    source_segment_ids: Vec<String>,
    edge_ids: Vec<String>,
) -> Result<shutoko_routing_core::ResolvedRouteSegment, RouteMembershipError> {
    let edge_ids_sha256 = ordered_edge_ids_sha256(&edge_ids)
        .map_err(|error| RouteMembershipError::Segment(error.to_string()))?;
    Ok(shutoko_routing_core::ResolvedRouteSegment {
        resolved_segment_id,
        role,
        membership_id,
        source_segment_ids,
        edge_ids,
        edge_ids_sha256,
    })
}

fn entry_approach_resolved_segment(
    graph: &Graph,
    membership: &RouteMembershipIndex,
    seed: &RadialReturnBillingPairSeed,
) -> Result<shutoko_routing_core::ResolvedRouteSegment, RouteMembershipError> {
    if seed.entry_endpoint.support_state != EndpointSupportState::VerifiedBound {
        return Err(RouteMembershipError::RampBinding(
            "entry endpoint is not verified_bound".into(),
        ));
    }
    let (mut source_segment_ids, mut edge_ids) =
        endpoint_bound_source_segments(graph, membership, &seed.entry_endpoint, EdgeKind::Entry)?;
    let entry_terminal_edge_id = edge_ids.last().ok_or_else(|| {
        RouteMembershipError::RampBinding("entry endpoint has no graph edges".into())
    })?;
    let entry_start_node_id = graph
        .edges
        .iter()
        .find(|edge| edge.id == *entry_terminal_edge_id)
        .map(|edge| edge.to.as_str())
        .ok_or_else(|| {
            RouteMembershipError::RampBinding("entry endpoint has no graph edges".into())
        })?;
    let terminal_edge_id = &seed.route_plan.entry_corridor.terminal_edge_id;
    let terminal_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == *terminal_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "entry terminal edge {} is absent from graph",
                terminal_edge_id
            ))
        })?;
    if terminal_edge.kind != EdgeKind::Shutoko
        || terminal_edge.to != seed.route_plan.entry_corridor.merge_node_id
    {
        return Err(RouteMembershipError::Validation(
            "entry terminal edge does not end at merge node M".into(),
        ));
    }
    let terminal_sources = relation_sequences(membership)
        .into_iter()
        .filter(|segment| segment.ordered_edge_ids.contains(terminal_edge_id))
        .collect::<Vec<_>>();
    if terminal_sources.len() != 1 {
        return Err(RouteMembershipError::Validation(format!(
            "entry terminal edge {} must occur in exactly one relationMainline source (matches={})",
            terminal_edge_id,
            terminal_sources.len()
        )));
    }
    let mut path = if entry_start_node_id == terminal_edge.from.as_str() {
        Vec::new()
    } else {
        find_relation_path_to_node(
            graph,
            membership,
            entry_start_node_id,
            None,
            &terminal_edge.from,
            CORRIDOR_EXIT_STATE_BUDGET,
        )?
    };
    path.push(RelationPathEdge {
        edge_id: terminal_edge.id.clone(),
        source_segment_id: terminal_sources[0].segment_id.clone(),
    });
    edge_ids.extend(path.iter().map(|edge| edge.edge_id.clone()));
    for edge in &path {
        if source_segment_ids.last() != Some(&edge.source_segment_id) {
            source_segment_ids.push(edge.source_segment_id.clone());
        }
    }
    validate_ordered_edge_sequence_allowing_repeats(graph, &edge_ids, "promoted entry approach")?;
    core_resolved_route_segment(
        format!("{}:resolved:entry", seed.id),
        shutoko_routing_core::RoutePlanSegmentRole::EntryApproach,
        membership.membership_id.clone(),
        source_segment_ids,
        edge_ids,
    )
}

pub fn promote_verified_radial_pair(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    seed: &RadialReturnBillingPairSeed,
    resolution: &DirectedRoutePlanResolution,
) -> Result<shutoko_routing_core::RadialReturnBillingPair, RouteMembershipError> {
    if seed.entry_endpoint.support_state != EndpointSupportState::VerifiedBound
        || seed.exit_endpoint.support_state != EndpointSupportState::VerifiedBound
        || resolution.first_exit.exact_directed_binding != EndpointSupportState::VerifiedBound
    {
        return Err(RouteMembershipError::RampBinding(
            "radialReturn promotion requires verified entry and exit endpoints".into(),
        ));
    }
    let exit = resolution.first_exit.exit.as_ref().ok_or_else(|| {
        RouteMembershipError::RampBinding(
            "radialReturn promotion has no resolved First Exit".into(),
        )
    })?;
    let entry_membership = corridor_membership(
        route_memberships,
        &seed.route_plan.entry_corridor.membership_id,
    )?;
    let lap_membership = corridor_membership(
        route_memberships,
        &seed.route_plan.mandatory_lap.membership_id,
    )?;
    let return_membership = corridor_membership(
        route_memberships,
        &seed.route_plan.return_corridor.membership_id,
    )?;
    let entry_segment = entry_approach_resolved_segment(graph, entry_membership, seed)?;
    let lap_segment = core_resolved_route_segment(
        format!("{}:resolved:lap", seed.id),
        shutoko_routing_core::RoutePlanSegmentRole::MandatoryLap,
        lap_membership.membership_id.clone(),
        vec![resolution.lap.source_segment_id.clone()],
        resolution.lap.edge_ids.clone(),
    )?;
    let return_segment = core_resolved_route_segment(
        format!("{}:resolved:return", seed.id),
        shutoko_routing_core::RoutePlanSegmentRole::ReturnCorridor,
        return_membership.membership_id.clone(),
        exit.mainline_source_segment_ids.clone(),
        exit.mainline_edge_ids.clone(),
    )?;
    let (exit_source_segment_ids, exit_edge_ids) = endpoint_bound_source_segments(
        graph,
        return_membership,
        &seed.exit_endpoint,
        EdgeKind::Exit,
    )?;
    let exit_segment = core_resolved_route_segment(
        format!("{}:resolved:exit", seed.id),
        shutoko_routing_core::RoutePlanSegmentRole::ExitApproach,
        return_membership.membership_id.clone(),
        exit_source_segment_ids,
        exit_edge_ids.clone(),
    )?;
    let resolved_route_segments = vec![entry_segment, lap_segment, return_segment, exit_segment];
    let builder_segments = resolved_route_segments
        .iter()
        .map(|segment| ResolvedRouteSegment {
            resolved_segment_id: segment.resolved_segment_id.clone(),
            role: match segment.role {
                shutoko_routing_core::RoutePlanSegmentRole::EntryApproach => {
                    RoutePlanSegmentRole::EntryApproach
                }
                shutoko_routing_core::RoutePlanSegmentRole::MandatoryLap => {
                    RoutePlanSegmentRole::MandatoryLap
                }
                shutoko_routing_core::RoutePlanSegmentRole::ReturnCorridor => {
                    RoutePlanSegmentRole::ReturnCorridor
                }
                shutoko_routing_core::RoutePlanSegmentRole::ExitApproach => {
                    RoutePlanSegmentRole::ExitApproach
                }
            },
            membership_id: segment.membership_id.clone(),
            source_segment_ids: segment.source_segment_ids.clone(),
            edge_ids: segment.edge_ids.clone(),
            edge_ids_sha256: segment.edge_ids_sha256.clone(),
        })
        .collect::<Vec<_>>();
    validate_resolved_route_plan(graph, route_memberships, &builder_segments)?;
    let entry_endpoint_edges = seed
        .entry_endpoint
        .directed_segments
        .iter()
        .flat_map(|segment| segment.edge_ids.iter().cloned())
        .collect::<Vec<_>>();
    let entry_id = entry_endpoint_edges
        .first()
        .cloned()
        .ok_or_else(|| RouteMembershipError::RampBinding("entry endpoint has no Edge".into()))?;
    let exit_id = exit_edge_ids
        .last()
        .cloned()
        .ok_or_else(|| RouteMembershipError::RampBinding("exit endpoint has no Edge".into()))?;
    if exit.edge_ids.last() != Some(&exit_id) {
        return Err(RouteMembershipError::RampBinding(
            "resolved First Exit does not end at the declared exit endpoint".into(),
        ));
    }
    let edge_count = u16::try_from(seed.route_plan.anchor.excluded_short_connector.edge_count)
        .map_err(|_| {
            RouteMembershipError::Validation("excluded connector edge count is too large".into())
        })?;
    Ok(shutoko_routing_core::RadialReturnBillingPair {
        id: seed.id.clone(),
        pair_kind: shutoko_routing_core::PairKind::RadialReturn,
        route_plan_version: 1,
        vehicle_profile: seed.vehicle_profile.clone(),
        entry_id,
        exit_id,
        entry_endpoint: core_billing_endpoint(&seed.entry_endpoint),
        exit_endpoint: core_billing_endpoint(&seed.exit_endpoint),
        route_plan: shutoko_routing_core::RoutePlanV1 {
            entry_corridor: shutoko_routing_core::EntryCorridor {
                membership_id: seed.route_plan.entry_corridor.membership_id.clone(),
                terminal_edge_id: seed.route_plan.entry_corridor.terminal_edge_id.clone(),
                merge_node_id: seed.route_plan.entry_corridor.merge_node_id.clone(),
            },
            anchor: shutoko_routing_core::RouteAnchor::DirectedJunction(
                shutoko_routing_core::DirectedJunctionAnchor {
                    merge_node_id: seed.route_plan.anchor.merge_node_id.clone(),
                    branch_node_id: seed.route_plan.anchor.branch_node_id.clone(),
                    merge_terminal_edge_id: seed.route_plan.anchor.merge_terminal_edge_id.clone(),
                    branch_initial_edge_id: seed.route_plan.anchor.branch_initial_edge_id.clone(),
                    route_id: seed.route_plan.anchor.route_id.clone(),
                    direction: seed.route_plan.anchor.direction.clone(),
                    arc_policy: shutoko_routing_core::ArcPolicy::OrdinaryLongArc,
                    excluded_short_connector: shutoko_routing_core::ExcludedShortConnector {
                        from_node_id: seed
                            .route_plan
                            .anchor
                            .excluded_short_connector
                            .from_node_id
                            .clone(),
                        to_node_id: seed
                            .route_plan
                            .anchor
                            .excluded_short_connector
                            .to_node_id
                            .clone(),
                        osm_way_id: seed.route_plan.anchor.excluded_short_connector.osm_way_id,
                        edge_count,
                        distance_meters: seed
                            .route_plan
                            .anchor
                            .excluded_short_connector
                            .distance_meters,
                    },
                },
            ),
            mandatory_lap: shutoko_routing_core::MandatoryLap {
                membership_id: seed.route_plan.mandatory_lap.membership_id.clone(),
                first_edge_id: seed.route_plan.mandatory_lap.first_edge_id.clone(),
                last_edge_id: seed.route_plan.mandatory_lap.last_edge_id.clone(),
                lap_count: 1,
            },
            return_corridor: shutoko_routing_core::ReturnCorridor {
                membership_id: seed.route_plan.return_corridor.membership_id.clone(),
                start_node_id: seed.route_plan.return_corridor.start_node_id.clone(),
                initial_edge_id: seed.route_plan.return_corridor.initial_edge_id.clone(),
                first_general_exit: shutoko_routing_core::FirstGeneralExit {
                    rule: "firstGeneralExit".into(),
                    expected_ramp_id: seed
                        .route_plan
                        .return_corridor
                        .first_general_exit
                        .expected_ramp_id
                        .clone(),
                    exact_directed_binding:
                        shutoko_routing_core::EndpointSupportState::VerifiedBound,
                },
            },
        },
        resolved_route_segments,
        routing_capability: match seed.routing_capability {
            crate::seed::RoutingCapability::Routable => {
                shutoko_routing_core::RoutingCapability::Routable
            }
            crate::seed::RoutingCapability::StructuralNoLoop => {
                shutoko_routing_core::RoutingCapability::StructuralNoLoop
            }
            crate::seed::RoutingCapability::Unsupported => {
                shutoko_routing_core::RoutingCapability::Unsupported
            }
        },
        pair_eligibility: shutoko_routing_core::PairEligibility {
            status: match seed.pair_eligibility.status {
                crate::seed::PairEligibilityStatus::VerifiedOneSectionAhead => {
                    shutoko_routing_core::PairEligibilityStatus::VerifiedOneSectionAhead
                }
                crate::seed::PairEligibilityStatus::Unverified => {
                    shutoko_routing_core::PairEligibilityStatus::Unverified
                }
                crate::seed::PairEligibilityStatus::TopologyOnly => {
                    shutoko_routing_core::PairEligibilityStatus::TopologyOnly
                }
            },
            one_section_ahead_verified: seed.pair_eligibility.one_section_ahead_verified,
        },
        loop_validation: shutoko_routing_core::LoopValidation {
            status: match seed.loop_validation.status {
                crate::seed::LoopValidationStatus::DeclaredRouteValidated => {
                    shutoko_routing_core::LoopValidationStatus::DeclaredRouteValidated
                }
                crate::seed::LoopValidationStatus::Unresolved => {
                    shutoko_routing_core::LoopValidationStatus::Unresolved
                }
                crate::seed::LoopValidationStatus::TopologyOnly => {
                    shutoko_routing_core::LoopValidationStatus::TopologyOnly
                }
            },
        },
        tariff: shutoko_routing_core::Tariff {
            status: match seed.tariff.status {
                crate::seed::TariffStatus::Priced => shutoko_routing_core::TariffStatus::Priced,
                crate::seed::TariffStatus::Unpriced => shutoko_routing_core::TariffStatus::Unpriced,
                crate::seed::TariffStatus::Expired => shutoko_routing_core::TariffStatus::Expired,
                crate::seed::TariffStatus::NotApplicable => {
                    shutoko_routing_core::TariffStatus::NotApplicable
                }
            },
            amount_yen: seed.tariff.amount_yen,
            billing_distance_meters: seed.tariff.billing_distance_meters,
            prices: seed
                .tariff
                .prices
                .iter()
                .map(|price| shutoko_routing_core::Price {
                    amount_yen: price.amount_yen,
                    effective_from: price.effective_from.clone(),
                    effective_to: price.effective_to.clone(),
                })
                .collect(),
        },
    })
}

pub fn find_first_exit_on_corridor_from_edge(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
) -> Result<CorridorExit, RouteMembershipError> {
    let initial_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == initial_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "initial edge {} is absent from graph",
                initial_edge_id
            ))
        })?;
    find_first_exit_on_corridor(
        graph,
        route_memberships,
        membership_id,
        &initial_edge.from,
        initial_edge_id,
        expected_ramp_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn node(id: &str) -> Node {
        Node {
            id: id.into(),
            lat: 35.0,
            lon: 139.0,
        }
    }

    fn edge(id: &str, from: &str, to: &str, kind: EdgeKind) -> Edge {
        Edge {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            kind,
            duration_seconds: 10,
            distance_meters: 100,
            name: None,
        }
    }

    fn way(id: i64, nodes: Vec<i64>) -> OsmElement {
        let mut tags = BTreeMap::new();
        tags.insert("highway".into(), "motorway".into());
        OsmElement {
            element_type: "way".into(),
            id,
            lat: None,
            lon: None,
            nodes: Some(nodes),
            tags: Some(tags),
            members: None,
        }
    }

    fn relation(id: i64, direction: &str, members: Vec<OsmMember>) -> OsmElement {
        let mut tags = BTreeMap::new();
        tags.insert("type".into(), "route".into());
        tags.insert("route".into(), "road".into());
        tags.insert("ref".into(), "R1".into());
        tags.insert("direction".into(), direction.into());
        OsmElement {
            element_type: "relation".into(),
            id,
            lat: None,
            lon: None,
            nodes: None,
            tags: Some(tags),
            members: Some(members),
        }
    }

    fn member(id: i64) -> OsmMember {
        OsmMember {
            member_type: "way".into(),
            ref_id: id,
            role: String::new(),
        }
    }

    fn synthetic() -> (OverpassResponse, Graph, String) {
        let response = OverpassResponse {
            version: Some(1.0),
            generator: None,
            elements: vec![
                way(101, vec![1, 2]),
                way(102, vec![2, 3]),
                way(103, vec![3, 4]),
                way(201, vec![7, 8, 10]),
                way(202, vec![4, 9]),
                relation(10, "forward", vec![member(101), member(102), member(103)]),
                way(203, vec![9, 10]),
            ],
        };
        let graph = Graph {
            schema_version: 2,
            release_id: "fixture".into(),
            vehicle_profile: "passenger-car-etc".into(),
            nodes: vec![
                node("n:1"),
                node("n:2"),
                node("n:3"),
                node("n:4"),
                node("n:7"),
                node("n:8"),
                node("n:9"),
                node("n:10"),
            ],
            edges: vec![
                edge("e:w101:0:f", "n:1", "n:2", EdgeKind::Shutoko),
                edge("e:w101:0:r", "n:2", "n:1", EdgeKind::Shutoko),
                edge("e:w102:0:f", "n:2", "n:3", EdgeKind::Shutoko),
                edge("e:w102:0:r", "n:3", "n:2", EdgeKind::Shutoko),
                edge("e:w103:0:f", "n:3", "n:4", EdgeKind::Shutoko),
                edge("e:w103:0:r", "n:4", "n:3", EdgeKind::Shutoko),
                edge("e:w201:0:f", "n:7", "n:8", EdgeKind::Entry),
                edge("e:w201:1:f", "n:8", "n:10", EdgeKind::Entry),
                edge("e:w202:0:f", "n:4", "n:9", EdgeKind::Exit),
                edge("e:w203:0:f", "n:9", "n:10", EdgeKind::Exit),
            ],
            billing_pairs: Vec::new(),
            forbidden_transitions: Vec::new(),
            ramps: vec![
                Ramp {
                    id: "ramp:entry".into(),
                    facility_id: "entry".into(),
                    name: "entry".into(),
                    route: "R1".into(),
                    direction: "forward".into(),
                    kind: RampKind::GeneralEntry,
                    edge_id: "e:w201:0:f".into(),
                    node_id: "n:7".into(),
                    mainline_node_id: "n:8".into(),
                    restrictions: Vec::new(),
                },
                Ramp {
                    id: "ramp:exit".into(),
                    facility_id: "exit".into(),
                    name: "exit".into(),
                    route: "R1".into(),
                    direction: "forward".into(),
                    kind: RampKind::GeneralExit,
                    edge_id: "e:w202:0:f".into(),
                    node_id: "n:9".into(),
                    mainline_node_id: "n:4".into(),
                    restrictions: Vec::new(),
                },
            ],
            od_tariffs: Vec::new(),
        };
        (response, graph, compute_sha256(b"fixture-snapshot"))
    }

    fn evidence() -> Vec<BoundRampEvidence> {
        let entry_edge_ids = vec!["e:w201:0:f".into(), "e:w201:1:f".into()];
        let exit_edge_ids = vec!["e:w202:0:f".into(), "e:w203:0:f".into()];
        vec![
            BoundRampEvidence {
                binding_evidence_id: "binding:entry".into(),
                ramp_id: "ramp:entry".into(),
                route_id: "R1".into(),
                direction: "forward".into(),
                osm_way_ids: vec![201],
                osm_node_ids: vec![7, 8, 10],
                from_node_id: "n:7".into(),
                to_node_id: "n:10".into(),
                edge_ids_sha256: ordered_edge_ids_sha256(&entry_edge_ids).unwrap(),
                edge_ids: entry_edge_ids,
            },
            BoundRampEvidence {
                binding_evidence_id: "binding:exit".into(),
                ramp_id: "ramp:exit".into(),
                route_id: "R1".into(),
                direction: "forward".into(),
                osm_way_ids: vec![202, 203],
                osm_node_ids: vec![4, 9, 10],
                from_node_id: "n:4".into(),
                to_node_id: "n:10".into(),
                edge_ids_sha256: ordered_edge_ids_sha256(&exit_edge_ids).unwrap(),
                edge_ids: exit_edge_ids,
            },
        ]
    }

    fn verified_diagnostic_radial_seed() -> RadialReturnBillingPairSeed {
        let parsed = crate::seed::parse_billing_pairs_seed(include_str!(
            "../../../fixtures/seed-v2/diagnostic-radial-v2.json"
        ))
        .unwrap();
        let mut seed = parsed.radial_pairs()[0].clone();
        let exit_edge_ids = vec!["e:w204:0:f".into()];
        let entry_edge_ids = vec!["e:w201:0:f".into(), "e:w201:1:f".into()];
        seed.entry_endpoint.ramp_id = "ramp:entry".into();
        seed.entry_endpoint.directed_segments = vec![DirectedEndpointSegment {
            segment_id: "binding:entry:candidate:0".into(),
            osm_way_ids: vec![201],
            osm_node_ids: vec![7, 8, 10],
            edge_ids: entry_edge_ids.clone(),
            from_node_id: "n:7".into(),
            to_node_id: "n:10".into(),
            edge_ids_sha256: ordered_edge_ids_sha256(&entry_edge_ids).unwrap(),
        }];
        seed.exit_endpoint.ramp_id = "ramp:expected".into();
        seed.exit_endpoint.support_state = EndpointSupportState::VerifiedBound;
        seed.exit_endpoint.directed_segments = vec![DirectedEndpointSegment {
            segment_id: "binding:expected:candidate:0".into(),
            osm_way_ids: vec![204],
            osm_node_ids: vec![3, 11],
            edge_ids: exit_edge_ids.clone(),
            from_node_id: "n:3".into(),
            to_node_id: "n:11".into(),
            edge_ids_sha256: ordered_edge_ids_sha256(&exit_edge_ids).unwrap(),
        }];
        seed.exit_endpoint.binding_candidates.clear();
        seed.pair_eligibility.status = crate::seed::PairEligibilityStatus::VerifiedOneSectionAhead;
        seed.pair_eligibility.one_section_ahead_verified = true;
        seed.route_plan.entry_corridor.membership_id = "route:R1:forward".into();
        seed.route_plan.entry_corridor.terminal_edge_id = "e:w101:0:f".into();
        seed.route_plan.entry_corridor.merge_node_id = "n:2".into();
        seed.route_plan.anchor.route_id = "R1".into();
        seed.route_plan.anchor.direction = "forward".into();
        seed.route_plan.anchor.merge_node_id = "n:2".into();
        seed.route_plan.anchor.branch_node_id = "n:4".into();
        seed.route_plan.anchor.merge_terminal_edge_id = "e:w101:0:f".into();
        seed.route_plan.anchor.branch_initial_edge_id = "e:w999:1:f".into();
        seed.route_plan.anchor.excluded_short_connector.from_node_id = "n:4".into();
        seed.route_plan.anchor.excluded_short_connector.to_node_id = "n:2".into();
        seed.route_plan.anchor.excluded_short_connector.osm_way_id = 998;
        seed.route_plan.anchor.excluded_short_connector.edge_count = 1;
        seed.route_plan
            .anchor
            .excluded_short_connector
            .distance_meters = 100;
        seed.route_plan.mandatory_lap.membership_id = "route:R1:forward".into();
        seed.route_plan.mandatory_lap.first_edge_id = "e:w102:0:f".into();
        seed.route_plan.mandatory_lap.last_edge_id = "e:w103:0:f".into();
        seed.route_plan.return_corridor.membership_id = "route:R1:forward".into();
        seed.route_plan.return_corridor.start_node_id = "n:4".into();
        seed.route_plan.return_corridor.initial_edge_id = "e:w999:1:f".into();
        seed.route_plan
            .return_corridor
            .first_general_exit
            .expected_ramp_id = "ramp:expected".into();
        seed.route_plan
            .return_corridor
            .first_general_exit
            .exact_directed_binding = EndpointSupportState::VerifiedBound;
        seed.validate().unwrap();
        seed
    }

    fn verified_diagnostic_radial_fixture() -> (
        RadialReturnBillingPairSeed,
        Graph,
        Vec<RouteMembershipIndex>,
    ) {
        let (mut response, mut graph, snapshot) = synthetic();
        response.elements.push(way(104, vec![10, 1]));
        response.elements[5]
            .members
            .as_mut()
            .unwrap()
            .push(member(104));
        graph
            .edges
            .push(edge("e:w104:0:f", "n:10", "n:1", EdgeKind::Shutoko));
        graph.nodes.push(node("n:11"));
        graph.nodes.push(node("n:12"));
        graph
            .edges
            .push(edge("e:w998:0:f", "n:4", "n:2", EdgeKind::Shutoko));
        graph
            .edges
            .push(edge("e:w999:1:f", "n:4", "n:3", EdgeKind::Shutoko));
        graph
            .edges
            .push(edge("e:w204:0:f", "n:3", "n:11", EdgeKind::Exit));
        graph
            .edges
            .push(edge("e:w205:0:f", "n:11", "n:12", EdgeKind::Exit));
        graph
            .edges
            .push(edge("e:w205:1:f", "n:12", "n:10", EdgeKind::Exit));
        graph.ramps.push(Ramp {
            id: "ramp:expected".into(),
            facility_id: "expected".into(),
            name: "expected".into(),
            route: "R1".into(),
            direction: "forward".into(),
            kind: RampKind::GeneralExit,
            edge_id: "e:w204:0:f".into(),
            node_id: "n:11".into(),
            mainline_node_id: "n:3".into(),
            restrictions: Vec::new(),
        });
        let expected_edge_ids = vec!["e:w204:0:f".into()];
        let mut bound_ramp_evidence = evidence();
        bound_ramp_evidence.push(BoundRampEvidence {
            binding_evidence_id: "binding:expected".into(),
            ramp_id: "ramp:expected".into(),
            route_id: "R1".into(),
            direction: "forward".into(),
            osm_way_ids: vec![204],
            osm_node_ids: vec![3, 11],
            from_node_id: "n:3".into(),
            to_node_id: "n:11".into(),
            edge_ids_sha256: ordered_edge_ids_sha256(&expected_edge_ids).unwrap(),
            edge_ids: expected_edge_ids,
        });
        let mut memberships = build_route_membership_indices(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot,
                relation_ids: None,
                bound_ramp_evidence,
            },
        )
        .unwrap();
        let segment = memberships
            .iter_mut()
            .flat_map(|membership| membership.segments.iter_mut())
            .find(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
            .unwrap();
        segment.ordered_edge_ids.push("e:w999:1:f".into());
        segment.ordered_edge_ids_sha256 =
            ordered_edge_ids_sha256(&segment.ordered_edge_ids).unwrap();
        (verified_diagnostic_radial_seed(), graph, memberships)
    }

    #[test]
    fn diagnostic_resolver_requires_endpoint_and_first_exit_binding_to_match() {
        let (seed, graph, memberships) = verified_diagnostic_radial_fixture();
        let resolution = resolve_diagnostic_radial_route_plan(&graph, &memberships, &seed).unwrap();
        assert_eq!(
            resolution.first_exit.exit.as_ref().unwrap().ramp_id,
            "ramp:expected"
        );

        let mut wrong_ramp = seed.clone();
        wrong_ramp
            .route_plan
            .return_corridor
            .first_general_exit
            .expected_ramp_id = "ramp:other".into();
        let error =
            resolve_diagnostic_radial_route_plan(&graph, &memberships, &wrong_ramp).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::RampBinding(message)
                if message == "exit endpoint ramp ramp:expected does not match first-general-exit expectation ramp:other"
        ));

        let mut wrong_state = seed;
        let declared_candidate = wrong_state.exit_endpoint.directed_segments[0].clone();
        wrong_state.exit_endpoint.support_state = EndpointSupportState::Unsupported;
        wrong_state.exit_endpoint.directed_segments.clear();
        wrong_state.exit_endpoint.binding_candidates = vec![crate::seed::BindingCandidate {
            candidate_id: "tengenji:unresolved".into(),
            status: crate::seed::BindingCandidateStatus::Unresolved,
            directed_segments: vec![declared_candidate],
        }];
        wrong_state
            .route_plan
            .return_corridor
            .first_general_exit
            .exact_directed_binding = EndpointSupportState::VerifiedBound;
        let error =
            resolve_diagnostic_radial_route_plan(&graph, &memberships, &wrong_state).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::RampBinding(message)
                if message == "exit endpoint supportState unsupported does not match first-general-exit exactDirectedBinding verified_bound"
        ));
    }

    #[test]
    fn resolves_a_corridor_across_relation_segment_boundaries() {
        let (_response, graph, snapshot) = synthetic();
        let first_edges = vec!["e:w102:0:f".to_string()];
        let second_edges = vec!["e:w103:0:f".to_string()];
        let membership = RouteMembershipIndex {
            membership_id: "route:split:forward".into(),
            route_id: "split".into(),
            direction: "forward".into(),
            direction_mapping_version: ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION.into(),
            segments: vec![
                RouteMembershipSegment {
                    segment_id: "relation:split:forward:0".into(),
                    source_kind: RouteMembershipSourceKind::RelationMainline,
                    source_relation_id: Some("split".into()),
                    source_snapshot_sha256: snapshot.clone(),
                    binding_evidence_id: None,
                    ordered_edge_ids_sha256: ordered_edge_ids_sha256(&first_edges).unwrap(),
                    ordered_edge_ids: first_edges,
                    member_indexes: Some(vec![0]),
                    member_order_matches_relation: Some(true),
                },
                RouteMembershipSegment {
                    segment_id: "relation:split:forward:1".into(),
                    source_kind: RouteMembershipSourceKind::RelationMainline,
                    source_relation_id: Some("split".into()),
                    source_snapshot_sha256: snapshot,
                    binding_evidence_id: None,
                    ordered_edge_ids_sha256: ordered_edge_ids_sha256(&second_edges).unwrap(),
                    ordered_edge_ids: second_edges,
                    member_indexes: Some(vec![1]),
                    member_order_matches_relation: Some(true),
                },
            ],
        };
        let path = find_relation_path_to_node(
            &graph,
            &membership,
            "n:2",
            Some("e:w102:0:f"),
            "n:4",
            CORRIDOR_EXIT_STATE_BUDGET,
        )
        .unwrap();
        assert_eq!(
            path.iter()
                .map(|edge| edge.edge_id.as_str())
                .collect::<Vec<_>>(),
            vec!["e:w102:0:f", "e:w103:0:f"]
        );
        assert_eq!(
            path.iter()
                .map(|edge| edge.source_segment_id.as_str())
                .collect::<Vec<_>>(),
            vec!["relation:split:forward:0", "relation:split:forward:1"]
        );
    }

    #[test]
    fn promotes_verified_radial_pair_into_schema4_union() {
        let (seed, graph, memberships) = verified_diagnostic_radial_fixture();
        let resolution = resolve_diagnostic_radial_route_plan(&graph, &memberships, &seed).unwrap();
        let promoted =
            promote_verified_radial_pair(&graph, &memberships, &seed, &resolution).unwrap();
        assert_eq!(promoted.resolved_route_segments.len(), 4);
        assert!(matches!(
            promoted.tariff.status,
            shutoko_routing_core::TariffStatus::Unpriced
        ));
        assert!(promoted.tariff.amount_yen.is_none());
        assert!(promoted.tariff.billing_distance_meters.is_none());
        assert_eq!(promoted.entry_id, "e:w201:0:f");
        assert_eq!(promoted.exit_id, "e:w204:0:f");
        assert_eq!(
            promoted.entry_endpoint.directed_segments[0]
                .osm_node_ids
                .len(),
            3
        );
        let json =
            graph_schema_v4_to_deterministic_json_with_radial(&graph, &memberships, vec![promoted])
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["billingPairs"].as_array().unwrap().len(), 1);
        assert_eq!(value["billingPairs"][0]["pairKind"], "radialReturn");
        let prepared = shutoko_routing_core::prepare_json(&json, "{}").unwrap();
        assert_eq!(prepared.radial_billing_pairs().len(), 1);
    }

    #[test]
    fn diagnostic_resolver_requires_exact_verified_exit_candidate_evidence() {
        let (seed, graph, memberships) = verified_diagnostic_radial_fixture();
        let mut reversed_ways = seed.clone();
        reversed_ways.exit_endpoint.directed_segments[0].osm_way_ids = vec![205];
        let error =
            resolve_diagnostic_radial_route_plan(&graph, &memberships, &reversed_ways).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::RampBinding(message) if message.contains("OSM way order")
        ));

        let mut alternate_graph = graph.clone();
        alternate_graph.nodes.push(node("n:13"));
        alternate_graph
            .edges
            .push(edge("e:w206:0:f", "n:3", "n:13", EdgeKind::Exit));
        alternate_graph
            .edges
            .push(edge("e:w206:1:f", "n:13", "n:10", EdgeKind::Exit));
        let alternate_edge_ids = vec!["e:w206:0:f".into(), "e:w206:1:f".into()];
        let mut alternate_path = seed.clone();
        alternate_path.exit_endpoint.directed_segments[0] = DirectedEndpointSegment {
            segment_id: "binding:expected:alternate:0".into(),
            osm_way_ids: vec![206],
            osm_node_ids: vec![3, 13, 10],
            edge_ids: alternate_edge_ids.clone(),
            from_node_id: "n:3".into(),
            to_node_id: "n:10".into(),
            edge_ids_sha256: ordered_edge_ids_sha256(&alternate_edge_ids).unwrap(),
        };
        let error =
            resolve_diagnostic_radial_route_plan(&alternate_graph, &memberships, &alternate_path)
                .unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::RampBinding(message)
                if message.contains("does not match the exact boundRamp segment")
        ));

        let mut non_exit_graph = graph;
        non_exit_graph
            .edges
            .iter_mut()
            .find(|edge| edge.id == "e:w204:0:f")
            .unwrap()
            .kind = EdgeKind::Entry;
        assert!(validate_declared_endpoint_candidate(
            &non_exit_graph,
            &seed.exit_endpoint.directed_segments[0],
            true
        )
        .is_err());
    }

    #[test]
    fn directed_route_plan_resolver_rejects_any_tampered_membership() {
        let (seed, graph, mut memberships) = verified_diagnostic_radial_fixture();
        let mut route_plan = seed.route_plan;
        route_plan
            .return_corridor
            .first_general_exit
            .exact_directed_binding = EndpointSupportState::Unresolved;
        resolve_directed_route_plan(&graph, &memberships, &route_plan).unwrap();
        let mut unrelated_segment = memberships[0].segments[0].clone();
        unrelated_segment.segment_id = "relation:unrelated:forward:0".into();
        unrelated_segment.ordered_edge_ids_sha256 = compute_sha256(b"tampered-membership");
        memberships.push(RouteMembershipIndex {
            membership_id: "route:unrelated:forward".into(),
            route_id: "unrelated".into(),
            direction: "forward".into(),
            direction_mapping_version: ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION.into(),
            segments: vec![unrelated_segment],
        });
        let error = resolve_directed_route_plan(&graph, &memberships, &route_plan).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::Segment(message)
                if message.contains("orderedEdgeIdsSha256 does not match")
        ));
    }

    #[test]
    fn normalizes_route_two_roles_and_rejects_tampered_relation_order() {
        let (mut response, graph, snapshot) = synthetic();
        let relation = &mut response.elements[5];
        relation
            .tags
            .as_mut()
            .unwrap()
            .insert("ref".into(), "2".into());
        relation.tags.as_mut().unwrap().remove("direction");
        relation.members.as_mut().unwrap()[0].role = "forward".into();
        relation.members.as_mut().unwrap()[1].role = "forward".into();
        relation.members.as_mut().unwrap()[2].role = "backward".into();
        let memberships = build_relation_memberships(&response, &graph, &snapshot, None).unwrap();
        assert!(memberships
            .iter()
            .any(|membership| membership.membership_id == "route:2:outbound"));
        assert!(memberships
            .iter()
            .any(|membership| membership.membership_id == "route:2:inbound"));
        assert!(memberships.iter().all(|membership| {
            membership.direction_mapping_version == ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION
        }));

        let mut tampered = build_route_memberships(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot,
                relation_ids: None,
                bound_ramp_evidence: evidence(),
            },
        )
        .unwrap();
        let segment = tampered
            .iter_mut()
            .flat_map(|membership| membership.segments.iter_mut())
            .find(|segment| {
                segment.source_kind == RouteMembershipSourceKind::RelationMainline
                    && segment.ordered_edge_ids.len() > 1
            })
            .unwrap();
        segment.ordered_edge_ids.swap(0, 1);
        segment.ordered_edge_ids_sha256 =
            ordered_edge_ids_sha256(&segment.ordered_edge_ids).unwrap();
        assert!(validate_route_memberships(
            &tampered,
            &graph,
            &response,
            &compute_sha256(b"fixture-snapshot"),
            &evidence()
        )
        .is_err());
    }

    #[test]
    fn preserves_relation_member_order_when_graph_order_differs() {
        let relation_segments = |memberships: &[RouteMembershipIndex]| {
            memberships
                .iter()
                .find(|membership| membership.membership_id == "route:R1:forward")
                .unwrap()
                .segments
                .iter()
                .map(|segment| segment.ordered_edge_ids.clone())
                .collect::<Vec<_>>()
        };
        let (mut topology_reordered, mut graph, snapshot) = synthetic();
        topology_reordered.elements.push(way(104, vec![4, 1]));
        topology_reordered.elements[5].members =
            Some(vec![member(101), member(104), member(102), member(103)]);
        graph
            .edges
            .push(edge("e:w104:0:f", "n:4", "n:1", EdgeKind::Shutoko));
        let memberships =
            build_relation_memberships(&topology_reordered, &graph, &snapshot, None).unwrap();
        assert_eq!(
            relation_segments(&memberships),
            vec![vec![
                "e:w101:0:f".to_owned(),
                "e:w102:0:f".to_owned(),
                "e:w103:0:f".to_owned(),
                "e:w104:0:f".to_owned(),
            ]]
        );
        let segment = &memberships[0].segments[0];
        assert_eq!(segment.member_indexes, Some(vec![0, 2, 3, 1]));
        assert_eq!(segment.member_order_matches_relation, Some(false));
    }

    #[test]
    fn rejects_ambiguous_oneway_external_duplicate_and_fallback_relation_paths() {
        let (mut branch, mut graph, snapshot) = synthetic();
        branch.elements.push(way(104, vec![2, 5]));
        branch.elements[5].members = Some(vec![member(101), member(102), member(104)]);
        graph.nodes.push(node("n:5"));
        graph
            .edges
            .push(edge("e:w104:0:f", "n:2", "n:5", EdgeKind::Shutoko));
        let error = build_relation_memberships(&branch, &graph, &snapshot, None).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::Relation(message)
                if message.contains("ambiguous directed branch")
        ));

        let (mut one_way, mut graph, snapshot) = synthetic();
        one_way.elements[5]
            .tags
            .as_mut()
            .unwrap()
            .insert("direction".into(), "backward".into());
        graph.edges.retain(|edge| !edge.id.ends_with(":r"));
        let error = build_relation_memberships(&one_way, &graph, &snapshot, None).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::Relation(message)
                if message.contains("no graph edges in requested direction")
        ));

        let (mut external, mut graph, snapshot) = synthetic();
        external.elements[1].nodes = Some(vec![5, 6]);
        external.elements[2].nodes = Some(vec![6, 7]);
        external.elements.push(way(999, vec![2, 5]));
        graph.nodes.push(node("n:5"));
        graph.nodes.push(node("n:6"));
        graph.nodes.push(node("n:7"));
        graph
            .edges
            .retain(|edge| !edge.id.starts_with("e:w102:") && !edge.id.starts_with("e:w103:"));
        graph
            .edges
            .push(edge("e:w102:0:f", "n:5", "n:6", EdgeKind::Shutoko));
        graph
            .edges
            .push(edge("e:w103:0:f", "n:6", "n:7", EdgeKind::Shutoko));
        graph
            .edges
            .push(edge("e:w999:0:f", "n:2", "n:5", EdgeKind::Shutoko));
        let error = build_relation_memberships(&external, &graph, &snapshot, None).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::Relation(message)
                if message.contains("way outside the relation")
        ));

        let (mut duplicate, graph, snapshot) = synthetic();
        duplicate.elements[5]
            .members
            .as_mut()
            .unwrap()
            .push(member(101));
        let error = build_relation_memberships(&duplicate, &graph, &snapshot, None).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::Relation(message) if message.contains("repeats way 101")
        ));

        let (mut fallback, mut graph, snapshot) = synthetic();
        fallback.elements[5]
            .tags
            .as_mut()
            .unwrap()
            .remove("direction");
        graph.edges.retain(|edge| {
            !edge.id.starts_with("e:w101:0:f")
                && !edge.id.starts_with("e:w102:0:f")
                && !edge.id.starts_with("e:w103:0:f")
        });
        let error = build_relation_memberships(&fallback, &graph, &snapshot, None).unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::Relation(message)
                if message.contains("no graph edges in requested direction")
        ));
    }

    #[test]
    fn filters_conflicting_route_links_and_keeps_c1_arms_separate() {
        let (mut response, graph, snapshot) = synthetic();
        response.elements[1].nodes = Some(vec![3, 2]);
        response.elements[2].nodes = Some(vec![4, 3]);
        let relation = &mut response.elements[5];
        relation
            .tags
            .as_mut()
            .unwrap()
            .insert("ref".into(), "C1".into());
        relation.tags.as_mut().unwrap().remove("direction");
        relation.members.as_mut().unwrap()[0].role = "Inner directions".into();
        relation.members.as_mut().unwrap()[1].role = "Outer directions".into();
        relation.members.as_mut().unwrap()[2].role = "Outer directions".into();
        let mut conflicting_link = way(301, vec![4, 9]);
        conflicting_link
            .tags
            .as_mut()
            .unwrap()
            .insert("highway".into(), "motorway_link".into());
        conflicting_link
            .tags
            .as_mut()
            .unwrap()
            .insert("nat_ref".into(), "8".into());
        conflicting_link
            .tags
            .as_mut()
            .unwrap()
            .insert("name".into(), "首都高速都心環状線".into());
        relation.members.as_mut().unwrap().push(member(301));
        response.elements.push(conflicting_link);
        let memberships = build_relation_memberships(&response, &graph, &snapshot, None).unwrap();
        assert!(memberships
            .iter()
            .any(|membership| membership.membership_id == "route:C1:inner"));
        assert!(memberships
            .iter()
            .any(|membership| membership.membership_id == "route:C1:outer"));
        assert!(!memberships
            .iter()
            .any(|membership| { membership.membership_id == "route:C1:backward" }));
        assert!(memberships.iter().all(|membership| membership
            .segments
            .iter()
            .all(|segment| !segment.ordered_edge_ids.iter().any(|id| id == "e:w301:0:f"))));
    }

    #[test]
    fn validates_directed_junction_arm_and_short_connector_contract() {
        let (response, mut graph, snapshot) = synthetic();
        graph.nodes.push(node("n:5"));
        graph
            .edges
            .push(edge("e:w999:0:f", "n:4", "n:2", EdgeKind::Shutoko));
        graph
            .edges
            .push(edge("e:w999:1:f", "n:4", "n:5", EdgeKind::Shutoko));
        graph
            .edges
            .push(edge("e:w999:2:f", "n:4", "n:3", EdgeKind::Shutoko));
        let memberships = build_route_membership_indices(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot,
                relation_ids: None,
                bound_ramp_evidence: evidence(),
            },
        )
        .unwrap();
        let anchor = DirectedJunctionAnchor {
            anchor_kind: crate::seed::AnchorKind::DirectedJunction,
            route_id: "R1".into(),
            direction: "forward".into(),
            merge_node_id: "n:2".into(),
            branch_node_id: "n:4".into(),
            merge_terminal_edge_id: "e:w101:0:f".into(),
            branch_initial_edge_id: "e:w999:1:f".into(),
            arc_policy: crate::seed::ArcPolicy::OrdinaryLongArc,
            excluded_short_connector: crate::seed::ExcludedShortConnector {
                from_node_id: "n:4".into(),
                to_node_id: "n:2".into(),
                osm_way_id: 999,
                edge_count: 1,
                distance_meters: 100,
            },
        };
        let lap = MandatoryLap {
            membership_id: "route:R1:forward".into(),
            first_edge_id: "e:w102:0:f".into(),
            last_edge_id: "e:w103:0:f".into(),
            lap_count: 1,
        };
        validate_directed_junction_mandatory_lap(&graph, &memberships, &anchor, &lap).unwrap();
        let mut wrong_arm = anchor.clone();
        wrong_arm.merge_node_id = "n:3".into();
        assert!(
            validate_directed_junction_mandatory_lap(&graph, &memberships, &wrong_arm, &lap)
                .is_err()
        );
        let mut wrong_way = anchor.clone();
        wrong_way.excluded_short_connector.osm_way_id = 1000;
        assert!(
            validate_directed_junction_mandatory_lap(&graph, &memberships, &wrong_way, &lap)
                .is_err()
        );
        let mut wrong_endpoints = anchor;
        wrong_endpoints.excluded_short_connector.to_node_id = "n:3".into();
        assert!(validate_directed_junction_mandatory_lap(
            &graph,
            &memberships,
            &wrong_endpoints,
            &lap
        )
        .is_err());
    }

    #[test]
    fn generates_the_long_arc_and_rejects_an_invalid_lap_count() {
        let (response, graph, snapshot) = synthetic();
        let memberships = build_route_membership_indices(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot,
                relation_ids: None,
                bound_ramp_evidence: evidence(),
            },
        )
        .unwrap();
        let anchor = DirectedJunctionAnchor {
            anchor_kind: crate::seed::AnchorKind::DirectedJunction,
            route_id: "R1".into(),
            direction: "forward".into(),
            merge_node_id: "n:2".into(),
            branch_node_id: "n:4".into(),
            merge_terminal_edge_id: "e:w101:0:f".into(),
            branch_initial_edge_id: "e:w999:1:f".into(),
            arc_policy: crate::seed::ArcPolicy::OrdinaryLongArc,
            excluded_short_connector: crate::seed::ExcludedShortConnector {
                from_node_id: "n:4".into(),
                to_node_id: "n:2".into(),
                osm_way_id: 999,
                edge_count: 1,
                distance_meters: 100,
            },
        };
        let mut graph = graph;
        graph
            .edges
            .push(edge("e:w999:0:f", "n:4", "n:2", EdgeKind::Shutoko));
        graph
            .edges
            .push(edge("e:w999:1:f", "n:4", "n:5", EdgeKind::Shutoko));
        graph.nodes.push(node("n:5"));
        let lap = generate_route_plan_lap_v1(&graph, &memberships, &anchor).unwrap();
        assert_eq!(lap.merge_node_id, "n:2");
        assert_eq!(lap.branch_node_id, "n:4");
        assert_eq!(lap.first_edge_id, "e:w102:0:f");
        assert_eq!(lap.last_edge_id, "e:w103:0:f");
        assert_eq!(lap.lap_count, 1);
        assert_eq!(lap.edge_ids, vec!["e:w102:0:f", "e:w103:0:f"]);
        assert_eq!(
            lap.edge_ids_sha256,
            ordered_edge_ids_sha256(&lap.edge_ids).unwrap()
        );
        let invalid_lap = MandatoryLap {
            membership_id: "route:R1:forward".into(),
            first_edge_id: "e:w102:0:f".into(),
            last_edge_id: "e:w103:0:f".into(),
            lap_count: 2,
        };
        assert!(validate_directed_junction_mandatory_lap(
            &graph,
            &memberships,
            &anchor,
            &invalid_lap
        )
        .is_err());
    }

    #[test]
    fn rejects_relation_mainline_arc_that_uses_the_anchor_connector() {
        let graph = Graph {
            schema_version: 2,
            release_id: "connector-mainline".into(),
            vehicle_profile: "passenger-car-etc".into(),
            nodes: vec![
                node("n:0"),
                node("n:1"),
                node("n:2"),
                node("n:3"),
                node("n:4"),
                node("n:5"),
                node("n:6"),
            ],
            edges: vec![
                edge("e:w900:0:f", "n:0", "n:1", EdgeKind::Shutoko),
                edge("e:w901:0:f", "n:1", "n:2", EdgeKind::Shutoko),
                edge("e:w902:0:f", "n:2", "n:3", EdgeKind::Shutoko),
                edge("e:w902:1:f", "n:3", "n:0", EdgeKind::Shutoko),
                edge("e:w903:0:f", "n:0", "n:4", EdgeKind::Shutoko),
                edge("e:w904:0:f", "n:4", "n:2", EdgeKind::Shutoko),
                edge("e:w905:0:f", "n:5", "n:0", EdgeKind::Shutoko),
                edge("e:w906:0:f", "n:2", "n:6", EdgeKind::Shutoko),
            ],
            billing_pairs: Vec::new(),
            forbidden_transitions: Vec::new(),
            ramps: Vec::new(),
            od_tariffs: Vec::new(),
        };
        let sequence = vec![
            "e:w900:0:f".to_string(),
            "e:w901:0:f".to_string(),
            "e:w902:0:f".to_string(),
            "e:w902:1:f".to_string(),
            "e:w903:0:f".to_string(),
            "e:w904:0:f".to_string(),
        ];
        let snapshot = compute_sha256(b"connector-mainline-snapshot");
        let memberships = vec![RouteMembershipIndex {
            membership_id: "route:connector:forward".into(),
            route_id: "connector".into(),
            direction: "forward".into(),
            direction_mapping_version: ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION.into(),
            segments: vec![RouteMembershipSegment {
                segment_id: "relation:connector:forward:0".into(),
                source_kind: RouteMembershipSourceKind::RelationMainline,
                source_relation_id: Some("connector".into()),
                source_snapshot_sha256: snapshot.clone(),
                binding_evidence_id: None,
                ordered_edge_ids_sha256: ordered_edge_ids_sha256(&sequence).unwrap(),
                ordered_edge_ids: sequence,
                member_indexes: Some(vec![0]),
                member_order_matches_relation: Some(true),
            }],
        }];
        let anchor = DirectedJunctionAnchor {
            anchor_kind: crate::seed::AnchorKind::DirectedJunction,
            route_id: "connector".into(),
            direction: "forward".into(),
            merge_node_id: "n:0".into(),
            branch_node_id: "n:2".into(),
            merge_terminal_edge_id: "e:w905:0:f".into(),
            branch_initial_edge_id: "e:w906:0:f".into(),
            arc_policy: crate::seed::ArcPolicy::OrdinaryLongArc,
            excluded_short_connector: crate::seed::ExcludedShortConnector {
                from_node_id: "n:2".into(),
                to_node_id: "n:0".into(),
                osm_way_id: 902,
                edge_count: 2,
                distance_meters: 200,
            },
        };
        let declared_lap = MandatoryLap {
            membership_id: "route:connector:forward".into(),
            first_edge_id: "e:w900:0:f".into(),
            last_edge_id: "e:w904:0:f".into(),
            lap_count: 1,
        };
        let error =
            validate_directed_junction_mandatory_lap(&graph, &memberships, &anchor, &declared_lap)
                .unwrap_err();
        assert!(matches!(
            error,
            RouteMembershipError::Segment(message)
                if message == "mandatory lap uses an excluded short connector"
        ));
        assert!(matches!(
            generate_route_plan_lap_v1(&graph, &memberships, &anchor).unwrap_err(),
            RouteMembershipError::Validation(message)
                if message == "no ordinary long relationMainline arc resolves from M to B"
        ));
    }

    #[test]
    fn generates_a_wrap_around_long_arc_from_a_cyclic_relation_segment() {
        let graph = Graph {
            schema_version: 2,
            release_id: "wrap".into(),
            vehicle_profile: "passenger-car-etc".into(),
            nodes: vec![
                node("n:0"),
                node("n:1"),
                node("n:2"),
                node("n:3"),
                node("n:4"),
                node("n:5"),
            ],
            edges: vec![
                edge("e:w900:0:f", "n:2", "n:3", EdgeKind::Shutoko),
                edge("e:w901:0:f", "n:3", "n:4", EdgeKind::Shutoko),
                edge("e:w901:1:f", "n:4", "n:0", EdgeKind::Shutoko),
                edge("e:w902:0:f", "n:0", "n:1", EdgeKind::Shutoko),
                edge("e:w902:1:f", "n:1", "n:2", EdgeKind::Shutoko),
                edge("e:w903:0:f", "n:3", "n:5", EdgeKind::Shutoko),
            ],
            billing_pairs: Vec::new(),
            forbidden_transitions: Vec::new(),
            ramps: Vec::new(),
            od_tariffs: Vec::new(),
        };
        let sequence = vec![
            "e:w900:0:f".to_string(),
            "e:w901:0:f".to_string(),
            "e:w901:1:f".to_string(),
            "e:w902:0:f".to_string(),
            "e:w902:1:f".to_string(),
        ];
        let segment = RouteMembershipSegment {
            segment_id: "relation:wrap:forward:0".into(),
            source_kind: RouteMembershipSourceKind::RelationMainline,
            source_relation_id: Some("wrap".into()),
            source_snapshot_sha256: compute_sha256(b"wrap-snapshot"),
            binding_evidence_id: None,
            ordered_edge_ids_sha256: ordered_edge_ids_sha256(&sequence).unwrap(),
            ordered_edge_ids: sequence,
            member_indexes: Some(vec![0]),
            member_order_matches_relation: Some(true),
        };
        let memberships = vec![RouteMembershipIndex {
            membership_id: "route:loop:forward".into(),
            route_id: "loop".into(),
            direction: "forward".into(),
            direction_mapping_version: ROUTE_MEMBERSHIP_DIRECTION_MAPPING_VERSION.into(),
            segments: vec![segment],
        }];
        let anchor = DirectedJunctionAnchor {
            anchor_kind: crate::seed::AnchorKind::DirectedJunction,
            route_id: "loop".into(),
            direction: "forward".into(),
            merge_node_id: "n:0".into(),
            branch_node_id: "n:3".into(),
            merge_terminal_edge_id: "e:w901:1:f".into(),
            branch_initial_edge_id: "e:w903:0:f".into(),
            arc_policy: crate::seed::ArcPolicy::OrdinaryLongArc,
            excluded_short_connector: crate::seed::ExcludedShortConnector {
                from_node_id: "n:3".into(),
                to_node_id: "n:0".into(),
                osm_way_id: 901,
                edge_count: 2,
                distance_meters: 200,
            },
        };
        let lap = generate_route_plan_lap_v1(&graph, &memberships, &anchor).unwrap();
        assert_eq!(lap.first_edge_id, "e:w902:0:f");
        assert_eq!(lap.last_edge_id, "e:w900:0:f");
        assert_eq!(lap.edge_ids, vec!["e:w902:0:f", "e:w902:1:f", "e:w900:0:f"]);
        assert_eq!(graph.edges.len(), 6);
        let resolved = ResolvedRouteSegment {
            resolved_segment_id: "resolved-lap".into(),
            role: RoutePlanSegmentRole::MandatoryLap,
            membership_id: "route:loop:forward".into(),
            source_segment_ids: vec![memberships[0].segments[0].segment_id.clone()],
            edge_ids_sha256: ordered_edge_ids_sha256(&lap.edge_ids).unwrap(),
            edge_ids: lap.edge_ids.clone(),
        };
        validate_resolved_route_plan_segments(&graph, &memberships, &[resolved]).unwrap();
    }

    #[test]
    fn preserves_first_exit_binding_state_and_does_not_skip_a_same_route_exit() {
        let (response, mut graph, snapshot) = synthetic();
        graph
            .edges
            .push(edge("e:w900:0:f", "n:2", "n:8", EdgeKind::Exit));
        graph.nodes.push(node("n:8"));
        graph.ramps.push(Ramp {
            id: "ramp:earlier".into(),
            facility_id: "earlier".into(),
            name: "earlier".into(),
            route: "R1".into(),
            direction: "forward".into(),
            kind: RampKind::GeneralExit,
            edge_id: "e:w900:0:f".into(),
            node_id: "n:8".into(),
            mainline_node_id: "n:2".into(),
            restrictions: Vec::new(),
        });
        let memberships = build_route_membership_indices(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot,
                relation_ids: None,
                bound_ramp_evidence: evidence(),
            },
        )
        .unwrap();
        let unresolved = find_first_exit_on_corridor_with_binding_budget(
            &graph,
            &memberships,
            "route:R1:forward",
            "n:1",
            "e:w101:0:f",
            "ramp:exit",
            EndpointSupportState::Unresolved,
            CORRIDOR_EXIT_STATE_BUDGET,
        )
        .unwrap();
        assert_eq!(
            unresolved.exact_directed_binding,
            EndpointSupportState::Unresolved
        );
        assert!(unresolved.exit.is_none());
        assert_eq!(
            unresolved.blocked_exit_edge_id.as_deref(),
            Some("e:w900:0:f")
        );
        assert_eq!(unresolved.blocked_ramp_id.as_deref(), Some("ramp:earlier"));
        assert!(find_first_exit_on_corridor_with_budget(
            &graph,
            &memberships,
            "route:R1:forward",
            "n:1",
            "e:w101:0:f",
            "ramp:exit",
            CORRIDOR_EXIT_STATE_BUDGET,
        )
        .is_err());
        let mut c1_graph = graph.clone();
        c1_graph.edges.retain(|edge| edge.id != "e:w900:0:f");
        c1_graph.ramps.retain(|ramp| ramp.id != "ramp:earlier");
        c1_graph
            .edges
            .push(edge("e:w901:0:f", "n:2", "n:8", EdgeKind::Exit));
        c1_graph.ramps.push(Ramp {
            id: "ramp:c1-exit".into(),
            facility_id: "c1-exit".into(),
            name: "C1 exit".into(),
            route: "C1".into(),
            direction: "inner".into(),
            kind: RampKind::GeneralExit,
            edge_id: "e:w901:0:f".into(),
            node_id: "n:8".into(),
            mainline_node_id: "n:2".into(),
            restrictions: Vec::new(),
        });
        let c1_exit = find_first_exit_on_corridor_with_budget(
            &c1_graph,
            &memberships,
            "route:R1:forward",
            "n:1",
            "e:w101:0:f",
            "ramp:exit",
            CORRIDOR_EXIT_STATE_BUDGET,
        )
        .unwrap();
        assert_eq!(c1_exit.exit_edge_id, "e:w202:0:f");
    }

    #[test]
    fn allows_repetition_between_segments_but_rejects_repetition_inside_one() {
        let (response, graph, snapshot) = synthetic();
        let memberships = build_route_membership_indices(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot,
                relation_ids: None,
                bound_ramp_evidence: evidence(),
            },
        )
        .unwrap();
        let source_id = memberships[0].segments[0].segment_id.clone();
        let first_edges = vec!["e:w101:0:f".to_string(), "e:w102:0:f".to_string()];
        let second_edges = vec!["e:w102:0:f".to_string(), "e:w103:0:f".to_string()];
        let segments = vec![
            ResolvedRouteSegment {
                resolved_segment_id: "entry".into(),
                role: RoutePlanSegmentRole::EntryApproach,
                membership_id: "route:R1:forward".into(),
                source_segment_ids: vec![source_id.clone()],
                edge_ids_sha256: ordered_edge_ids_sha256(&first_edges).unwrap(),
                edge_ids: first_edges,
            },
            ResolvedRouteSegment {
                resolved_segment_id: "return".into(),
                role: RoutePlanSegmentRole::ReturnCorridor,
                membership_id: "route:R1:forward".into(),
                source_segment_ids: vec![source_id],
                edge_ids_sha256: ordered_edge_ids_sha256(&second_edges).unwrap(),
                edge_ids: second_edges,
            },
        ];
        validate_resolved_route_plan_segments(&graph, &memberships, &segments).unwrap();
        let repeated = vec![
            segments[0].edge_ids[0].clone(),
            segments[0].edge_ids[0].clone(),
        ];
        let invalid = ResolvedRouteSegment {
            resolved_segment_id: "invalid".into(),
            role: RoutePlanSegmentRole::ReturnCorridor,
            membership_id: "route:R1:forward".into(),
            source_segment_ids: vec![memberships[0].segments[0].segment_id.clone()],
            edge_ids_sha256: ordered_edge_ids_sha256(&repeated).unwrap(),
            edge_ids: repeated,
        };
        assert!(validate_resolved_route_plan_segments(&graph, &memberships, &[invalid]).is_err());
    }

    #[test]
    fn rejects_binding_hash_endpoint_and_missing_way_evidence() {
        let (_response, graph, snapshot) = synthetic();
        let multi_edge_ids = vec!["e:w202:0:f".into(), "e:w203:0:f".into()];
        let segment = DirectedEndpointSegment {
            segment_id: "binding:multi:segment:0".into(),
            osm_way_ids: vec![202, 203],
            osm_node_ids: vec![4, 9, 10],
            edge_ids: multi_edge_ids.clone(),
            from_node_id: "n:4".into(),
            to_node_id: "n:10".into(),
            edge_ids_sha256: ordered_edge_ids_sha256(&multi_edge_ids).unwrap(),
        };
        let constructed = BoundRampEvidence::from_directed_segment(
            "binding:multi",
            "ramp:exit",
            "R1",
            "forward",
            &segment,
        );
        assert_eq!(constructed.from_node_id, "n:4");
        assert_eq!(constructed.to_node_id, "n:10");
        assert!(build_bound_ramp_memberships(&graph, &snapshot, &[constructed]).is_ok());
        let mut bad_hash = evidence();
        bad_hash[0].edge_ids_sha256 = compute_sha256(b"wrong");
        assert!(build_bound_ramp_memberships(&graph, &snapshot, &bad_hash).is_err());
        let mut bad_endpoint = evidence();
        bad_endpoint[0].to_node_id = "n:8".into();
        assert!(build_bound_ramp_memberships(&graph, &snapshot, &bad_endpoint).is_err());
        let mut missing_edge = evidence();
        missing_edge[1].edge_ids.pop();
        missing_edge[1].to_node_id = "n:9".into();
        missing_edge[1].edge_ids_sha256 =
            ordered_edge_ids_sha256(&missing_edge[1].edge_ids).unwrap();
        assert!(build_bound_ramp_memberships(&graph, &snapshot, &missing_edge).is_err());
    }

    #[test]
    fn builds_top_level_schema4_memberships_and_hashes() {
        let (response, graph, snapshot) = synthetic();
        let options = RouteMembershipBuildOptions {
            source_snapshot_sha256: snapshot,
            relation_ids: None,
            bound_ramp_evidence: evidence(),
        };
        let memberships = build_route_membership_indices(&response, &graph, &options).unwrap();
        assert_eq!(memberships.len(), 1);
        assert_eq!(
            memberships[0].segments.len(),
            3,
            "{:#?}",
            memberships[0].segments
        );
        assert!(memberships[0]
            .segments
            .iter()
            .any(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline));
        assert!(memberships[0]
            .segments
            .iter()
            .any(|segment| segment.source_kind == RouteMembershipSourceKind::BoundRamp));
        for segment in &memberships[0].segments {
            assert_eq!(
                segment.ordered_edge_ids_sha256,
                ordered_edge_ids_sha256(&segment.ordered_edge_ids).unwrap()
            );
        }
        let json: serde_json::Value = serde_json::from_str(
            &graph_schema_v4_to_deterministic_json(&graph, &memberships).unwrap(),
        )
        .unwrap();
        assert_eq!(json["schemaVersion"], 4);
        assert!(json["routeMemberships"].is_array());
        assert!(json.get("graph").is_none());
    }

    #[test]
    fn rejects_reverse_relation_and_unproven_ramp() {
        let (mut response, mut graph, snapshot) = synthetic();
        response.elements[5]
            .tags
            .as_mut()
            .unwrap()
            .insert("direction".into(), "outer".into());
        graph.edges.retain(|edge| !edge.id.starts_with("e:w103:"));
        assert!(build_relation_memberships(&response, &graph, &snapshot, None).is_err());

        let (mut response, graph, snapshot) = synthetic();
        let mut ramp_way = way(301, vec![4, 9]);
        ramp_way
            .tags
            .as_mut()
            .unwrap()
            .insert("highway".into(), "motorway_link".into());
        response.elements.push(ramp_way);
        response.elements[5]
            .members
            .as_mut()
            .unwrap()
            .push(member(301));
        assert!(build_relation_memberships(&response, &graph, &snapshot, None).is_err());

        let (response, graph, snapshot) = synthetic();
        let memberships = build_route_membership_indices(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot.clone(),
                relation_ids: None,
                bound_ramp_evidence: evidence(),
            },
        )
        .unwrap();
        assert!(
            validate_route_memberships(&memberships, &graph, &response, &snapshot, &[]).is_err()
        );
    }

    #[test]
    fn validates_multi_way_ramp_order_and_mandatory_lap_exclusion() {
        let (response, graph, snapshot) = synthetic();
        let options = RouteMembershipBuildOptions {
            source_snapshot_sha256: snapshot,
            relation_ids: None,
            bound_ramp_evidence: evidence(),
        };
        let memberships = build_route_membership_indices(&response, &graph, &options).unwrap();
        validate_mandatory_lap(
            &graph,
            &memberships,
            "route:R1:forward",
            &["e:w101:0:f".into(), "e:w102:0:f".into()],
            &[],
            None,
        )
        .unwrap();
        assert!(validate_mandatory_lap(
            &graph,
            &memberships,
            "route:R1:forward",
            &["e:w101:0:f".into(), "e:w102:0:f".into()],
            &["e:w102:0:f".into()],
            None,
        )
        .is_err());
        let reversed_entry_edges = vec!["e:w201:1:f".into(), "e:w201:0:f".into()];
        assert!(build_bound_ramp_memberships(
            &graph,
            &compute_sha256(b"fixture-snapshot"),
            &[BoundRampEvidence {
                binding_evidence_id: "bad".into(),
                ramp_id: "ramp:entry".into(),
                route_id: "R1".into(),
                direction: "forward".into(),
                osm_way_ids: vec![201],
                osm_node_ids: vec![8, 7],
                from_node_id: "n:8".into(),
                to_node_id: "n:7".into(),
                edge_ids_sha256: ordered_edge_ids_sha256(&reversed_entry_edges).unwrap(),
                edge_ids: reversed_entry_edges,
            }],
        )
        .is_err());
    }

    #[test]
    fn finds_only_the_first_bound_exit_on_the_relation_corridor() {
        let (response, mut graph, snapshot) = synthetic();
        graph.nodes.push(node("n:11"));
        graph
            .edges
            .push(edge("e:w301:0:f", "n:3", "n:11", EdgeKind::Exit));
        graph.ramps.push(Ramp {
            id: "ramp:other-exit".into(),
            facility_id: "other-exit".into(),
            name: "other exit".into(),
            route: "R1".into(),
            direction: "forward".into(),
            kind: RampKind::GeneralExit,
            edge_id: "e:w301:0:f".into(),
            node_id: "n:11".into(),
            mainline_node_id: "n:3".into(),
            restrictions: Vec::new(),
        });
        let memberships = build_route_membership_indices(
            &response,
            &graph,
            &RouteMembershipBuildOptions {
                source_snapshot_sha256: snapshot,
                relation_ids: None,
                bound_ramp_evidence: evidence(),
            },
        )
        .unwrap();
        let exit = find_first_exit_on_corridor(
            &graph,
            &memberships,
            "route:R1:forward",
            "n:3",
            "e:w103:0:f",
            "ramp:exit",
        )
        .unwrap();
        assert_eq!(exit.exit_edge_id, "e:w202:0:f");
        assert_eq!(
            exit.edge_ids,
            vec!["e:w103:0:f", "e:w202:0:f", "e:w203:0:f"]
        );
        assert_eq!(exit.mainline_edge_ids, vec!["e:w103:0:f"]);
        assert_eq!(exit.distance_meters, 300);
        assert!(matches!(
            find_first_exit_on_corridor_with_budget(
                &graph,
                &memberships,
                "route:R1:forward",
                "n:1",
                "e:w101:0:f",
                "ramp:exit",
                1,
            ),
            Err(RouteMembershipError::BudgetExceeded(_))
        ));
        assert!(find_first_exit_on_corridor(
            &graph,
            &memberships,
            "route:R1:forward",
            "n:2",
            "e:w102:0:f",
            "ramp:exit",
        )
        .is_err());
    }
}
