use crate::inventory::{
    CanonicalRampInventoryItem, OsmRampBinding, OsmRampBindingsFile, RampInventoryFile,
};
use crate::model::{BillingPair, Edge, EdgeKind, Graph, Node, OdTariff, Ramp, RampKind};
use crate::osm::{OsmElement, OsmMember, OverpassResponse};
use crate::seed::{DirectedEndpointSegment, DirectedJunctionAnchor, MandatoryLap};
use crate::validate::contains_forbidden_transition;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteMembershipSourceKind {
    RelationMainline,
    BoundRamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteMembershipIndex {
    pub membership_id: String,
    pub route_id: String,
    pub direction: String,
    pub direction_mapping_version: String,
    pub segments: Vec<RouteMembershipSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteMembershipSegment {
    pub segment_id: String,
    pub source_kind: RouteMembershipSourceKind,
    pub source_relation_id: Option<String>,
    pub source_snapshot_sha256: String,
    pub binding_evidence_id: Option<String>,
    pub ordered_edge_ids: Vec<String>,
    pub ordered_edge_ids_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundRampEvidence {
    pub binding_evidence_id: String,
    pub ramp_id: String,
    pub route_id: String,
    pub direction: String,
    pub osm_way_ids: Vec<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphSchemaV4 {
    pub schema_version: u32,
    pub release_id: String,
    pub vehicle_profile: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub billing_pairs: Vec<BillingPair>,
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
        Self {
            schema_version: 4,
            release_id: graph.release_id.clone(),
            vehicle_profile: graph.vehicle_profile.clone(),
            nodes: graph.nodes.clone(),
            edges: graph.edges.clone(),
            billing_pairs: graph.billing_pairs.clone(),
            forbidden_transitions: graph.forbidden_transitions.clone(),
            ramps: graph.ramps.clone(),
            od_tariffs: graph.od_tariffs.clone(),
            route_memberships,
        }
    }
}

pub fn graph_schema_v4_to_deterministic_json(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
) -> Result<String, serde_json::Error> {
    let document = GraphSchemaV4::from_graph(graph, route_memberships.to_vec());
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

#[derive(Debug)]
struct RelationMemberEdges {
    way_id: i64,
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
        let ordered_edge_ids =
            map_relation_way_from_response(relation, member, direction, response, &edges)?;
        let first_id = ordered_edge_ids.first().ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} way {} produced no graph edges",
                relation.id, member.ref_id
            ))
        })?;
        let last_id = ordered_edge_ids.last().ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} way {} produced no graph edges",
                relation.id, member.ref_id
            ))
        })?;
        let first = edges.get(first_id.as_str()).ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} produced an unknown edge {}",
                relation.id, first_id
            ))
        })?;
        let last = edges.get(last_id.as_str()).ok_or_else(|| {
            RouteMembershipError::Relation(format!(
                "relation {} produced an unknown edge {}",
                relation.id, last_id
            ))
        })?;
        mapped_members.push(RelationMemberEdges {
            way_id: member.ref_id,
            from_node_id: first.from.clone(),
            to_node_id: last.to.clone(),
            ordered_edge_ids,
        });
    }
    if mapped_members.is_empty() {
        return Ok(Vec::new());
    }

    mapped_members.sort_by_key(|member| member.way_id);
    let mut outgoing: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut incoming: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, member) in mapped_members.iter().enumerate() {
        outgoing
            .entry(member.from_node_id.clone())
            .or_default()
            .push(index);
        incoming
            .entry(member.to_node_id.clone())
            .or_default()
            .push(index);
    }
    for candidates in outgoing.values_mut() {
        candidates.sort_by_key(|index| mapped_members[*index].way_id);
    }
    for candidates in incoming.values_mut() {
        candidates.sort_by_key(|index| mapped_members[*index].way_id);
    }

    let mut used = vec![false; mapped_members.len()];
    let mut paths: Vec<Vec<String>> = Vec::new();
    while used.iter().any(|used| !used) {
        let mut roots = (0..mapped_members.len())
            .filter(|index| {
                !used[*index]
                    && !incoming
                        .get(&mapped_members[*index].from_node_id)
                        .is_some_and(|preds| preds.iter().any(|pred| !used[*pred]))
            })
            .collect::<Vec<_>>();
        if roots.is_empty() {
            roots.push(
                used.iter()
                    .position(|used| !used)
                    .expect("loop requires an unused member"),
            );
        }
        roots.sort_by_key(|index| mapped_members[*index].way_id);
        for root in roots {
            if used[root] {
                continue;
            }
            used[root] = true;
            let mut path_member_indices = vec![root];
            let mut visited_nodes = HashSet::from([mapped_members[root].from_node_id.clone()]);
            let mut current_node = mapped_members[root].to_node_id.clone();
            while visited_nodes.insert(current_node.clone()) {
                let Some(next) = outgoing.get(&current_node).and_then(|candidates| {
                    candidates
                        .iter()
                        .copied()
                        .find(|candidate| !used[*candidate])
                }) else {
                    break;
                };
                used[next] = true;
                path_member_indices.push(next);
                current_node = mapped_members[next].to_node_id.clone();
            }
            let ordered_edge_ids = path_member_indices
                .into_iter()
                .flat_map(|index| mapped_members[index].ordered_edge_ids.clone())
                .collect::<Vec<_>>();
            validate_ordered_edges(graph, &ordered_edge_ids, "assembled relation member path")?;
            paths.push(ordered_edge_ids);
        }
    }
    paths.sort_by_key(|path| path[0].clone());

    let mut result = Vec::with_capacity(paths.len());
    for (index, ordered_edge_ids) in paths.into_iter().enumerate() {
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

fn map_relation_way_from_response(
    relation: &OsmElement,
    member: &OsmMember,
    direction: &str,
    response: &OverpassResponse,
    edges: &HashMap<&str, &Edge>,
) -> Result<Vec<String>, RouteMembershipError> {
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
    let requested_reverse = direction_is_reverse(direction);
    let reverse = if way_has_directed_edges(edges, member.ref_id, nodes, requested_reverse) {
        requested_reverse
    } else {
        !requested_reverse
    };
    let mut ordered = Vec::with_capacity(nodes.len() - 1);
    for index in 0..nodes.len() - 1 {
        let (from, to) = if reverse {
            (nodes[index + 1], nodes[index])
        } else {
            (nodes[index], nodes[index + 1])
        };
        let from = format!("n:{}", from);
        let to = format!("n:{}", to);
        let candidates: Vec<&Edge> = edges
            .values()
            .copied()
            .filter(|edge| {
                edge.kind == EdgeKind::Shutoko
                    && edge_way_id(edge) == Some(member.ref_id)
                    && edge.from == from
                    && edge.to == to
            })
            .collect();
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
        ordered.push(candidates[0].id.clone());
    }
    Ok(ordered)
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
                    if segment.source_relation_id.is_none()
                        || segment.binding_evidence_id.is_some()
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
                        || segment.binding_evidence_id.is_none()
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
    if edge_ids.iter().collect::<HashSet<_>>().len() != edge_ids.len() {
        return Err(RouteMembershipError::Segment(
            "mandatory lap repeats an edge".into(),
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
                graph
                    .edges
                    .iter()
                    .find(|edge| edge.id == *edge_id)
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
        let source = &segment.ordered_edge_ids;
        if edge_ids.len() <= source.len()
            && source
                .windows(edge_ids.len())
                .any(|window| window == edge_ids)
        {
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
    if merge_terminal.to != anchor.merge_node_id || branch_initial.from != anchor.branch_node_id {
        return Err(RouteMembershipError::Validation(
            "directed junction M/B boundaries do not match their terminal edges".into(),
        ));
    }

    let mut matches = Vec::new();
    for segment in membership
        .segments
        .iter()
        .filter(|segment| segment.source_kind == RouteMembershipSourceKind::RelationMainline)
    {
        let first = segment
            .ordered_edge_ids
            .iter()
            .position(|edge_id| edge_id == &mandatory_lap.first_edge_id);
        let last = segment
            .ordered_edge_ids
            .iter()
            .position(|edge_id| edge_id == &mandatory_lap.last_edge_id);
        if let (Some(first), Some(last)) = (first, last) {
            if first < last {
                matches.push((segment, first, last));
            }
        }
    }
    if matches.len() != 1 {
        return Err(RouteMembershipError::Validation(format!(
            "directed mandatory lap boundaries must occur once in forward relationMainline order (matches={})",
            matches.len()
        )));
    }
    let (segment, first, last) = matches[0];
    let lap_edge_ids = segment.ordered_edge_ids[first..=last].to_vec();
    validate_mandatory_lap(
        graph,
        route_memberships,
        &mandatory_lap.membership_id,
        &lap_edge_ids,
        &[],
        Some(anchor.excluded_short_connector.osm_way_id),
    )?;
    let first_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == segment.ordered_edge_ids[first])
        .ok_or_else(|| {
            RouteMembershipError::Validation("mandatory lap first edge is absent".into())
        })?;
    let last_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == segment.ordered_edge_ids[last])
        .ok_or_else(|| {
            RouteMembershipError::Validation("mandatory lap last edge is absent".into())
        })?;
    if first_edge.from != anchor.merge_node_id || last_edge.to != anchor.branch_node_id {
        return Err(RouteMembershipError::Validation(
            "mandatory lap does not run from M to B on the declared arm".into(),
        ));
    }

    let connector_candidates = graph
        .edges
        .iter()
        .filter(|edge| {
            edge_way_id(edge) == Some(anchor.excluded_short_connector.osm_way_id)
                && edge.from == anchor.excluded_short_connector.from_node_id
                && edge.to == anchor.excluded_short_connector.to_node_id
        })
        .collect::<Vec<_>>();
    let connector_distance = connector_candidates
        .iter()
        .map(|edge| edge.distance_meters)
        .sum::<u64>();
    if connector_candidates.len() != anchor.excluded_short_connector.edge_count as usize
        || connector_distance != anchor.excluded_short_connector.distance_meters
    {
        return Err(RouteMembershipError::Validation(format!(
            "excluded short connector evidence does not match graph (edges={}, distance={})",
            connector_candidates.len(),
            connector_distance
        )));
    }
    let mut current_node = anchor.excluded_short_connector.from_node_id.clone();
    let mut visited = HashSet::new();
    while let Some(edge) = connector_candidates
        .iter()
        .find(|edge| edge.from == current_node)
    {
        if !visited.insert(edge.id.as_str()) {
            return Err(RouteMembershipError::Validation(
                "excluded short connector is not an acyclic directed path".into(),
            ));
        }
        if lap_edge_ids.iter().any(|edge_id| edge_id == &edge.id) {
            return Err(RouteMembershipError::Segment(
                "mandatory lap uses an excluded short connector".into(),
            ));
        }
        current_node = edge.to.clone();
    }
    if current_node != anchor.excluded_short_connector.to_node_id {
        return Err(RouteMembershipError::Validation(
            "excluded short connector is disconnected".into(),
        ));
    }
    Ok(())
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

pub fn find_first_exit_on_corridor_with_budget(
    graph: &Graph,
    route_memberships: &[RouteMembershipIndex],
    membership_id: &str,
    start_node_id: &str,
    initial_edge_id: &str,
    expected_ramp_id: &str,
    state_budget: usize,
) -> Result<CorridorExit, RouteMembershipError> {
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
    let membership = route_memberships
        .iter()
        .find(|membership| membership.membership_id == membership_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!("unknown route membership {}", membership_id))
        })?;
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
    let sequences = relation_sequences(membership);
    let matching_sequences: Vec<&&RouteMembershipSegment> = sequences
        .iter()
        .filter(|segment| {
            segment
                .ordered_edge_ids
                .iter()
                .any(|id| id == initial_edge_id)
        })
        .collect();
    if matching_sequences.len() != 1 {
        return Err(RouteMembershipError::Validation(format!(
            "initial edge {} must occur in exactly one relationMainline segment of {}",
            initial_edge_id, membership_id
        )));
    }
    let sequence = &matching_sequences[0].ordered_edge_ids;
    let start_position = sequence
        .iter()
        .position(|id| id == initial_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(
                "initial edge is absent from its relation segment".into(),
            )
        })?;
    let first_edge = graph
        .edges
        .iter()
        .find(|edge| edge.id == *initial_edge_id)
        .ok_or_else(|| {
            RouteMembershipError::Validation(format!(
                "initial edge {} is absent from graph",
                initial_edge_id
            ))
        })?;
    if first_edge.kind != EdgeKind::Shutoko || first_edge.from != start_node_id {
        return Err(RouteMembershipError::Validation(format!(
            "initial edge {} is not a mainline edge leaving {}",
            initial_edge_id, start_node_id
        )));
    }
    let mut position = start_position;
    let mut path = Vec::new();
    let mut mainline = Vec::new();
    let mut distance = 0u64;
    let mut states = 0usize;
    loop {
        if states >= state_budget {
            return Err(RouteMembershipError::BudgetExceeded(format!(
                "corridor search exceeded {} states before finding a verified exit",
                state_budget
            )));
        }
        states += 1;
        let edge_id = &sequence[position];
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
        if let Some(previous_id) = path.last() {
            let previous = graph
                .edges
                .iter()
                .find(|candidate| candidate.id == *previous_id)
                .ok_or_else(|| {
                    RouteMembershipError::Validation(format!(
                        "corridor edge {} is absent from graph",
                        previous_id
                    ))
                })?;
            if previous.to != edge.from {
                return Err(RouteMembershipError::Validation(
                    "corridor relation segment has a node discontinuity".into(),
                ));
            }
        }
        path.push(edge.id.clone());
        mainline.push(edge.id.clone());
        distance = distance.saturating_add(edge.distance_meters);

        let mut exit_edges = graph
            .edges
            .iter()
            .filter(|candidate| candidate.kind == EdgeKind::Exit && candidate.from == edge.to)
            .collect::<Vec<_>>();
        if !exit_edges.is_empty() {
            exit_edges.sort_by(|left, right| left.id.cmp(&right.id));
            let first_exit = exit_edges[0];
            let (bound_segment, ramp) = bound_ramp_segment_for_exit(
                graph,
                route_memberships,
                membership,
                expected_ramp_id,
                &first_exit.id,
            )?;
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
            return Ok(CorridorExit {
                distance_meters: distance,
                exit_edge_id: ramp_edges[0].id.clone(),
                ramp_id: ramp.id.clone(),
                edge_ids: full_path,
                mainline_edge_ids: mainline,
            });
        }

        if position + 1 >= sequence.len() {
            return Err(RouteMembershipError::ExitNotFound(format!(
                "corridor ended without a bound general Exit for {}",
                expected_ramp_id
            )));
        }
        let next = &sequence[position + 1];
        let next_edge = graph
            .edges
            .iter()
            .find(|candidate| candidate.id == *next)
            .ok_or_else(|| {
                RouteMembershipError::Validation(format!(
                    "corridor edge {} is absent from graph",
                    next
                ))
            })?;
        if edge.to != next_edge.from {
            return Err(RouteMembershipError::Validation(
                "corridor cannot continue to the next relation edge".into(),
            ));
        }
        position += 1;
    }
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
                from_node_id: "n:4".into(),
                to_node_id: "n:10".into(),
                edge_ids_sha256: ordered_edge_ids_sha256(&exit_edge_ids).unwrap(),
                edge_ids: exit_edge_ids,
            },
        ]
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
    fn filters_conflicting_route_links_and_keeps_c1_arms_separate() {
        let (mut response, graph, snapshot) = synthetic();
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
        let mut wrong_connector = anchor;
        wrong_connector.excluded_short_connector.osm_way_id = 1000;
        assert!(validate_directed_junction_mandatory_lap(
            &graph,
            &memberships,
            &wrong_connector,
            &lap
        )
        .is_err());
    }

    #[test]
    fn rejects_binding_hash_endpoint_and_missing_way_evidence() {
        let (_response, graph, snapshot) = synthetic();
        let multi_edge_ids = vec!["e:w202:0:f".into(), "e:w203:0:f".into()];
        let segment = DirectedEndpointSegment {
            segment_id: "binding:multi:segment:0".into(),
            osm_way_ids: vec![202, 203],
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
