import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "vitest";
import generatedGraph from "../../fixtures/generated/graph.json?raw";
import generatedManifest from "../../fixtures/generated/manifest.json?raw";
import { routeMembershipsSha256, seedR2 } from "../scripts/seed-local-r2.mjs";

const FIXTURE_NAMES = ["graph.json", "snap-index.json"];
const ENGINE_NAMES = [
  "shutoko_routing_bg.wasm",
  "shutoko_routing.js",
  "shutoko_routing.d.ts",
  "index.d.ts",
];
const PUBLISH_ORDER = [...FIXTURE_NAMES, ...ENGINE_NAMES, "engine.json", "manifest.json"];

function sha256(content) {
  return crypto.createHash("sha256").update(content).digest("hex");
}

function createRepo(t, { releaseId = "c1-real-v2", manifestTransform } = {}) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "seed-r2-test-"));
  t.onTestFinished(() => fs.rmSync(repoRoot, { recursive: true, force: true }));
  const fixturesDir = path.join(repoRoot, "fixtures/generated");
  const wasmDir = path.join(repoRoot, "dist/wasm");
  fs.mkdirSync(fixturesDir, { recursive: true });
  fs.mkdirSync(wasmDir, { recursive: true });
  fs.mkdirSync(path.join(repoRoot, "workers"), { recursive: true });

  const artifacts = FIXTURE_NAMES.map((name) => {
    const content = Buffer.from(`${name}-content`);
    fs.writeFileSync(path.join(fixturesDir, name), content);
    return { path: name, sha256: sha256(content), byteLength: content.length };
  });
  for (const name of ENGINE_NAMES) {
    fs.writeFileSync(path.join(wasmDir, name), Buffer.from(`${name}-content`));
  }

  let manifest = { schemaVersion: 1, releaseId, artifacts };
  if (manifestTransform) manifest = manifestTransform(manifest);
  fs.writeFileSync(path.join(fixturesDir, "manifest.json"), `${JSON.stringify(manifest)}\n`);
  return repoRoot;
}

function createSchema4Repo(t) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "seed-r2-schema4-test-"));
  t.onTestFinished(() => fs.rmSync(repoRoot, { recursive: true, force: true }));
  const fixturesDir = path.join(repoRoot, "fixtures/generated");
  const wasmDir = path.join(repoRoot, "dist/wasm");
  fs.mkdirSync(fixturesDir, { recursive: true });
  fs.mkdirSync(wasmDir, { recursive: true });
  fs.mkdirSync(path.join(repoRoot, "workers"), { recursive: true });

  const edgeIds = ["edge:main"];
  const graph = {
    schemaVersion: 4,
    releaseId: "all-real-v3",
    vehicleProfile: "passenger-car-etc",
    nodes: [
      { id: "n:1", lat: 35.0, lon: 139.0 },
      { id: "n:2", lat: 35.1, lon: 139.1 },
    ],
    edges: [
      {
        id: edgeIds[0],
        from: "n:1",
        to: "n:2",
        kind: "shutoko",
        durationSeconds: 60,
        distanceMeters: 1000,
      },
    ],
    billingPairs: [],
    routeMemberships: [
      {
        membershipId: "route:C1:inner",
        routeId: "C1",
        direction: "inner",
        directionMappingVersion: "osm-relation-role/v1",
        segments: [
          {
            segmentId: "relation:1:inner:0",
            sourceKind: "relationMainline",
            sourceRelationId: "1",
            sourceSnapshotSha256: "a".repeat(64),
            bindingEvidenceId: null,
            orderedEdgeIds: edgeIds,
            orderedEdgeIdsSha256: sha256(Buffer.from(JSON.stringify(edgeIds))),
          },
        ],
      },
    ],
  };
  const graphContent = Buffer.from(`${JSON.stringify(graph)}\n`);
  const snapContent = Buffer.from('{"schemaVersion":2,"releaseId":"all-real-v3","nodes":[]}\n');
  const rampsContent = Buffer.from('{"schemaVersion":1,"releaseId":"all-real-v3","ramps":[]}\n');
  fs.writeFileSync(path.join(fixturesDir, "graph.json"), graphContent);
  fs.writeFileSync(path.join(fixturesDir, "snap-index.json"), snapContent);
  fs.writeFileSync(path.join(fixturesDir, "ramps.json"), rampsContent);
  for (const name of ENGINE_NAMES) {
    fs.writeFileSync(path.join(wasmDir, name), Buffer.from(`${name}-content`));
  }
  const artifacts = [
    { name: "graph.json", content: graphContent },
    { name: "snap-index.json", content: snapContent },
    { name: "ramps.json", content: rampsContent },
  ].map(({ name, content }) => ({ path: name, sha256: sha256(content), byteLength: content.length }));
  const manifest = {
    schemaVersion: 1,
    releaseId: "all-real-v3",
    graphSchemaVersion: 4,
    routePlanVersion: 1,
    billingPairsVersion: "v2",
    routeMembershipsSha256: routeMembershipsSha256(graph.routeMemberships),
    artifacts,
  };
  fs.writeFileSync(path.join(fixturesDir, "manifest.json"), `${JSON.stringify(manifest)}\n`);
  return repoRoot;
}

function operation(call) {
  return call.args[3];
}

function keyName(call) {
  return call.args[4].split("/").at(-1);
}

function fileArg(call) {
  return call.args[call.args.indexOf("--file") + 1];
}

function quiet() {}

test("manifest.releaseId is required and validated before any command", (t) => {
  for (const invalid of [undefined, "", "C1-real-v2", "../c1-real-v2", "a".repeat(65)]) {
    const repoRoot = createRepo(t, {
      manifestTransform: (manifest) => {
        if (invalid === undefined) delete manifest.releaseId;
        else manifest.releaseId = invalid;
        return manifest;
      },
    });
    let calls = 0;
    assert.throws(
      () => seedR2({ repoRoot, runCommand: () => calls++, log: quiet }),
      /manifest\.releaseId is required/
    );
    assert.equal(calls, 0);
  }
});

test("manifest artifact paths are fixed safe basenames", (t) => {
  const repoRoot = createRepo(t, {
    manifestTransform: (manifest) => {
      manifest.artifacts[0].path = "../graph.json";
      return manifest;
    },
  });
  assert.throws(
    () => seedR2({ repoRoot, runCommand: () => assert.fail("must not execute"), log: quiet }),
    /Unsafe or unsupported manifest artifact path/
  );
});

test("all-real releases require ramps.json while c1-real-v1/v2 remain backward compatible", (t) => {
  for (const releaseId of ["c1-real-v1", "c1-real-v2"]) {
    const repoRoot = createRepo(t, { releaseId });
    assert.doesNotThrow(() => seedR2({ repoRoot, runCommand: () => {}, log: quiet }));
  }

  for (const releaseId of ["all-real-v1", "all-real-v2"]) {
    const missingRampsRoot = createRepo(t, { releaseId });
    assert.throws(
      () => seedR2({ repoRoot: missingRampsRoot, runCommand: () => assert.fail("must not publish"), log: quiet }),
      /missing required artifact ramps\.json/
    );
  }
});

test("schema 4 release metadata and route membership hash are verified before seeding", (t) => {
  const repoRoot = createSchema4Repo(t);
  let calls = 0;
  assert.doesNotThrow(() => seedR2({ repoRoot, runCommand: () => calls++, log: quiet }));
  assert.equal(calls, 9);

  const manifestPath = path.join(repoRoot, "fixtures/generated/manifest.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  manifest.routeMembershipsSha256 = "b".repeat(64);
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest)}\n`);
  assert.throws(
    () => seedR2({ repoRoot, runCommand: () => assert.fail("must not publish"), log: quiet }),
    /routeMembershipsSha256 mismatch/,
  );
});

test("generated graph and manifest use the same route membership hash", () => {
  const graph = JSON.parse(generatedGraph);
  const manifest = JSON.parse(generatedManifest);
  assert.equal(graph.schemaVersion, 4);
  assert.match(manifest.routeMembershipsSha256, /^[0-9a-f]{64}$/);
  assert.equal(routeMembershipsSha256(graph.routeMemberships), manifest.routeMembershipsSha256);
});

test("all four engine artifacts are required in local and remote modes", (t) => {
  for (const isRemote of [false, true]) {
    const repoRoot = createRepo(t);
    fs.rmSync(path.join(repoRoot, "dist/wasm/index.d.ts"));
    let calls = 0;
    assert.throws(
      () => seedR2({ isRemote, repoRoot, runCommand: () => calls++, log: quiet }),
      /Required artifact index\.d\.ts not found/
    );
    assert.equal(calls, 0);
  }
});

test("local seed uses argument arrays, publishes manifest last, and permits reruns", (t) => {
  const repoRoot = createRepo(t);
  const calls = [];
  const runCommand = (executable, args, options) => {
    calls.push({ executable, args, options });
  };

  seedR2({ repoRoot, runCommand, log: quiet });
  seedR2({ repoRoot, runCommand, log: quiet });

  assert.equal(calls.length, 16);
  for (const batch of [calls.slice(0, 8), calls.slice(8)]) {
    assert.deepEqual(batch.map(operation), Array(8).fill("put"));
    assert.deepEqual(batch.map(keyName), PUBLISH_ORDER);
    assert.ok(batch.every((call) => call.executable === "npx"));
    assert.ok(batch.every((call) => call.args.includes("--local")));
    assert.ok(batch.every((call) => call.options.stdio === "inherit"));
  }
});

test("remote seed refuses an existing manifest and fails closed on unknown preflight errors", (t) => {
  const existingRoot = createRepo(t);
  const existingCalls = [];
  assert.throws(
    () =>
      seedR2({
        isRemote: true,
        repoRoot: existingRoot,
        log: quiet,
        runCommand: (executable, args) => existingCalls.push({ executable, args }),
      }),
    /already published; refusing to overwrite/
  );
  assert.equal(existingCalls.length, 1);
  assert.equal(operation(existingCalls[0]), "get");
  assert.equal(keyName(existingCalls[0]), "manifest.json");

  const errorRoot = createRepo(t);
  let errorCalls = 0;
  assert.throws(
    () =>
      seedR2({
        isRemote: true,
        repoRoot: errorRoot,
        log: quiet,
        runCommand: () => {
          errorCalls += 1;
          throw new Error("authentication unavailable");
        },
      }),
    /Remote preflight failed closed/
  );
  assert.equal(errorCalls, 1);
});

test("remote seed publishes payload then engine then manifest and verifies all eight objects", (t) => {
  const repoRoot = createRepo(t);
  const calls = [];
  const uploaded = new Map();
  let preflight = true;
  const runCommand = (executable, args, options) => {
    const call = { executable, args, options };
    calls.push(call);
    const name = keyName(call);
    if (operation(call) === "put") {
      uploaded.set(name, fs.readFileSync(fileArg(call)));
      return;
    }
    if (preflight) {
      preflight = false;
      const error = new Error("The specified key does not exist");
      error.stderr = "The specified key does not exist";
      throw error;
    }
    fs.writeFileSync(fileArg(call), uploaded.get(name));
  };

  const result = seedR2({ isRemote: true, repoRoot, runCommand, log: quiet });
  assert.equal(result.releaseId, "c1-real-v2");
  assert.deepEqual(result.files, PUBLISH_ORDER);
  assert.equal(calls[0].executable, "npx");
  assert.equal(operation(calls[0]), "get");
  assert.equal(keyName(calls[0]), "manifest.json");
  assert.deepEqual(calls.slice(1, 8).map(operation), Array(7).fill("put"));
  assert.deepEqual(calls.slice(1, 8).map(keyName), PUBLISH_ORDER.slice(0, 7));
  assert.deepEqual(calls.slice(8, 15).map(operation), Array(7).fill("get"));
  assert.deepEqual(calls.slice(8, 15).map(keyName), PUBLISH_ORDER.slice(0, 7));
  assert.equal(operation(calls[15]), "put");
  assert.equal(keyName(calls[15]), "manifest.json");
  assert.equal(operation(calls[16]), "get");
  assert.equal(keyName(calls[16]), "manifest.json");
  assert.ok(calls.every((call) => call.args.includes("--remote")));
});

test("remote read-back mismatch fails the seed", (t) => {
  const repoRoot = createRepo(t);
  const uploaded = new Map();
  let preflight = true;
  assert.throws(
    () =>
      seedR2({
        isRemote: true,
        repoRoot,
        log: quiet,
        runCommand: (_executable, args) => {
          const call = { args };
          const name = keyName(call);
          if (operation(call) === "put") {
            uploaded.set(name, fs.readFileSync(fileArg(call)));
            return;
          }
          if (preflight) {
            preflight = false;
            throw new Error("The specified key does not exist");
          }
          const content = name === "graph.json" ? Buffer.from("corrupt") : uploaded.get(name);
          fs.writeFileSync(fileArg(call), content);
        },
      }),
    /Remote read-back mismatch for graph\.json/
  );
});
