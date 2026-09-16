/// Smoke test: route search against the full 23-ward graph (602k nodes / 1.2M edges).
///
/// This test is **ignored by default** so it never runs in ordinary CI.
/// To run it, set LARGE_GRAPH_PATH to the graph JSON file and pass --ignored:
///
///   LARGE_GRAPH_PATH=<path>/graph-23ku.json \
///     cargo test -p shutoko-routing-core --release --test large_graph_smoke \
///     -- --ignored --nocapture
///
/// The test simply prints observations.  Hard assertions are intentionally
/// minimal: we only fail when a search() call returns an Err, or when the
/// number of candidates is inconsistent with a clearly non-empty graph.
use shutoko_routing_core::{prepare, search_prepared, Graph, SearchLimits, SearchRequest};
use std::time::Instant;

/// (pair_id, entry_edge_from_node_id)
///
/// These were extracted from the large graph programmatically.  The entry
/// node is the `from` node of the first edge in `entryToAnchorEdgeIds`.
const PAIRS: &[(&str, &str)] = &[
    ("bp:c1-inner:daikancho-kasumigaseki", "n:1866081909"), // issue #25
    ("bp:c1-inner:kasumigaseki-shibakoen", "n:573233927"),
    ("bp:c1-inner:shibakoen-shiodome", "n:254367256"),
    ("bp:c1-inner:takaracho-kandabashi", "n:1105125663"),
    ("bp:c1-outer:ginza-shibakoen", "n:835996316"), // issue #25
    ("bp:c1-outer:kandabashi-takaracho", "n:1070862943"),
    ("bp:c1-outer:kasumigaseki-daikancho", "n:577255402"), // issue #25
    ("bp:c1-outer:shibakoen-iikura", "n:940044988"),
];

/// Pairs that were isolated/disconnected in the small (C1-area-only) fixture
/// graph due to OSM boundary clipping (issue #25).  The large 23-ward graph
/// should have enough coverage to reconnect them.
const ISSUE_25_PAIRS: &[&str] = &[
    "bp:c1-outer:ginza-shibakoen",
    "bp:c1-outer:kasumigaseki-daikancho",
    "bp:c1-inner:daikancho-kasumigaseki",
];

#[test]
#[ignore = "requires LARGE_GRAPH_PATH; run manually with --ignored --nocapture"]
fn large_graph_smoke_search() {
    // ------------------------------------------------------------------
    // 1. Resolve graph path.  If not set, skip silently (CI won't fail).
    // ------------------------------------------------------------------
    let path = match std::env::var("LARGE_GRAPH_PATH") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[large_graph_smoke] LARGE_GRAPH_PATH not set – skipping");
            return;
        }
    };

    // ------------------------------------------------------------------
    // 2. Load the graph from disk.
    // ------------------------------------------------------------------
    eprintln!("[large_graph_smoke] loading graph from {}", path);
    let load_start = Instant::now();
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("cannot open {}: {}", path, e));
    let reader = std::io::BufReader::new(file);
    let graph: Graph =
        serde_json::from_reader(reader).unwrap_or_else(|e| panic!("JSON parse failed: {}", e));
    let load_elapsed = load_start.elapsed();
    eprintln!(
        "[large_graph_smoke] graph loaded in {:.2?}  nodes={} edges={} billing_pairs={}",
        load_elapsed,
        graph.nodes.len(),
        graph.edges.len(),
        graph.billing_pairs.len(),
    );

    let release_id = graph.release_id.clone();
    let vehicle_profile = graph.vehicle_profile.clone();

    // ------------------------------------------------------------------
    // 3. Build PreparedGraph (index construction).
    // ------------------------------------------------------------------
    let limits = SearchLimits::default();
    eprintln!(
        "[large_graph_smoke] max_graph_nodes={} max_graph_edges={} max_access_entries={}",
        limits.max_graph_nodes, limits.max_graph_edges, limits.max_access_entries,
    );
    let prepare_start = Instant::now();
    let pg = prepare(graph, &limits).unwrap_or_else(|e| panic!("prepare() failed: {}", e));
    let prepare_elapsed = prepare_start.elapsed();
    eprintln!("[large_graph_smoke] prepare() took {:.2?}", prepare_elapsed,);

    // ------------------------------------------------------------------
    // 4. Search each billing pair (cold run + warm run).
    // ------------------------------------------------------------------
    eprintln!("\n[large_graph_smoke] === SEARCH RESULTS ===");
    eprintln!(
        "{:<50}  {:>10}  {:>4}  {:>8}  {:>8}  {:>8}  {:>8}  own",
        "pair_id", "status", "cand", "expand", "1st_ms", "2nd_ms", "delta_ms"
    );

    let mut issue25_results: Vec<(&str, String, usize)> = Vec::new();

    for &(pair_id, origin_node_id) in PAIRS {
        let make_request = |req_id: &str| SearchRequest {
            request_id: req_id.to_owned(),
            release_id: release_id.clone(),
            origin_node_id: Some(origin_node_id.to_owned()),
            origin: None,
            entry_ramp_id: None,
            exit_ramp_id: None,
            min_minutes: 15,
            max_minutes: 60,
            vehicle_profile: vehicle_profile.clone(),
            pricing_at: "2026-09-10T00:00:00Z".to_owned(),
        };

        // First search (cold – no cache hit for this pair's loops yet).
        let t1 = Instant::now();
        let res1 = search_prepared(&pg, &make_request(&format!("smoke-1-{}", pair_id)))
            .unwrap_or_else(|e| panic!("search_prepared() error for {}: {}", pair_id, e));
        let ms1 = t1.elapsed().as_secs_f64() * 1000.0;

        // Second search on the same PreparedGraph (warm – reachable_cache populated).
        let t2 = Instant::now();
        let res2 = search_prepared(&pg, &make_request(&format!("smoke-2-{}", pair_id)))
            .unwrap_or_else(|e| panic!("search_prepared() error (2nd) for {}: {}", pair_id, e));
        let ms2 = t2.elapsed().as_secs_f64() * 1000.0;

        assert_eq!(
            res1.status, res2.status,
            "second search must return same status as first for {}",
            pair_id
        );

        let own_pair = res1
            .candidates
            .iter()
            .any(|c| c.toll.billing_pair_id == pair_id);

        eprintln!(
            "{:<50}  {:>10}  {:>4}  {:>8}  {:>8.1}  {:>8.1}  {:>8.1}  {}",
            pair_id,
            res1.status,
            res1.candidates.len(),
            res1.expanded_states,
            ms1,
            ms2,
            ms1 - ms2,
            if own_pair { "YES" } else { "no" },
        );

        if ISSUE_25_PAIRS.contains(&pair_id) {
            issue25_results.push((pair_id, res1.status.clone(), res1.candidates.len()));
        }

        // Hard assertion: every tested pair is a verified pair that must produce legal candidates
        assert_eq!(
            res1.status, "ok",
            "search status must be ok for verified pair {}",
            pair_id
        );
        assert!(
            !res1.candidates.is_empty(),
            "expected at least one candidate for verified pair {}",
            pair_id
        );
    }

    // ------------------------------------------------------------------
    // 5. Summary for issue #25 pairs.
    // ------------------------------------------------------------------
    eprintln!("\n[large_graph_smoke] === ISSUE #25 RECONNECTION CHECK ===");
    eprintln!(
        "{:<50}  {:>10}  {:>4}  resolved",
        "pair_id", "status", "cand"
    );
    for (pair_id, status, cands) in &issue25_results {
        let resolved = *status == "ok";
        eprintln!(
            "{:<50}  {:>10}  {:>4}  {}",
            pair_id,
            status,
            cands,
            if resolved {
                "YES (issue #25 fixed)"
            } else {
                "no (still disconnected)"
            }
        );
    }

    // ------------------------------------------------------------------
    // 6. Performance summary.
    // ------------------------------------------------------------------
    eprintln!("\n[large_graph_smoke] === PERFORMANCE SUMMARY ===");
    eprintln!("  load time          : {:.2?}", load_elapsed);
    eprintln!("  prepare() time     : {:.2?}", prepare_elapsed);
    eprintln!("  performance target : search p95 <= 2000 ms, peak memory <= 128 MiB");
    eprintln!("[large_graph_smoke] done");
}

/// Same smoke test but with max_expanded_states bumped to 1_000_000 to
/// determine whether candidate results become available with a larger budget.
///
/// Run with:
///   LARGE_GRAPH_PATH=<path> cargo test -p shutoko-routing-core --release \
///     --test large_graph_smoke -- large_graph_smoke_search_extended_limits \
///     --ignored --nocapture
#[test]
#[ignore = "requires LARGE_GRAPH_PATH; run manually with --ignored --nocapture"]
fn large_graph_smoke_search_extended_limits() {
    let path = match std::env::var("LARGE_GRAPH_PATH") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("[smoke_ext] LARGE_GRAPH_PATH not set – skipping");
            return;
        }
    };

    eprintln!("[smoke_ext] loading graph…");
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("open: {}", e));
    let graph: Graph = serde_json::from_reader(std::io::BufReader::new(file))
        .unwrap_or_else(|e| panic!("parse: {}", e));
    let release_id = graph.release_id.clone();
    let vehicle_profile = graph.vehicle_profile.clone();

    let limits = SearchLimits {
        max_expanded_states: 1_000_000, // maximum allowed by validation
        ..SearchLimits::default()
    };
    eprintln!(
        "[smoke_ext] max_expanded_states={}",
        limits.max_expanded_states
    );

    let prep_start = Instant::now();
    let pg = prepare(graph, &limits).unwrap_or_else(|e| panic!("prepare: {}", e));
    eprintln!("[smoke_ext] prepare() took {:.2?}", prep_start.elapsed());

    eprintln!("\n[smoke_ext] === SEARCH RESULTS (extended limits) ===");
    eprintln!(
        "{:<50}  {:>10}  {:>4}  {:>8}  {:>8}  own",
        "pair_id", "status", "cand", "expand", "ms"
    );

    for &(pair_id, origin_node_id) in PAIRS {
        let req = SearchRequest {
            request_id: format!("ext-{}", pair_id),
            release_id: release_id.clone(),
            origin_node_id: Some(origin_node_id.to_owned()),
            origin: None,
            entry_ramp_id: None,
            exit_ramp_id: None,
            min_minutes: 15,
            max_minutes: 60,
            vehicle_profile: vehicle_profile.clone(),
            pricing_at: "2026-09-10T00:00:00Z".to_owned(),
        };
        let t = Instant::now();
        let res = search_prepared(&pg, &req).unwrap_or_else(|e| panic!("search: {}", e));
        let ms = t.elapsed().as_secs_f64() * 1000.0;

        let own_pair = res
            .candidates
            .iter()
            .any(|c| c.toll.billing_pair_id == pair_id);
        eprintln!(
            "{:<50}  {:>10}  {:>4}  {:>8}  {:>8.1}  {}",
            pair_id,
            res.status,
            res.candidates.len(),
            res.expanded_states,
            ms,
            if own_pair { "YES" } else { "no" },
        );
    }
    eprintln!("[smoke_ext] done");
}
