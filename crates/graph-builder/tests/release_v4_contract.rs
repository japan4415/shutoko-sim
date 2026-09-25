//! End-to-end contract for the `all-real-v4` release build.
//!
//! The heavy cases run the release binary over the real OSM snapshot, so they are
//! marked `#[ignore]` and executed in release mode by CI:
//! `cargo test --release -p shutoko-graph-builder --test release_v4_contract -- --ignored`.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BUILDER: &str = env!("CARGO_BIN_EXE_shutoko-graph-builder");
const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");
const ARTIFACTS: [&str; 5] = [
    "graph.json",
    "snap-index.json",
    "ramps.json",
    "od-tariffs.json",
    "pair-candidates.json",
];

fn repo_root() -> PathBuf {
    Path::new(REPO_ROOT)
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn sha256(bytes: &[u8]) -> String {
    shutoko_graph_builder::compute_sha256(bytes)
}

fn builder_command(out_dir: &Path, release_id: &str) -> Command {
    let root = repo_root();
    let mut command = Command::new(BUILDER);
    command
        .arg("--osm")
        .arg(root.join("fixtures/osm/shutoko-all.json"))
        .arg("--seed")
        .arg(root.join("data/billing-pairs-seed.json"))
        .arg("--inventory")
        .arg(root.join("data/ramp-inventory.json"))
        .arg("--bindings")
        .arg(root.join("data/osm-ramp-bindings.json"))
        .arg("--tariffs")
        .arg(root.join("data/od-tariffs.json"))
        .arg("--adjacency")
        .arg(root.join("data/billing-pair-adjacency.json"))
        .arg("--support-decisions")
        .arg(root.join("data/ramp-support-decisions.json"))
        .arg("--out-dir")
        .arg(out_dir)
        .arg("--release-id")
        .arg(release_id)
        .arg("--built-at")
        .arg("2026-09-24T00:00:00Z")
        .arg("--source-date")
        .arg("2026-09-16")
        .arg("--vehicle-profile")
        .arg("passenger-car-etc")
        .arg("--coverage-area")
        .arg("Metropolitan Expressway network (Tokyo, Kanagawa, Saitama)")
        .arg("--graph-version")
        .arg("1.0.0");
    command
}

fn run_builder(out_dir: &Path, extra: &[&str]) -> std::process::Output {
    builder_command(out_dir, "all-real-v4")
        .args(extra)
        .output()
        .expect("builder must be runnable")
}

fn contract_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "shutoko-release-v4-contract-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("contract output directory");
    dir
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("artifact must exist")).expect("valid JSON")
}

fn generate(name: &str) -> PathBuf {
    let dir = contract_dir(name);
    let output = run_builder(&dir, &[]);
    assert!(
        output.status.success(),
        "all-real-v4 build failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}

fn artifact_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    ARTIFACTS
        .iter()
        .map(|name| {
            (
                (*name).to_string(),
                fs::read(dir.join(name)).unwrap_or_else(|error| panic!("{name}: {error}")),
            )
        })
        .collect()
}

#[test]
fn relation_selection_fails_closed_before_any_expensive_work() {
    let dir = contract_dir("selection");
    // Relation selection is resolved before the topology build, so these cases
    // stay cheap even in a debug test run.
    let cases: [(Option<&str>, &[&str], &str); 5] = [
        (
            None,
            &["--relation-id", "999999999"],
            "is absent from the OSM input",
        ),
        (
            None,
            &["--relation-id", "24039781"],
            "is an OSM way element, not a route relation",
        ),
        (
            None,
            &["--relation-id", "4256011"],
            "is not an OSM type=route relation",
        ),
        (
            Some("all-real-v4"),
            &["--relation-id", "4256008"],
            "--relation-id cannot be combined with a full-coverage release",
        ),
        (
            Some("all-real-v4"),
            &["--all-route-relations", "--relation-id", "4256008"],
            "--relation-id cannot be combined with a full-coverage release",
        ),
    ];
    for (release_id, args, expected) in cases {
        let release = release_id.unwrap_or("relation-selection-probe");
        let output = builder_command(&dir, release)
            .args(args)
            .output()
            .expect("builder must be runnable");
        assert!(!output.status.success(), "{args:?} must fail closed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected),
            "{args:?} stderr was {stderr:?}, expected {expected:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "runs the release binary three times over the real OSM snapshot"]
fn all_real_v4_artifacts_are_byte_identical_across_three_generations() {
    let first = generate("gen-1");
    let second = generate("gen-2");
    let third = generate("gen-3");
    assert_eq!(artifact_bytes(&first), artifact_bytes(&second));
    assert_eq!(artifact_bytes(&second), artifact_bytes(&third));
    for dir in [first, second, third] {
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
#[ignore = "runs the release binary over the real OSM snapshot"]
fn all_real_v4_manifest_binds_every_artifact_and_input_hash() {
    let dir = generate("manifest");
    let manifest = read_json(&dir.join("manifest.json"));
    let graph = read_json(&dir.join("graph.json"));
    let candidates = read_json(&dir.join("pair-candidates.json"));
    let root = repo_root();

    assert_eq!(manifest["releaseId"], "all-real-v4");
    assert_eq!(manifest["graphSchemaVersion"], 4);
    assert_eq!(manifest["routePlanVersion"], 1);
    assert_eq!(manifest["billingPairsVersion"], "v3");
    assert_eq!(manifest["tariffModelVersion"], 1);
    assert_eq!(
        graph["billingPairsVersion"],
        manifest["billingPairsVersion"]
    );
    assert_eq!(graph["tariffModelVersion"], manifest["tariffModelVersion"]);

    let artifacts = manifest["artifacts"].as_array().unwrap();
    let mut paths = artifacts
        .iter()
        .map(|artifact| artifact["path"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    paths.sort();
    let mut expected_paths = ARTIFACTS.map(|name| name.to_string()).to_vec();
    expected_paths.sort();
    assert_eq!(paths, expected_paths);
    for artifact in artifacts {
        let path = artifact["path"].as_str().unwrap();
        let bytes = fs::read(dir.join(path)).unwrap();
        assert_eq!(artifact["sha256"], sha256(&bytes), "{path}");
        assert_eq!(artifact["byteLength"], bytes.len() as u64, "{path}");
    }

    let input_hashes = &manifest["pairDerivation"]["inputHashes"];
    assert_eq!(
        input_hashes["osmSnapshotSha256"],
        sha256(&fs::read(root.join("fixtures/osm/shutoko-all.json")).unwrap())
    );
    assert_eq!(
        input_hashes["odTariffsSha256"],
        sha256(&fs::read(root.join("data/od-tariffs.json")).unwrap())
    );
    assert_eq!(
        input_hashes["billingPairAdjacencySha256"],
        sha256(&fs::read(root.join("data/billing-pair-adjacency.json")).unwrap())
    );
    assert_eq!(
        manifest["routeMembershipsSha256"],
        input_hashes["routeMembershipIndexSha256"]
    );
    for key in [
        "osmSnapshotSha256",
        "rampLedgerSha256",
        "routeMembershipIndexSha256",
        "billingPairAdjacencySha256",
        "odTariffsSha256",
    ] {
        let value = input_hashes[key].as_str().unwrap();
        assert_eq!(value.len(), 64, "{key}");
        assert!(
            value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "{key}"
        );
    }

    // The standalone artifact is the versioned catalog verbatim, and the graph
    // embeds the same catalog through the routing-core v3 projection.
    let catalog_bytes = fs::read(root.join("data/od-tariffs.json")).unwrap();
    assert_eq!(
        fs::read(dir.join("od-tariffs.json")).unwrap(),
        catalog_bytes
    );
    let embedded = graph["odTariffsV3"].clone();
    assert!(
        embedded.is_object(),
        "odTariffsV3 must be embedded as an object"
    );
    let source: Value = serde_json::from_slice(&catalog_bytes).unwrap();
    assert_eq!(embedded["version"], source["version"]);
    assert_eq!(embedded["fareBasis"], source["fareBasis"]);
    assert_eq!(embedded["vehicleClass"], source["vehicleClass"]);
    assert_eq!(embedded["paymentMethod"], source["paymentMethod"]);
    let embedded_prices = embedded["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|assignment| {
            (
                assignment["assignmentId"].clone(),
                assignment["prices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|price| {
                        (
                            price["ruleId"].clone(),
                            price["amountYen"].clone(),
                            price["effectiveFrom"].clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let source_prices = source["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|assignment| {
            (
                assignment["assignmentId"].clone(),
                assignment["prices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|price| {
                        (
                            price["ruleId"].clone(),
                            price["amountYen"].clone(),
                            price["effectiveFrom"].clone(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(embedded_prices, source_prices);
    assert_eq!(
        candidates["inputHashes"], *input_hashes,
        "the report must repeat the hashed inputs"
    );
    assert_eq!(candidates["schemaVersion"], 2);
    assert_eq!(candidates["rule"], "billingPairDerivation/v2");
    assert_eq!(candidates["automaticSeedWrite"], false);
    let _ = fs::remove_dir_all(dir);
}

#[test]
#[ignore = "runs the release binary over the real OSM snapshot"]
fn all_real_v4_pair_derivation_covers_every_route_relation_and_membership() {
    let dir = generate("coverage");
    let graph = read_json(&dir.join("graph.json"));
    let candidates = read_json(&dir.join("pair-candidates.json"));

    let membership_ids = graph["routeMemberships"]
        .as_array()
        .unwrap()
        .iter()
        .map(|membership| membership["membershipId"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    let mut manifest_ids = candidates["relationManifest"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["membershipId"].as_str().unwrap())
        .collect::<Vec<_>>();
    manifest_ids.sort_unstable();
    let mut expected = membership_ids.iter().copied().collect::<Vec<_>>();
    expected.sort_unstable();
    assert_eq!(manifest_ids, expected);
    assert!(candidates["relationManifest"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["candidatePairIds"].as_array().unwrap().is_empty()));

    let coverage = candidates["relationCoverage"].as_array().unwrap();
    assert_eq!(coverage.len(), 26);
    assert_eq!(
        candidates["summary"]["relationCoverage"]["relationTotal"]
            .as_u64()
            .unwrap(),
        coverage.len() as u64
    );
    let expanded = coverage
        .iter()
        .filter(|relation| relation["status"] == "pass")
        .count();
    let failed = coverage.len() - expanded;
    assert!(failed > 0, "unexpandable relations must stay visible");
    assert_eq!(
        candidates["summary"]["relationCoverage"]["relationExpanded"]
            .as_u64()
            .unwrap(),
        expanded as u64
    );
    assert_eq!(
        candidates["summary"]["relationCoverage"]["relationFailed"]
            .as_u64()
            .unwrap(),
        failed as u64
    );
    for relation in coverage {
        assert!(relation["membershipIds"].is_array());
        if relation["status"] == "fail" {
            assert!(relation["reasonCode"].is_string(), "{relation}");
            assert!(relation["reason"].is_string(), "{relation}");
        }
    }
    for membership_id in [
        "route:C1:inner",
        "route:C1:outer",
        "route:2:inbound",
        "route:2:outbound",
    ] {
        let record = candidates["relationManifest"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["membershipId"] == membership_id)
            .unwrap_or_else(|| panic!("{membership_id} is missing"));
        assert!(!record["candidatePairIds"].as_array().unwrap().is_empty());
    }
    let summary = &candidates["summary"];
    assert_eq!(summary["candidateTotal"], 11);
    assert_eq!(summary["eligibleForReview"], 9);
    assert_eq!(summary["hold"], 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
#[ignore = "runs the release binary over the real OSM snapshot"]
fn routing_core_reads_the_generated_graph_and_the_legacy_release() {
    let dir = generate("routing-core");
    let graph = fs::read_to_string(dir.join("graph.json")).unwrap();
    shutoko_routing_core::prepare_json(&graph, "{}")
        .expect("the generated all-real-v4 graph must be readable by routing-core");
    // 公開 fixture 自体も同じ build の all-real-v4 であり、reader が一致すること。
    let checked_in = fs::read_to_string(repo_root().join("fixtures/generated/graph.json")).unwrap();
    let checked_in_manifest = read_json(&repo_root().join("fixtures/generated/manifest.json"));
    assert_eq!(checked_in_manifest["releaseId"], "all-real-v4");
    assert_eq!(checked_in_manifest["billingPairsVersion"], "v3");
    assert_eq!(checked_in_manifest["tariffModelVersion"], 1);
    assert_eq!(
        shutoko_graph_builder::compute_sha256(checked_in.as_bytes()),
        sha256(&graph),
        "the checked-in all-real-v4 graph must be byte identical to a fresh build"
    );
    shutoko_routing_core::prepare_json(&checked_in, "{}")
        .expect("the checked-in all-real-v4 graph must be readable by routing-core");
    let _ = fs::remove_dir_all(dir);
}
