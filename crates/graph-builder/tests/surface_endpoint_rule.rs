//! Acceptance test suite for the `firstPublicRoadConnection/v1` ramp endpoint rule.
//!
//! # Verification Scope
//! 1. Directed motorway_link chain scan (exit outbound forward, entry reverse).
//! 2. 5-step tag evaluation precedence:
//!    - Step 1: Conditional restrictions (*:conditional, oneway:conditional, reversible, alternating)
//!      -> immediate fail-closed (`unresolved`).
//!    - Step 2: Access hierarchy (`motorcar` -> `motor_vehicle` -> `vehicle` -> `access`)
//!      with explicit rejected/allowed taxonomies and fail-closed on unknown values.
//!    - Step 3: Highway taxonomy whitelist/blacklist with `service=alley` only.
//!    - Step 4: Oneway continuation/arrival legality for Exit and Entry.
//!    - Step 5: Connection uniqueness (0 -> NO_GROUND_CONNECTION, 1 -> verified_bound,
//!      multiple -> MULTIPLE_GROUND_CONNECTION_CANDIDATES).
//! 3. Wire enums compliance:
//!    - EndpointSupportState: verified_bound, unsupported, unresolved.
//!    - LoopValidationStatus: declared_route_validated, unresolved, topology_only.
//!    - PairEligibilityStatus: verified_one_section_ahead, unverified, topology_only.
//!    - RoutingCapability: routable, structural_no_loop, unsupported.
//!    - TariffStatus: priced, unpriced, expired, not_applicable.
//! 4. Positive fixture: Tengenji exit resolves to 4-way, 16-edge (n:1832672162, 明治通り way 258834790,
//!    tail way 172358466 removed, verified_bound).
//! 5. Negative fixture: Shibakoen entry (way 4853801 connecting to way 40969792 with access:conditional)
//!    stops as unresolved.

use shutoko_graph_builder::osm::{OsmElement, OverpassResponse};
use shutoko_graph_builder::seed::{
    EndpointSupportState, LoopValidationStatus, PairEligibilityStatus, RoutingCapability,
    TariffStatus,
};
use shutoko_graph_builder::topology::{
    evaluate_access_hierarchy, evaluate_highway_type, find_conditional_restriction,
    find_first_public_road_connection, is_direction_continuation_legal,
    resolve_first_public_road_connection, resolve_first_public_road_connection_from_osm,
    FirstPublicRoadConnectionResolution, RampFlowDirection, SurfaceWayConnectionAudit,
    ALLOWED_ACCESS_VALUES, ALLOWED_SERVICE_SUBTAGS, ALLOWED_SURFACE_HIGHWAYS,
    FIRST_PUBLIC_ROAD_CONNECTION_RULE, REASON_CONDITIONAL_ACCESS_RESTRICTION,
    REASON_EARLY_SURFACE_CONNECTION, REASON_MULTIPLE_GROUND_CONNECTION_CANDIDATES,
    REASON_NO_GROUND_CONNECTION, REJECTED_ACCESS_VALUES, REJECTED_SERVICE_SUBTAGS,
    REJECTED_SURFACE_HIGHWAYS,
};
use std::collections::{BTreeMap, HashMap};

fn make_test_way(id: i64, nodes: Vec<i64>, tags: Vec<(&str, &str)>) -> OsmElement {
    let mut map = BTreeMap::new();
    for (k, v) in tags {
        map.insert(k.to_string(), v.to_string());
    }
    OsmElement {
        element_type: "way".to_string(),
        id,
        lat: None,
        lon: None,
        nodes: Some(nodes),
        tags: Some(map),
        members: None,
    }
}

// ---------------------------------------------------------------------------
// Step 1: Conditional restrictions (*:conditional, reversible, alternating)
// ---------------------------------------------------------------------------

#[test]
fn test_step_1_conditional_access_triggers_fail_closed() {
    let way_access_cond = make_test_way(
        1001,
        vec![10, 20],
        vec![
            ("highway", "primary"),
            ("access:conditional", "no @ (08:00-20:00)"),
        ],
    );
    assert!(find_conditional_restriction(&way_access_cond).is_some());
    let audit = shutoko_graph_builder::topology::evaluate_surface_way_for_connection(
        &way_access_cond,
        10,
        RampFlowDirection::Exit,
    );
    assert!(matches!(audit, SurfaceWayConnectionAudit::FailClosed(_)));

    let way_motorcar_cond = make_test_way(
        1002,
        vec![10, 20],
        vec![
            ("highway", "secondary"),
            ("motorcar:conditional", "no @ (07:00-09:00)"),
        ],
    );
    assert!(find_conditional_restriction(&way_motorcar_cond).is_some());

    let way_oneway_cond = make_test_way(
        1003,
        vec![10, 20],
        vec![
            ("highway", "tertiary"),
            ("oneway:conditional", "no @ (08:00-20:00)"),
        ],
    );
    assert!(find_conditional_restriction(&way_oneway_cond).is_some());

    let way_reversible = make_test_way(
        1004,
        vec![10, 20],
        vec![("highway", "primary"), ("oneway", "reversible")],
    );
    assert!(find_conditional_restriction(&way_reversible).is_some());

    let way_alternating = make_test_way(
        1005,
        vec![10, 20],
        vec![("highway", "primary"), ("oneway", "alternating")],
    );
    assert!(find_conditional_restriction(&way_alternating).is_some());

    let way_reversible_tag = make_test_way(
        1006,
        vec![10, 20],
        vec![("highway", "secondary"), ("reversible", "yes")],
    );
    assert!(find_conditional_restriction(&way_reversible_tag).is_some());
}

// ---------------------------------------------------------------------------
// Step 2: Access hierarchy (motorcar -> motor_vehicle -> vehicle -> access)
// ---------------------------------------------------------------------------

#[test]
fn test_step_2_access_hierarchy_precedence_and_taxonomies() {
    // motorcar overrides access
    let way_motorcar_override_allow = make_test_way(
        2001,
        vec![10, 20],
        vec![
            ("highway", "primary"),
            ("access", "no"),
            ("motorcar", "yes"),
        ],
    );
    assert_eq!(
        evaluate_access_hierarchy(&way_motorcar_override_allow),
        Ok(true)
    );

    let way_motorcar_override_deny = make_test_way(
        2002,
        vec![10, 20],
        vec![
            ("highway", "primary"),
            ("access", "yes"),
            ("motorcar", "private"),
        ],
    );
    assert_eq!(
        evaluate_access_hierarchy(&way_motorcar_override_deny),
        Ok(false)
    );

    // motor_vehicle overrides vehicle and access
    let way_mv_override = make_test_way(
        2003,
        vec![10, 20],
        vec![
            ("highway", "primary"),
            ("access", "no"),
            ("vehicle", "no"),
            ("motor_vehicle", "designated"),
        ],
    );
    assert_eq!(evaluate_access_hierarchy(&way_mv_override), Ok(true));

    // All rejected values are rejected
    for rejected in REJECTED_ACCESS_VALUES {
        let way = make_test_way(
            2010,
            vec![10, 20],
            vec![("highway", "primary"), ("access", rejected)],
        );
        assert_eq!(
            evaluate_access_hierarchy(&way),
            Ok(false),
            "expected {} to be rejected",
            rejected
        );
    }

    // All allowed values are allowed
    for allowed in ALLOWED_ACCESS_VALUES {
        let way = make_test_way(
            2020,
            vec![10, 20],
            vec![("highway", "primary"), ("access", allowed)],
        );
        assert_eq!(
            evaluate_access_hierarchy(&way),
            Ok(true),
            "expected {} to be allowed",
            allowed
        );
    }

    // Unspecified access defaults to allowed
    let way_unspecified = make_test_way(2030, vec![10, 20], vec![("highway", "primary")]);
    assert_eq!(evaluate_access_hierarchy(&way_unspecified), Ok(true));

    // Unknown access value triggers fail-closed
    let way_unknown = make_test_way(
        2040,
        vec![10, 20],
        vec![("highway", "primary"), ("motorcar", "prohibited_custom")],
    );
    assert!(evaluate_access_hierarchy(&way_unknown).is_err());
}

// ---------------------------------------------------------------------------
// Step 3: Highway taxonomy whitelist / blacklist & service=alley
// ---------------------------------------------------------------------------

#[test]
fn test_step_3_highway_taxonomy_whitelist_blacklist_and_service() {
    assert_eq!(ALLOWED_SERVICE_SUBTAGS, &["alley"]);
    // Allowed highways
    for hw in ALLOWED_SURFACE_HIGHWAYS {
        if *hw == "service" {
            let way = make_test_way(
                3001,
                vec![10, 20],
                vec![("highway", hw), ("service", "alley")],
            );
            assert!(evaluate_highway_type(&way), "service=alley must be allowed");
        } else {
            let way = make_test_way(3002, vec![10, 20], vec![("highway", hw)]);
            assert!(
                evaluate_highway_type(&way),
                "highway={} must be allowed",
                hw
            );
        }
    }

    // Rejected service subtags
    for sub in REJECTED_SERVICE_SUBTAGS {
        let way = make_test_way(
            3010,
            vec![10, 20],
            vec![("highway", "service"), ("service", sub)],
        );
        assert!(
            !evaluate_highway_type(&way),
            "service={} must be rejected",
            sub
        );
    }
    // Service with absent subtag is rejected
    let way_service_nosub = make_test_way(3011, vec![10, 20], vec![("highway", "service")]);
    assert!(
        !evaluate_highway_type(&way_service_nosub),
        "service without subtag must be rejected"
    );

    // Blacklist highways
    for hw in REJECTED_SURFACE_HIGHWAYS {
        let way = make_test_way(3020, vec![10, 20], vec![("highway", hw)]);
        assert!(
            !evaluate_highway_type(&way),
            "highway={} must be rejected",
            hw
        );
    }

    // area=yes is rejected
    let way_area = make_test_way(
        3030,
        vec![10, 20],
        vec![("highway", "primary"), ("area", "yes")],
    );
    assert!(
        !evaluate_highway_type(&way_area),
        "area=yes must be rejected"
    );
}

// ---------------------------------------------------------------------------
// Step 4: Oneway continuation / arrival legality
// ---------------------------------------------------------------------------

#[test]
fn test_step_4_oneway_continuation_and_arrival_semantics() {
    let way_fwd = make_test_way(
        4001,
        vec![10, 20, 30],
        vec![("highway", "primary"), ("oneway", "yes")],
    );
    // Exit flow: leaves node along surface way in forward direction
    assert_eq!(
        is_direction_continuation_legal(&way_fwd, 10, RampFlowDirection::Exit),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_fwd, 20, RampFlowDirection::Exit),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_fwd, 30, RampFlowDirection::Exit),
        Ok(false)
    ); // dead end forward

    // Entry flow: arrives at node along surface way in forward direction
    assert_eq!(
        is_direction_continuation_legal(&way_fwd, 10, RampFlowDirection::Entry),
        Ok(false)
    ); // cannot arrive from before start
    assert_eq!(
        is_direction_continuation_legal(&way_fwd, 20, RampFlowDirection::Entry),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_fwd, 30, RampFlowDirection::Entry),
        Ok(true)
    );

    let way_rev = make_test_way(
        4002,
        vec![10, 20, 30],
        vec![("highway", "primary"), ("oneway", "-1")],
    );
    // Exit flow with reverse oneway: leaves node backwards
    assert_eq!(
        is_direction_continuation_legal(&way_rev, 10, RampFlowDirection::Exit),
        Ok(false)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_rev, 20, RampFlowDirection::Exit),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_rev, 30, RampFlowDirection::Exit),
        Ok(true)
    );

    // Entry flow with reverse oneway: arrives backwards
    assert_eq!(
        is_direction_continuation_legal(&way_rev, 10, RampFlowDirection::Entry),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_rev, 20, RampFlowDirection::Entry),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_rev, 30, RampFlowDirection::Entry),
        Ok(false)
    );

    // Bidirectional oneway
    let way_bi = make_test_way(4003, vec![10, 20, 30], vec![("highway", "primary")]);
    assert_eq!(
        is_direction_continuation_legal(&way_bi, 10, RampFlowDirection::Exit),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_bi, 30, RampFlowDirection::Exit),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_bi, 10, RampFlowDirection::Entry),
        Ok(true)
    );
    assert_eq!(
        is_direction_continuation_legal(&way_bi, 30, RampFlowDirection::Entry),
        Ok(true)
    );
}

// ---------------------------------------------------------------------------
// Step 5: Connection uniqueness and chain scan resolution
// ---------------------------------------------------------------------------

#[test]
fn test_step_5_connection_uniqueness_scenarios() {
    let mut way_map = HashMap::new();
    let mut node_to_ways: HashMap<i64, Vec<i64>> = HashMap::new();

    // Ramp way: 100 -> 101 -> 102
    let ramp_way = make_test_way(
        5001,
        vec![100, 101, 102],
        vec![("highway", "motorway_link"), ("oneway", "yes")],
    );
    way_map.insert(ramp_way.id, &ramp_way);
    for nid in &[100, 101, 102] {
        node_to_ways.entry(*nid).or_default().push(ramp_way.id);
    }

    // Scenario A: No ground connection anywhere
    let res_no_conn = resolve_first_public_road_connection(
        &[100, 101, 102],
        RampFlowDirection::Exit,
        &way_map,
        &node_to_ways,
        None,
    );
    assert_eq!(res_no_conn.support_state, EndpointSupportState::Unresolved);
    assert_eq!(res_no_conn.reason_codes, vec![REASON_NO_GROUND_CONNECTION]);

    // Scenario B: Exactly 1 unique ground connection at endpoint 102
    let surface_way_1 = make_test_way(
        6001,
        vec![102, 200],
        vec![("highway", "secondary"), ("name", "Main St")],
    );
    way_map.insert(surface_way_1.id, &surface_way_1);
    node_to_ways.entry(102).or_default().push(surface_way_1.id);

    let res_unique = resolve_first_public_road_connection(
        &[100, 101, 102],
        RampFlowDirection::Exit,
        &way_map,
        &node_to_ways,
        Some(6001),
    );
    assert_eq!(
        res_unique.support_state,
        EndpointSupportState::VerifiedBound
    );
    assert_eq!(res_unique.ground_node_id, Some(102));
    assert_eq!(res_unique.ground_way_id, Some(6001));
    assert_eq!(res_unique.ground_way_name, Some("Main St".into()));
    assert!(res_unique.reason_codes.is_empty());

    // Scenario C: Multiple connections at same node 102
    let surface_way_2 = make_test_way(
        6002,
        vec![102, 201],
        vec![("highway", "tertiary"), ("name", "Side St")],
    );
    way_map.insert(surface_way_2.id, &surface_way_2);
    node_to_ways.entry(102).or_default().push(surface_way_2.id);

    let res_multi_same_node = resolve_first_public_road_connection(
        &[100, 101, 102],
        RampFlowDirection::Exit,
        &way_map,
        &node_to_ways,
        None,
    );
    assert_eq!(
        res_multi_same_node.support_state,
        EndpointSupportState::Unresolved
    );
    assert_eq!(
        res_multi_same_node.reason_codes,
        vec![REASON_MULTIPLE_GROUND_CONNECTION_CANDIDATES]
    );

    // Scenario D: Early connection along chain (node 101 connects, but chain continues to 102)
    let mut way_map_d = HashMap::new();
    let mut node_to_ways_d: HashMap<i64, Vec<i64>> = HashMap::new();
    way_map_d.insert(ramp_way.id, &ramp_way);
    for nid in &[100, 101, 102] {
        node_to_ways_d.entry(*nid).or_default().push(ramp_way.id);
    }
    let early_way = make_test_way(
        6003,
        vec![101, 205],
        vec![("highway", "primary"), ("name", "Early Ave")],
    );
    way_map_d.insert(early_way.id, &early_way);
    node_to_ways_d.entry(101).or_default().push(early_way.id);

    let res_early = resolve_first_public_road_connection(
        &[100, 101, 102],
        RampFlowDirection::Exit,
        &way_map_d,
        &node_to_ways_d,
        None,
    );
    assert_eq!(res_early.support_state, EndpointSupportState::Unresolved);
    assert!(res_early
        .reason_codes
        .contains(&REASON_EARLY_SURFACE_CONNECTION.to_string()));
    assert!(res_early
        .reason_codes
        .contains(&REASON_MULTIPLE_GROUND_CONNECTION_CANDIDATES.to_string()));
}

// ---------------------------------------------------------------------------
// Real Fixture Positive Test: Tengenji Exit (4-way, 16-edge -> verified_bound)
// ---------------------------------------------------------------------------

#[test]
fn test_tengenji_exit_positive_fixture_resolves_verified_bound() {
    let raw_osm = include_str!("../../../fixtures/osm/shutoko-all.json");
    let osm_resp: OverpassResponse = serde_json::from_str(raw_osm).unwrap();

    // 4-way candidate defined in design-fix.json tengenjiResolution
    let tengenji_ways: Vec<i64> = vec![172358461, 422023171, 931759044, 172358460];
    let mut chain_node_ids = Vec::new();
    let mut way_map = HashMap::new();
    let mut node_to_ways: HashMap<i64, Vec<i64>> = HashMap::new();

    for elem in &osm_resp.elements {
        if elem.is_way() {
            way_map.insert(elem.id, elem);
            if let Some(nodes) = &elem.nodes {
                for nid in nodes {
                    node_to_ways.entry(*nid).or_default().push(elem.id);
                }
            }
        }
    }

    // Assemble ordered nodes and edge IDs
    let mut edge_ids = Vec::new();
    for (idx, wid) in tengenji_ways.iter().enumerate() {
        let way = way_map.get(wid).expect("tengenji way must exist");
        let nodes = way.nodes.as_ref().unwrap();
        if idx == 0 {
            chain_node_ids.extend(nodes.iter().copied());
        } else {
            chain_node_ids.extend(nodes.iter().skip(1).copied());
        }
        for edge_idx in 0..nodes.len() - 1 {
            edge_ids.push(format!("e:w{wid}:{edge_idx}:f"));
        }
    }

    // Verify 4-way, 16-edge topology metrics
    assert_eq!(
        tengenji_ways.len(),
        4,
        "tengenji resolution must have exactly 4 ways"
    );
    assert_eq!(
        edge_ids.len(),
        16,
        "tengenji resolution must have exactly 16 edges"
    );
    assert_eq!(
        chain_node_ids.first(),
        Some(&252175582),
        "fromNodeId must be n:252175582"
    );
    assert_eq!(
        chain_node_ids.last(),
        Some(&1832672162),
        "toNodeId must be n:1832672162"
    );

    // Run firstPublicRoadConnection/v1 resolution
    let resolution = resolve_first_public_road_connection(
        &chain_node_ids,
        RampFlowDirection::Exit,
        &way_map,
        &node_to_ways,
        Some(258834790), // 明治通り
    );

    assert_eq!(resolution.rule, FIRST_PUBLIC_ROAD_CONNECTION_RULE);
    assert_eq!(
        resolution.support_state,
        EndpointSupportState::VerifiedBound,
        "tengenji 4-way candidate must resolve to verified_bound"
    );
    assert_eq!(resolution.ground_node_id, Some(1832672162));
    assert_eq!(resolution.ground_way_id, Some(258834790));
    assert_eq!(resolution.ground_way_name.as_deref(), Some("明治通り"));
    assert!(
        resolution.reason_codes.is_empty(),
        "verified candidate must have no reason codes"
    );

    // Also test helper resolve_first_public_road_connection_from_osm
    let res_from_osm: FirstPublicRoadConnectionResolution =
        resolve_first_public_road_connection_from_osm(
            &chain_node_ids,
            RampFlowDirection::Exit,
            &osm_resp,
            Some(258834790),
        );
    assert_eq!(
        res_from_osm.support_state,
        EndpointSupportState::VerifiedBound
    );
    assert_eq!(res_from_osm.ground_node_id, Some(1832672162));
    assert_eq!(res_from_osm.ground_way_id, Some(258834790));

    // Discover first connection along unpruned 5-way chain (including tail 172358466)
    let tail_way = way_map.get(&172358466).unwrap();
    let mut unpruned_chain = chain_node_ids.clone();
    unpruned_chain.extend(tail_way.nodes.as_ref().unwrap().iter().skip(1).copied());
    assert_eq!(unpruned_chain.last(), Some(&1832672205));

    let discovery = find_first_public_road_connection(
        &unpruned_chain,
        RampFlowDirection::Exit,
        &way_map,
        &node_to_ways,
    );
    assert_eq!(discovery.support_state, EndpointSupportState::VerifiedBound);
    assert_eq!(
        discovery.ground_node_id,
        Some(1832672162),
        "first connection must stop at 1832672162, removing tail way 172358466"
    );
    assert_eq!(discovery.ground_way_id, Some(258834790));
}

// ---------------------------------------------------------------------------
// Real Fixture Negative Test: Shibakoen Entry (way 40969792 access:conditional -> unresolved)
// ---------------------------------------------------------------------------

#[test]
fn test_shibakoen_entry_negative_fixture_stops_unresolved() {
    let raw_osm = include_str!("../../../fixtures/osm/shutoko-all.json");
    let osm_resp: OverpassResponse = serde_json::from_str(raw_osm).unwrap();

    // Shibakoen entry ramp way 4853801
    let mut way_map = HashMap::new();
    let mut node_to_ways: HashMap<i64, Vec<i64>> = HashMap::new();

    for elem in &osm_resp.elements {
        if elem.is_way() {
            way_map.insert(elem.id, elem);
            if let Some(nodes) = &elem.nodes {
                for nid in nodes {
                    node_to_ways.entry(*nid).or_default().push(elem.id);
                }
            }
        }
    }

    let shibakoen_way = way_map.get(&4853801).expect("way 4853801 must exist");
    assert_eq!(shibakoen_way.get_tag("name"), Some("芝公園入口"));
    let chain_nodes = shibakoen_way.nodes.as_ref().unwrap();
    assert_eq!(chain_nodes[0], 940044988);

    // Verify connecting way 40969792 has access:conditional
    let connecting_way = way_map.get(&40969792).expect("way 40969792 must exist");
    assert_eq!(
        connecting_way.get_tag("access:conditional"),
        Some("no @ (08:00-20:00)")
    );
    assert!(find_conditional_restriction(connecting_way).is_some());

    // Resolve first public road connection for Shibakoen entry
    let resolution = resolve_first_public_road_connection(
        chain_nodes,
        RampFlowDirection::Entry,
        &way_map,
        &node_to_ways,
        Some(40969792),
    );

    assert_eq!(
        resolution.support_state,
        EndpointSupportState::Unresolved,
        "shibakoen entry must stop fail-closed as unresolved"
    );
    assert_eq!(
        resolution.reason_codes,
        vec![REASON_CONDITIONAL_ACCESS_RESTRICTION],
        "must trigger CONDITIONAL_ACCESS_RESTRICTION reason code"
    );
    assert!(
        resolution
            .notes
            .iter()
            .any(|n| n.contains("access:conditional")),
        "diagnostic notes must capture the access:conditional tag"
    );
}

// ---------------------------------------------------------------------------
// Wire Enums Compliance Test
// ---------------------------------------------------------------------------

#[test]
fn test_wire_enums_exact_wire_representation() {
    // EndpointSupportState
    assert_eq!(
        serde_json::to_string(&EndpointSupportState::VerifiedBound).unwrap(),
        "\"verified_bound\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointSupportState::Unsupported).unwrap(),
        "\"unsupported\""
    );
    assert_eq!(
        serde_json::to_string(&EndpointSupportState::Unresolved).unwrap(),
        "\"unresolved\""
    );

    // RoutingCapability
    assert_eq!(
        serde_json::to_string(&RoutingCapability::Routable).unwrap(),
        "\"routable\""
    );
    assert_eq!(
        serde_json::to_string(&RoutingCapability::StructuralNoLoop).unwrap(),
        "\"structural_no_loop\""
    );
    assert_eq!(
        serde_json::to_string(&RoutingCapability::Unsupported).unwrap(),
        "\"unsupported\""
    );

    // PairEligibilityStatus
    assert_eq!(
        serde_json::to_string(&PairEligibilityStatus::VerifiedOneSectionAhead).unwrap(),
        "\"verified_one_section_ahead\""
    );
    assert_eq!(
        serde_json::to_string(&PairEligibilityStatus::Unverified).unwrap(),
        "\"unverified\""
    );
    assert_eq!(
        serde_json::to_string(&PairEligibilityStatus::TopologyOnly).unwrap(),
        "\"topology_only\""
    );

    // LoopValidationStatus
    assert_eq!(
        serde_json::to_string(&LoopValidationStatus::DeclaredRouteValidated).unwrap(),
        "\"declared_route_validated\""
    );
    assert_eq!(
        serde_json::to_string(&LoopValidationStatus::Unresolved).unwrap(),
        "\"unresolved\""
    );
    assert_eq!(
        serde_json::to_string(&LoopValidationStatus::TopologyOnly).unwrap(),
        "\"topology_only\""
    );

    // TariffStatus
    assert_eq!(
        serde_json::to_string(&TariffStatus::Priced).unwrap(),
        "\"priced\""
    );
    assert_eq!(
        serde_json::to_string(&TariffStatus::Unpriced).unwrap(),
        "\"unpriced\""
    );
    assert_eq!(
        serde_json::to_string(&TariffStatus::Expired).unwrap(),
        "\"expired\""
    );
    assert_eq!(
        serde_json::to_string(&TariffStatus::NotApplicable).unwrap(),
        "\"not_applicable\""
    );
}
