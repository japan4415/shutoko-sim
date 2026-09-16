//! CLI entry point for shutoko-graph-builder.
//!
//! Generates deterministic `graph.json`, `snap-index.json`, and `manifest.json`
//! from OSM Overpass export JSON and human-verified billing pair seeds.

use shutoko_graph_builder::{
    apply_od_tariffs_to_graph, bind_ramps_to_graph, build_manifest, build_topology_with_report,
    generate_and_validate_billing_pairs, manifest_to_deterministic_json,
    ramps_artifact_to_deterministic_json, snap_index_to_deterministic_json, to_deterministic_json,
    validate_od_tariffs, validate_osm_ramp_bindings, validate_osm_ramp_bindings_against_osm,
    validate_ramp_inventory, BillingPairProvenance, BillingPairsSeedFile, EdgeKind, ManifestConfig,
    OdTariffsFile, OsmRampBindingsFile, OverpassResponse, RampInventoryFile, RampKind,
    RampsArtifact, TopologyConfig, VerificationStatus,
};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

fn print_help() {
    println!(
        r#"shutoko-graph-builder: offline OSM to directed road graph pipeline

USAGE:
    shutoko-graph-builder --osm <PATH> --out-dir <PATH> [OPTIONS]

REQUIRED ARGUMENTS:
    --osm <PATH>            Path to input Overpass OSM [out:json] file
    --out-dir <PATH>        Output directory to write generated JSON artifacts

OPTIONS:
    --seed <PATH>           Path to billing-pairs-seed.json file
    --inventory <PATH>      Path to canonical ramp-inventory.json file
    --bindings <PATH>       Path to osm-ramp-bindings.json file
    --tariffs <PATH>        Path to od-tariffs.json file
    --release-id <STRING>   Release ID [default: "default-release"]
    --vehicle-profile <STR> Vehicle profile [default: "passenger-car-etc"]
    --built-at <ISO8601>    External fixed build timestamp [default: $SHUTOKO_BUILT_AT or "2026-09-10T00:00:00Z"]
    --source-date <DATE>    Data capture date (YYYY-MM-DD) [default: "2026-09-10"]
    --coverage-area <STR>   Textual coverage scope description [default: "Tokyo Inner Circular Route (C1) and Metropolitan Expressway"]
    --graph-version <VER>   Graph dataset version [default: "1.0.0"]
    --unverified-section <S> Unverified section to record in manifest (can be specified multiple times)
    --strict                Fail with non-zero exit code if no verified billing pairs are generated
    -h, --help              Print help information
    -V, --version           Print version information
"#
    );
}

struct CliArgs {
    osm_path: PathBuf,
    out_dir: PathBuf,
    seed_path: Option<PathBuf>,
    inventory_path: Option<PathBuf>,
    bindings_path: Option<PathBuf>,
    tariffs_path: Option<PathBuf>,
    release_id: String,
    vehicle_profile: String,
    built_at: String,
    source_date: String,
    coverage_area: String,
    graph_version: String,
    unverified_sections: Vec<String>,
    strict: bool,
}

fn parse_args() -> Result<CliArgs, String> {
    let raw_args: Vec<String> = env::args().collect();
    if raw_args.len() <= 1 {
        print_help();
        process::exit(0);
    }

    let mut osm_path: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut seed_path: Option<PathBuf> = None;
    let mut inventory_path: Option<PathBuf> = None;
    let mut bindings_path: Option<PathBuf> = None;
    let mut tariffs_path: Option<PathBuf> = None;
    let mut release_id = "default-release".to_string();
    let mut vehicle_profile = "passenger-car-etc".to_string();
    let mut built_at =
        env::var("SHUTOKO_BUILT_AT").unwrap_or_else(|_| "2026-09-10T00:00:00Z".to_string());
    let mut source_date = "2026-09-10".to_string();
    let mut coverage_area =
        "Tokyo Inner Circular Route (C1) and Metropolitan Expressway".to_string();
    let mut graph_version = "1.0.0".to_string();
    let mut unverified_sections: Vec<String> = Vec::new();
    let mut strict = false;

    let mut i = 1;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("shutoko-graph-builder {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "--osm" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--osm requires a path argument".into());
                }
                osm_path = Some(PathBuf::from(&raw_args[i]));
            }
            "--out-dir" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--out-dir requires a directory path argument".into());
                }
                out_dir = Some(PathBuf::from(&raw_args[i]));
            }
            "--seed" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--seed requires a path argument".into());
                }
                seed_path = Some(PathBuf::from(&raw_args[i]));
            }
            "--inventory" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--inventory requires a path argument".into());
                }
                inventory_path = Some(PathBuf::from(&raw_args[i]));
            }
            "--bindings" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--bindings requires a path argument".into());
                }
                bindings_path = Some(PathBuf::from(&raw_args[i]));
            }
            "--tariffs" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--tariffs requires a path argument".into());
                }
                tariffs_path = Some(PathBuf::from(&raw_args[i]));
            }
            "--release-id" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--release-id requires a string argument".into());
                }
                release_id = raw_args[i].clone();
            }
            "--vehicle-profile" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--vehicle-profile requires a string argument".into());
                }
                vehicle_profile = raw_args[i].clone();
            }
            "--built-at" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--built-at requires a timestamp argument".into());
                }
                built_at = raw_args[i].clone();
            }
            "--source-date" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--source-date requires a date argument".into());
                }
                source_date = raw_args[i].clone();
            }
            "--coverage-area" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--coverage-area requires a string argument".into());
                }
                coverage_area = raw_args[i].clone();
            }
            "--graph-version" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--graph-version requires a version argument".into());
                }
                graph_version = raw_args[i].clone();
            }
            "--unverified-section" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--unverified-section requires a string argument".into());
                }
                unverified_sections.push(raw_args[i].clone());
            }
            "--strict" => {
                strict = true;
            }
            unknown => {
                return Err(format!("unknown option: {}", unknown));
            }
        }
        i += 1;
    }

    let osm_path = osm_path.ok_or_else(|| "missing required argument: --osm".to_string())?;
    let out_dir = out_dir.ok_or_else(|| "missing required argument: --out-dir".to_string())?;

    Ok(CliArgs {
        osm_path,
        out_dir,
        seed_path,
        inventory_path,
        bindings_path,
        tariffs_path,
        release_id,
        vehicle_profile,
        built_at,
        source_date,
        coverage_area,
        graph_version,
        unverified_sections,
        strict,
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!("Run with --help for usage instructions.");
            process::exit(1);
        }
    };

    // 1. Read and parse OSM JSON
    let osm_raw = fs::read_to_string(&args.osm_path).map_err(|e| {
        format!(
            "failed to read OSM input {}: {}",
            args.osm_path.display(),
            e
        )
    })?;
    let overpass_resp: OverpassResponse = serde_json::from_str(&osm_raw).map_err(|e| {
        format!(
            "failed to parse OSM JSON {}: {}",
            args.osm_path.display(),
            e
        )
    })?;

    // 2. Build topology
    let top_config = TopologyConfig {
        release_id: args.release_id.clone(),
        vehicle_profile: args.vehicle_profile.clone(),
    };
    let (mut graph, snap_index, top_report) =
        build_topology_with_report(&overpass_resp, &top_config)
            .map_err(|e| format!("topology build failed: {}", e))?;

    // 3. Process declarative billing pair seeds if provided
    let mut unverified_from_seeds = Vec::new();
    let mut billing_provenances = Vec::new();

    if let Some(seed_file_path) = &args.seed_path {
        let seed_raw = fs::read_to_string(seed_file_path).map_err(|e| {
            format!(
                "failed to read seed file {}: {}",
                seed_file_path.display(),
                e
            )
        })?;
        let seed_file: BillingPairsSeedFile = serde_json::from_str(&seed_raw).map_err(|e| {
            format!(
                "failed to parse seed JSON {}: {}",
                seed_file_path.display(),
                e
            )
        })?;

        let report = generate_and_validate_billing_pairs(&graph, &seed_file);

        if !report.rejected_pairs.is_empty() {
            eprintln!(
                "Warning: {} billing pair seed(s) rejected during generation/validation:",
                report.rejected_pairs.len()
            );
            for rej in &report.rejected_pairs {
                eprintln!("  - {}: {}", rej.seed_id, rej.reason);
                unverified_from_seeds.push(format!("rejected:{}:{}", rej.seed_id, rej.reason));
            }
        }

        for pair in &report.valid_pairs {
            if pair.status == VerificationStatus::Verified {
                if let Some(s) = seed_file.billing_pairs.iter().find(|s| s.id == pair.id) {
                    billing_provenances.push(BillingPairProvenance {
                        id: s.id.clone(),
                        source: s.provenance.source.clone(),
                        source_date: s.provenance.source_date.clone(),
                        notes: s.provenance.notes.clone(),
                    });
                }
            } else if pair.status == VerificationStatus::Unverified {
                unverified_from_seeds.push(format!("unverified:{}", pair.id));
            }
        }

        if args.strict
            && report
                .valid_pairs
                .iter()
                .all(|p| p.status != VerificationStatus::Verified)
        {
            return Err("strict mode: no verified billing pair generated".into());
        }

        graph.billing_pairs = report.valid_pairs;
    }

    // 3.5. Process canonical ramp inventory, OSM bindings, and OD tariffs if provided
    let mut ramps_artifact_opt: Option<(RampsArtifact, String)> = None;
    if let Some(inv_path) = &args.inventory_path {
        let inv_raw = fs::read_to_string(inv_path).map_err(|e| {
            format!(
                "failed to read inventory file {}: {}",
                inv_path.display(),
                e
            )
        })?;
        let inv: RampInventoryFile = serde_json::from_str(&inv_raw).map_err(|e| {
            format!(
                "failed to parse inventory JSON {}: {}",
                inv_path.display(),
                e
            )
        })?;
        validate_ramp_inventory(&inv).map_err(|errs| {
            format!(
                "inventory validation failed for {}:\n  {}",
                inv_path.display(),
                errs.join("\n  ")
            )
        })?;

        let bindings_file: OsmRampBindingsFile = if let Some(bin_path) = &args.bindings_path {
            let bin_raw = fs::read_to_string(bin_path).map_err(|e| {
                format!("failed to read bindings file {}: {}", bin_path.display(), e)
            })?;
            let b: OsmRampBindingsFile = serde_json::from_str(&bin_raw).map_err(|e| {
                format!(
                    "failed to parse bindings JSON {}: {}",
                    bin_path.display(),
                    e
                )
            })?;
            validate_osm_ramp_bindings(&b, &inv).map_err(|errs| {
                format!(
                    "bindings validation failed for {}:\n  {}",
                    bin_path.display(),
                    errs.join("\n  ")
                )
            })?;
            validate_osm_ramp_bindings_against_osm(&b, &overpass_resp).map_err(|errs| {
                format!(
                    "bindings OSM fixture existence validation failed for {}:\n  {}",
                    bin_path.display(),
                    errs.join("\n  ")
                )
            })?;
            b
        } else {
            OsmRampBindingsFile {
                version: 1,
                source_date: args.source_date.clone(),
                bindings: Vec::new(),
                shared_physical_overrides: Vec::new(),
            }
        };

        let (bound_ramps, ramp_artifact_entries, unbound_notes) =
            bind_ramps_to_graph(&graph, &inv, &bindings_file);
        graph.ramps = bound_ramps;
        for note in unbound_notes {
            unverified_from_seeds.push(note);
        }

        // Hard assertion: every verified active general entry/exit is bound,
        // while explicit unsupported records remain inventory-only.
        let unbound_active_general: Vec<&str> = inv
            .ramps
            .iter()
            .filter(|r| {
                r.status == "active"
                    && matches!(r.kind, RampKind::GeneralEntry | RampKind::GeneralExit)
                    && r.support_state.as_deref() == Some("verified_bound")
                    && !graph.ramps.iter().any(|gr| gr.id == r.ramp_id)
            })
            .map(|r| r.ramp_id.as_str())
            .collect();

        if !unbound_active_general.is_empty() {
            return Err(format!(
                "hard assertion failed: {} verified active general ramp(s) are unbound:\n  {}",
                unbound_active_general.len(),
                unbound_active_general.join("\n  ")
            )
            .into());
        }

        // Boundary/JCT connectors and non-active historical/planned ramps are
        // inventory-only records. They must never enter graph.ramps, because
        // routing-core treats graph.ramps as the user-selectable ramp set.
        let selectable_non_general: Vec<&str> = graph
            .ramps
            .iter()
            .filter(|gr| {
                inv.ramps.iter().any(|r| {
                    r.ramp_id == gr.id
                        && (r.status != "active"
                            || !matches!(r.kind, RampKind::GeneralEntry | RampKind::GeneralExit))
                })
            })
            .map(|gr| gr.id.as_str())
            .collect();
        if !selectable_non_general.is_empty() {
            return Err(format!(
                "hard assertion failed: non-general/non-active ramps leaked into graph.ramps:\n  {}",
                selectable_non_general.join("\n  ")
            )
            .into());
        }

        if let Some(tar_path) = &args.tariffs_path {
            let tar_raw = fs::read_to_string(tar_path).map_err(|e| {
                format!("failed to read tariffs file {}: {}", tar_path.display(), e)
            })?;
            let tariffs: OdTariffsFile = serde_json::from_str(&tar_raw).map_err(|e| {
                format!("failed to parse tariffs JSON {}: {}", tar_path.display(), e)
            })?;
            validate_od_tariffs(&tariffs, &inv).map_err(|errs| {
                format!(
                    "tariffs validation failed for {}:\n  {}",
                    tar_path.display(),
                    errs.join("\n  ")
                )
            })?;
            apply_od_tariffs_to_graph(&mut graph, &tariffs);
        }

        let artifact = RampsArtifact {
            schema_version: 1,
            release_id: args.release_id.clone(),
            source_date: args.source_date.clone(),
            total_ramps: ramp_artifact_entries.len(),
            bound_ramps: graph.ramps.len(),
            ramps: ramp_artifact_entries,
        };
        let ramps_json = ramps_artifact_to_deterministic_json(&artifact)
            .map_err(|e| format!("ramps serialization failed: {}", e))?;
        ramps_artifact_opt = Some((artifact, ramps_json));
    }

    // 4. Serialize graph.json and snap-index.json deterministically
    let graph_json =
        to_deterministic_json(&graph).map_err(|e| format!("graph serialization failed: {}", e))?;
    let snap_json = snap_index_to_deterministic_json(&snap_index)
        .map_err(|e| format!("snap index serialization failed: {}", e))?;

    // 5. Gather verified endpoints for manifest coverage
    let mut verified_entries: Vec<String> = graph
        .billing_pairs
        .iter()
        .filter(|p| p.status == VerificationStatus::Verified)
        .map(|p| p.entry_id.clone())
        .collect();
    verified_entries.sort();
    verified_entries.dedup();

    let mut verified_exits: Vec<String> = graph
        .billing_pairs
        .iter()
        .filter(|p| p.status == VerificationStatus::Verified)
        .map(|p| p.exit_id.clone())
        .collect();
    verified_exits.sort();
    verified_exits.dedup();

    // Index OSM way names from overpass elements for human-readable unverified section labels
    let mut way_names: HashMap<i64, String> = HashMap::new();
    for elem in &overpass_resp.elements {
        if elem.is_way() {
            if let Some(name) = elem
                .get_tag("name")
                .or_else(|| elem.get_tag("name:ja"))
                .or_else(|| elem.get_tag("description"))
            {
                way_names.insert(elem.id, name.to_string());
            }
        }
    }

    let extract_way_id = |edge_id: &str| -> Option<i64> {
        let parts: Vec<&str> = edge_id.split(':').collect();
        if parts.len() >= 2 && parts[1].starts_with('w') {
            parts[1][1..].parse::<i64>().ok()
        } else {
            None
        }
    };

    // Automatically enumerate unverified entry and exit edges present in graph but not in verified billing pairs
    let mut unverified_edge_sections = Vec::new();
    for e in &graph.edges {
        if e.kind == EdgeKind::Entry && !verified_entries.contains(&e.id) {
            let label = if let Some(wid) = extract_way_id(&e.id) {
                if let Some(name) = way_names.get(&wid) {
                    format!("entry:{} ({})", e.id, name)
                } else {
                    format!("entry:{}", e.id)
                }
            } else {
                format!("entry:{}", e.id)
            };
            unverified_edge_sections.push(label);
        } else if e.kind == EdgeKind::Exit && !verified_exits.contains(&e.id) {
            let label = if let Some(wid) = extract_way_id(&e.id) {
                if let Some(name) = way_names.get(&wid) {
                    format!("exit:{} ({})", e.id, name)
                } else {
                    format!("exit:{}", e.id)
                }
            } else {
                format!("exit:{}", e.id)
            };
            unverified_edge_sections.push(label);
        }
    }

    let mut all_unverified = args.unverified_sections;
    all_unverified.extend(unverified_from_seeds);
    all_unverified.extend(unverified_edge_sections);

    // Note ramp edges that could not be classified due to missing surface context.
    // Non-zero counts indicate the OSM extract lacked vehicle-accessible surface road
    // ways at the ramp endpoints; downstream tooling can detect this quality signal.
    if top_report.undecidable_ramp_edges > 0 {
        all_unverified.push(format!(
            "undecidable-ramp-classification: {} ramp edge(s) could not be classified \
             as Entry/Exit (no vehicle-accessible surface road context and no discriminating \
             OSM node tag); conservatively classified as Shutoko — re-run with updated OSM \
             extract for accurate classification",
            top_report.undecidable_ramp_edges
        ));
    }

    // Note excluded routes and skipped/unsupported restrictions
    if args.coverage_area.contains("C1")
        && !args.coverage_area.contains("All")
        && !args.coverage_area.contains("all")
        && !args.coverage_area.contains("24")
    {
        all_unverified.push(
            "excluded-route: Metropolitan Expressway lines other than C1 (e.g. B, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, K, S, Y)".to_string(),
        );
    }
    if top_report.skipped_conditional > 0 {
        all_unverified.push(format!(
            "unsupported-restriction: {} conditional turn restrictions (conditional) excluded from static graph",
            top_report.skipped_conditional
        ));
    }
    if top_report.skipped_no_via > 0 {
        all_unverified.push(format!(
            "unsupported-restriction: {} turn restrictions missing via member (no via) excluded from static graph",
            top_report.skipped_no_via
        ));
    }
    if top_report.skipped_missing_elements > 0 {
        all_unverified.push(format!(
            "unsupported-restriction: {} turn restrictions referencing elements outside graph (outside graph) excluded from static graph",
            top_report.skipped_missing_elements
        ));
    }
    if top_report.skipped_disconnected > 0 {
        all_unverified.push(format!(
            "unsupported-restriction: {} disconnected via-way turn restrictions (disconnected) excluded from static graph",
            top_report.skipped_disconnected
        ));
    }
    if top_report.skipped_only_via_way > 0 {
        all_unverified.push(format!(
            "unsupported-restriction: {} only_* turn restrictions with via=way (only via-way) excluded from static graph",
            top_report.skipped_only_via_way
        ));
    }
    if top_report.skipped_unrecognized > 0 {
        all_unverified.push(format!(
            "unsupported-restriction: {} unrecognized turn restrictions (unrecognized) excluded from static graph",
            top_report.skipped_unrecognized
        ));
    }

    all_unverified.sort();
    all_unverified.dedup();

    // 6. Build manifest
    let manifest_config = ManifestConfig {
        release_id: args.release_id.clone(),
        engine_version: shutoko_routing_core::VERSION.into(),
        graph_version: args.graph_version,
        built_at: args.built_at,
        source_date: args.source_date,
        coverage_area: args.coverage_area,
        vehicle_profile: args.vehicle_profile,
        time_model_version: "v1-static-speeds".into(),
        billing_pairs_version: "v1".into(),
        unverified_sections: all_unverified,
        provenance: billing_provenances,
    };

    let mut artifacts_to_bundle: Vec<(&str, &[u8])> = vec![
        ("graph.json", graph_json.as_bytes()),
        ("snap-index.json", snap_json.as_bytes()),
    ];
    if let Some((_, ref r_json)) = ramps_artifact_opt {
        artifacts_to_bundle.push(("ramps.json", r_json.as_bytes()));
    }

    let manifest = build_manifest(
        &manifest_config,
        verified_entries,
        verified_exits,
        artifacts_to_bundle,
    );

    let manifest_json = manifest_to_deterministic_json(&manifest)
        .map_err(|e| format!("manifest serialization failed: {}", e))?;

    // 7. Write outputs to out_dir
    fs::create_dir_all(&args.out_dir).map_err(|e| {
        format!(
            "failed to create output directory {}: {}",
            args.out_dir.display(),
            e
        )
    })?;

    let graph_out = args.out_dir.join("graph.json");
    let snap_out = args.out_dir.join("snap-index.json");
    let manifest_out = args.out_dir.join("manifest.json");

    fs::write(&graph_out, graph_json.as_bytes())
        .map_err(|e| format!("failed to write {}: {}", graph_out.display(), e))?;
    fs::write(&snap_out, snap_json.as_bytes())
        .map_err(|e| format!("failed to write {}: {}", snap_out.display(), e))?;
    fs::write(&manifest_out, manifest_json.as_bytes())
        .map_err(|e| format!("failed to write {}: {}", manifest_out.display(), e))?;

    if let Some((_, ref r_json)) = ramps_artifact_opt {
        let ramps_out = args.out_dir.join("ramps.json");
        fs::write(&ramps_out, r_json.as_bytes())
            .map_err(|e| format!("failed to write {}: {}", ramps_out.display(), e))?;
    }

    println!(
        "Successfully generated release \"{}\" in {}:",
        args.release_id,
        args.out_dir.display()
    );
    println!(
        "  - graph.json ({} nodes, {} edges, {} billing pairs, {} ramps)",
        graph.nodes.len(),
        graph.edges.len(),
        graph.billing_pairs.len(),
        graph.ramps.len(),
    );
    println!(
        "  - snap-index.json ({} local snap nodes)",
        snap_index.nodes.len()
    );
    if let Some((ref art, _)) = ramps_artifact_opt {
        println!(
            "  - ramps.json ({} canonical ramps, {} bound to graph)",
            art.total_ramps, art.bound_ramps
        );
    }
    println!("  - manifest.json ({} artifacts)", manifest.artifacts.len());

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
