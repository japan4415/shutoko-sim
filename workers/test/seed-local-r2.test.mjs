import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "vitest";
import generatedGraph from "../../fixtures/generated/graph.json?raw";
import generatedManifest from "../../fixtures/generated/manifest.json?raw";
import {
  pinnedWranglerVersion,
  preflightWrangler,
  resolveWranglerBinary,
  routeMembershipsSha256,
  seedR2,
} from "../scripts/seed-local-r2.mjs";

const FIXTURE_NAMES = ["graph.json", "snap-index.json"];
const ENGINE_NAMES = [
  "shutoko_routing_bg.wasm",
  "shutoko_routing.js",
  "shutoko_routing.d.ts",
  "index.d.ts",
];
const PUBLISH_ORDER = [...FIXTURE_NAMES, ...ENGINE_NAMES, "engine.json", "manifest.json"];
/** CI (npm run) は workers/node_modules/.bin を PATH へ足すので、lock の固定版と一致する。 */
const PINNED_WRANGLER = "4.131.0";

function sha256(content) {
  return crypto.createHash("sha256").update(content).digest("hex");
}

/** 実行可能な空ファイルを作る。実体は runCommand がスタブなので実行しない。 */
function fakeWrangler(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "seed-r2-wrangler-"));
  t.onTestFinished(() => fs.rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, "wrangler");
  fs.writeFileSync(file, "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  return file;
}

function isVersionCall(call) {
  return Array.isArray(call.args) && call.args.length === 1 && call.args[0] === "--version";
}

/** `wrangler --version` だけ版を返し、それ以外の実行は spy 側へ渡す。 */
function versionSpy(spy, version = PINNED_WRANGLER) {
  return (executable, args, options) => {
    if (Array.isArray(args) && args.length === 1 && args[0] === "--version") {
      return `${version}\n`;
    }
    return spy(executable, args, options);
  };
}

/** 全呼び出し（`--version` を含む）を記録し、`--version` には版を返す。 */
function recordingSpy(version = PINNED_WRANGLER) {
  const calls = [];
  const run = (executable, args, options) => {
    const call = { executable, args, options };
    calls.push(call);
    if (isVersionCall(call)) return `${version}\n`;
    return undefined;
  };
  return { calls, run };
}

function writePin(workersDir, version) {
  fs.writeFileSync(
    path.join(workersDir, "package-lock.json"),
    `${JSON.stringify({
      lockfileVersion: 3,
      packages: {
        "": { devDependencies: { wrangler: version } },
        "node_modules/wrangler": { version },
      },
    })}\n`,
  );
}

function createRepo(
  t,
  { releaseId = "c1-real-v2", manifestTransform, pinnedWrangler = PINNED_WRANGLER } = {},
) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "seed-r2-test-"));
  t.onTestFinished(() => fs.rmSync(repoRoot, { recursive: true, force: true }));
  const fixturesDir = path.join(repoRoot, "fixtures/generated");
  const wasmDir = path.join(repoRoot, "dist/wasm");
  const workersDir = path.join(repoRoot, "workers");
  fs.mkdirSync(fixturesDir, { recursive: true });
  fs.mkdirSync(wasmDir, { recursive: true });
  fs.mkdirSync(workersDir, { recursive: true });
  writePin(workersDir, pinnedWrangler);

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

/**
 * graph schema 4 の release を再現する。billingPairsVersion により v3（all-real-v4）か
 * v2（all-real-v3, rollback 先）かを表し、後者には od-tariffs / pair-candidates を結ばない。
 */
function createSchema4Repo(
  t,
  {
    releaseId = "all-real-v3",
    billingPairsVersion = "v2",
    withV4Artifacts = false,
    manifestTransform,
  } = {},
) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "seed-r2-schema4-test-"));
  t.onTestFinished(() => fs.rmSync(repoRoot, { recursive: true, force: true }));
  const fixturesDir = path.join(repoRoot, "fixtures/generated");
  const wasmDir = path.join(repoRoot, "dist/wasm");
  const workersDir = path.join(repoRoot, "workers");
  fs.mkdirSync(fixturesDir, { recursive: true });
  fs.mkdirSync(wasmDir, { recursive: true });
  fs.mkdirSync(workersDir, { recursive: true });
  writePin(workersDir, PINNED_WRANGLER);

  const edgeIds = ["edge:main"];
  const graph = {
    schemaVersion: 4,
    releaseId,
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
  const snapContent = Buffer.from(`{"schemaVersion":2,"releaseId":"${releaseId}","nodes":[]}\n`);
  const rampsContent = Buffer.from(`{"schemaVersion":1,"releaseId":"${releaseId}","ramps":[]}\n`);
  const tariffsContent = Buffer.from(`{"version":3,"releaseId":"${releaseId}"}\n`);
  const candidatesContent = Buffer.from(`{"schemaVersion":2,"releaseId":"${releaseId}"}\n`);
  fs.writeFileSync(path.join(fixturesDir, "graph.json"), graphContent);
  fs.writeFileSync(path.join(fixturesDir, "snap-index.json"), snapContent);
  fs.writeFileSync(path.join(fixturesDir, "ramps.json"), rampsContent);
  const files = [
    { name: "graph.json", content: graphContent },
    { name: "snap-index.json", content: snapContent },
    { name: "ramps.json", content: rampsContent },
  ];
  if (withV4Artifacts) {
    fs.writeFileSync(path.join(fixturesDir, "od-tariffs.json"), tariffsContent);
    fs.writeFileSync(path.join(fixturesDir, "pair-candidates.json"), candidatesContent);
    files.push({ name: "od-tariffs.json", content: tariffsContent });
    files.push({ name: "pair-candidates.json", content: candidatesContent });
  }
  for (const name of ENGINE_NAMES) {
    fs.writeFileSync(path.join(wasmDir, name), Buffer.from(`${name}-content`));
  }
  const artifacts = files.map(({ name, content }) => ({
    path: name,
    sha256: sha256(content),
    byteLength: content.length,
  }));
  let manifest = {
    schemaVersion: 1,
    releaseId,
    graphSchemaVersion: 4,
    routePlanVersion: 1,
    billingPairsVersion,
    ...(billingPairsVersion === "v3" ? { tariffModelVersion: 1 } : {}),
    routeMembershipsSha256: routeMembershipsSha256(graph.routeMemberships),
    artifacts,
  };
  if (manifestTransform) manifest = manifestTransform(manifest);
  fs.writeFileSync(path.join(fixturesDir, "manifest.json"), `${JSON.stringify(manifest)}\n`);
  return repoRoot;
}

// 引数は ["r2", "object", "put" | "get", "<bucket>/releases/<id>/<name>", ...]。
function operation(call) {
  return call.args[2];
}

function keyName(call) {
  return call.args[3].split("/").at(-1);
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
      /manifest\.releaseId is required/,
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
    () =>
      seedR2({
        repoRoot,
        runCommand: versionSpy(() => assert.fail("must not upload")),
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
      }),
    /Unsafe or unsupported manifest artifact path/,
  );
});

test("all-real releases require ramps.json while c1-real-v1/v2 remain backward compatible", (t) => {
  for (const releaseId of ["c1-real-v1", "c1-real-v2"]) {
    const repoRoot = createRepo(t, { releaseId });
    assert.doesNotThrow(() =>
      seedR2({
        repoRoot,
        runCommand: versionSpy(() => {}),
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
      }),
    );
  }

  for (const releaseId of ["all-real-v1", "all-real-v2"]) {
    const missingRampsRoot = createRepo(t, { releaseId });
    assert.throws(
      () =>
        seedR2({
          repoRoot: missingRampsRoot,
          runCommand: versionSpy(() => assert.fail("must not publish")),
          env: { WRANGLER_BIN: fakeWrangler(t) },
          log: quiet,
        }),
      /missing required artifact ramps\.json/,
    );
  }
});

test("schema 4 release metadata and route membership hash are verified before seeding", (t) => {
  const repoRoot = createSchema4Repo(t);
  const spy = recordingSpy();
  assert.doesNotThrow(() =>
    seedR2({ repoRoot, runCommand: spy.run, env: { WRANGLER_BIN: fakeWrangler(t) }, log: quiet }),
  );
  // wrangler --version 1 + manifest artifacts 3 + engine 4 + engine.json + manifest.json
  assert.equal(spy.calls.length, 10);
  assert.equal(spy.calls.filter(isVersionCall).length, 1);
  assert.ok(spy.calls.every((call) => call.executable === spy.calls[0].executable));

  const manifestPath = path.join(repoRoot, "fixtures/generated/manifest.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  manifest.routeMembershipsSha256 = "b".repeat(64);
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest)}\n`);
  assert.throws(
    () =>
      seedR2({
        repoRoot,
        runCommand: versionSpy(() => assert.fail("must not publish")),
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
      }),
    /routeMembershipsSha256 mismatch/,
  );
});

test("all-real-v4 の billingPairsVersion=v3 と tariffModelVersion=1 を検証してから投入する", (t) => {
  const repoRoot = createSchema4Repo(t, {
    releaseId: "all-real-v4",
    billingPairsVersion: "v3",
    withV4Artifacts: true,
  });
  const spy = recordingSpy();
  const result = seedR2({
    repoRoot,
    runCommand: spy.run,
    env: { WRANGLER_BIN: fakeWrangler(t) },
    log: quiet,
  });
  assert.equal(result.releaseId, "all-real-v4");
  // od-tariffs.json と pair-candidates.json も payload として先に投入する。
  assert.deepEqual(result.files, [
    "graph.json",
    "snap-index.json",
    "ramps.json",
    "od-tariffs.json",
    "pair-candidates.json",
    ...ENGINE_NAMES,
    "engine.json",
    "manifest.json",
  ]);
  const puts = spy.calls.filter((call) => !isVersionCall(call));
  assert.equal(puts.length, 11);
  assert.deepEqual(puts.map(keyName), result.files);
  // npx ではなく解決済みの wrangler を "r2 object put" として直接呼ぶ。
  assert.ok(puts.every((call) => call.executable === puts[0].executable));
  assert.ok(puts[0].executable.endsWith("wrangler"));
  assert.deepEqual(puts[0].args.slice(0, 3), ["r2", "object", "put"]);
});

test("未知の billingPairsVersion と tariffModelVersion 欠落は投入前に停止する", (t) => {
  const unknownVersion = createSchema4Repo(t, {
    releaseId: "all-real-v4",
    billingPairsVersion: "v9",
    withV4Artifacts: true,
  });
  assert.throws(
    () =>
      seedR2({
        repoRoot: unknownVersion,
        runCommand: versionSpy(() => assert.fail("must not publish")),
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
      }),
    /billingPairsVersion must be one of v2, v3/,
  );

  const missingTariffModel = createSchema4Repo(t, {
    releaseId: "all-real-v4",
    billingPairsVersion: "v3",
    withV4Artifacts: true,
    manifestTransform: (manifest) => {
      delete manifest.tariffModelVersion;
      return manifest;
    },
  });
  assert.throws(
    () =>
      seedR2({
        repoRoot: missingTariffModel,
        runCommand: versionSpy(() => assert.fail("must not publish")),
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
      }),
    /tariffModelVersion=1 is required for billingPairsVersion=v3/,
  );
});

test("generated graph and manifest use the same route membership hash", () => {
  const graph = JSON.parse(generatedGraph);
  const manifest = JSON.parse(generatedManifest);
  assert.equal(graph.schemaVersion, 4);
  assert.equal(manifest.releaseId, "all-real-v4");
  assert.equal(manifest.billingPairsVersion, "v3");
  assert.equal(manifest.tariffModelVersion, 1);
  assert.match(manifest.routeMembershipsSha256, /^[0-9a-f]{64}$/);
  assert.equal(routeMembershipsSha256(graph.routeMemberships), manifest.routeMembershipsSha256);
});

test("all four engine artifacts are required in local and remote modes", (t) => {
  for (const isRemote of [false, true]) {
    const repoRoot = createRepo(t);
    fs.rmSync(path.join(repoRoot, "dist/wasm/index.d.ts"));
    let calls = 0;
    assert.throws(
      () =>
        seedR2({
          isRemote,
          repoRoot,
          runCommand: versionSpy(() => calls++),
          env: { WRANGLER_BIN: fakeWrangler(t) },
          log: quiet,
        }),
      /Required artifact index\.d\.ts not found/,
    );
    // engine 成果物が欠ければ wrangler にも触れない。
    assert.equal(calls, 0);
  }
});

test("local seed uses argument arrays, publishes manifest last, and permits reruns", (t) => {
  const repoRoot = createRepo(t);
  const spy = recordingSpy();
  const env = { WRANGLER_BIN: fakeWrangler(t) };

  seedR2({ repoRoot, runCommand: spy.run, env, log: quiet });
  seedR2({ repoRoot, runCommand: spy.run, env, log: quiet });

  assert.equal(spy.calls.filter(isVersionCall).length, 2);
  const objects = spy.calls.filter((call) => !isVersionCall(call));
  const batches = [objects.slice(0, 8), objects.slice(8)];
  for (const batch of batches) {
    assert.deepEqual(batch.map(operation), Array(8).fill("put"));
    assert.deepEqual(batch.map(keyName), PUBLISH_ORDER);
    assert.ok(batch.every((call) => call.executable === objects[0].executable));
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
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
        runCommand: versionSpy((executable, args) => existingCalls.push({ executable, args })),
      }),
    /already published; refusing to overwrite/,
  );
  const existingObjects = existingCalls.filter((call) => !isVersionCall(call));
  assert.equal(existingObjects.length, 1);
  assert.equal(operation(existingObjects[0]), "get");
  assert.equal(keyName(existingObjects[0]), "manifest.json");

  const errorRoot = createRepo(t);
  let errorCalls = 0;
  assert.throws(
    () =>
      seedR2({
        isRemote: true,
        repoRoot: errorRoot,
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
        runCommand: versionSpy(() => {
          errorCalls += 1;
          throw new Error("authentication unavailable");
        }),
      }),
    /Remote preflight failed closed/,
  );
  assert.equal(errorCalls, 1);
});

test("remote seed publishes payload then engine then manifest and verifies all eight objects", (t) => {
  const repoRoot = createRepo(t);
  const spy = recordingSpy();
  const calls = spy.calls;
  const uploaded = new Map();
  let preflight = true;
  const runCommand = (executable, args, options) => {
    const output = spy.run(executable, args, options);
    if (output !== undefined) return output;
    const call = { executable, args, options };
    const name = keyName(call);
    if (operation(call) === "put") {
      uploaded.set(name, fs.readFileSync(fileArg(call)));
      return undefined;
    }
    if (preflight) {
      preflight = false;
      const error = new Error("The specified key does not exist");
      error.stderr = "The specified key does not exist";
      throw error;
    }
    fs.writeFileSync(fileArg(call), uploaded.get(name));
    return undefined;
  };

  const result = seedR2({
    isRemote: true,
    repoRoot,
    runCommand,
    env: { WRANGLER_BIN: fakeWrangler(t) },
    log: quiet,
  });
  assert.equal(result.releaseId, "c1-real-v2");
  assert.deepEqual(result.files, PUBLISH_ORDER);
  assert.equal(calls.filter(isVersionCall).length, 1);
  const objects = calls.filter((call) => !isVersionCall(call));
  // 0: manifest の存在 preflight (get) / 1-7: payload put / 8-14: read-back get /
  // 15: manifest put / 16: manifest read-back get
  assert.ok(objects.every((call) => call.executable === objects[0].executable));
  assert.ok(objects[0].executable.endsWith("wrangler"));
  assert.equal(operation(objects[0]), "get");
  assert.equal(keyName(objects[0]), "manifest.json");
  assert.deepEqual(objects.slice(1, 8).map(operation), Array(7).fill("put"));
  assert.deepEqual(objects.slice(1, 8).map(keyName), PUBLISH_ORDER.slice(0, 7));
  assert.deepEqual(objects.slice(8, 15).map(operation), Array(7).fill("get"));
  assert.deepEqual(objects.slice(8, 15).map(keyName), PUBLISH_ORDER.slice(0, 7));
  assert.equal(operation(objects[15]), "put");
  assert.equal(keyName(objects[15]), "manifest.json");
  assert.equal(operation(objects[16]), "get");
  assert.equal(keyName(objects[16]), "manifest.json");
  assert.ok(objects.every((call) => call.args.includes("--remote")));
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
        env: { WRANGLER_BIN: fakeWrangler(t) },
        log: quiet,
        runCommand: versionSpy((_executable, args) => {
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
        }),
      }),
    /Remote read-back mismatch for graph\.json/,
  );
});

test("wrangler は WRANGLER_BIN を優先し、PATH 探しで解決し、npx にはフォールバックしない", (t) => {
  const override = fakeWrangler(t);
  assert.deepEqual(resolveWranglerBinary({ env: { WRANGLER_BIN: override, PATH: "" } }), {
    binary: override,
    source: "WRANGLER_BIN",
  });
  assert.deepEqual(resolveWranglerBinary({ env: { WRANGLER_BIN: override, PATH: "/nonexistent" } }), {
    binary: override,
    source: "WRANGLER_BIN",
  });
  const onPathDir = path.dirname(fakeWrangler(t));
  assert.deepEqual(resolveWranglerBinary({ env: { PATH: onPathDir } }), {
    binary: path.join(onPathDir, "wrangler"),
    source: "PATH",
  });
  assert.throws(
    () => resolveWranglerBinary({ env: { PATH: "/nonexistent" } }),
    /npx is not used as a fallback/,
  );
  // メッセージは実際の判定（ファイルであること）に合わせる。
  assert.throws(
    () => resolveWranglerBinary({ env: { WRANGLER_BIN: path.join(onPathDir, "absent"), PATH: onPathDir } }),
    /WRANGLER_BIN=.* is not a file \(resolved to /,
  );
  assert.throws(
    () => resolveWranglerBinary({ env: { WRANGLER_BIN: onPathDir, PATH: "" } }),
    /WRANGLER_BIN=.* is not a file/,
  );

  // 相対パスは渡した cwd を基準に解決する（runbook の例は repo root 相対）。
  const relative = path.relative(process.cwd(), override);
  assert.deepEqual(
    resolveWranglerBinary({ env: { WRANGLER_BIN: relative, PATH: "" }, cwd: process.cwd() }),
    { binary: override, source: "WRANGLER_BIN" },
  );
});

test("preflight は lock の固定版と並べ、差を表示し、固定を要求する環境変数では停止する", (t) => {
  const repoRoot = createRepo(t);
  const wrangler = fakeWrangler(t);
  const lines = [];
  const info = preflightWrangler({
    runCommand: () => "4.131.0\n",
    commandCwd: path.join(repoRoot, "workers"),
    repoRoot,
    env: { WRANGLER_BIN: wrangler },
    log: (line) => lines.push(line),
  });
  assert.deepEqual(info, {
    binary: wrangler,
    source: "WRANGLER_BIN",
    version: "4.131.0",
    pinnedVersion: PINNED_WRANGLER,
    matchesPin: true,
  });
  assert.ok(lines.some((line) => line.includes("wrangler 4.131.0")));
  assert.ok(lines.some((line) => line.includes(`pinned ${PINNED_WRANGLER}`)));

  const mismatchLines = [];
  const mismatch = preflightWrangler({
    runCommand: () => "4.92.0\n",
    commandCwd: path.join(repoRoot, "workers"),
    repoRoot,
    env: { WRANGLER_BIN: wrangler },
    log: (line) => mismatchLines.push(line),
  });
  assert.equal(mismatch.matchesPin, false);
  assert.ok(
    mismatchLines.some((line) => line.includes("wrangler 4.92.0 differs from the pinned 4.131.0")),
  );

  assert.throws(
    () =>
      preflightWrangler({
        runCommand: () => "4.92.0\n",
        commandCwd: path.join(repoRoot, "workers"),
        repoRoot,
        env: { WRANGLER_BIN: wrangler, SHUTOKO_REQUIRE_PINNED_WRANGLER: "1" },
        log: quiet,
      }),
    /SHUTOKO_REQUIRE_PINNED_WRANGLER=1 requires an exact match/,
  );

  assert.throws(
    () =>
      preflightWrangler({
        runCommand: () => "3.99.0\n",
        commandCwd: path.join(repoRoot, "workers"),
        repoRoot,
        env: { WRANGLER_BIN: wrangler },
        log: quiet,
      }),
    /older than the required 4\.0\.0/,
  );
  assert.throws(
    () =>
      preflightWrangler({
        runCommand: () => "no version here\n",
        commandCwd: path.join(repoRoot, "workers"),
        repoRoot,
        env: { WRANGLER_BIN: wrangler },
        log: quiet,
      }),
    /printed no version/,
  );
});

test("workers/package-lock.json の固定版を読む", (t) => {
  const repoRoot = createRepo(t, { pinnedWrangler: "4.99.1" });
  assert.equal(
    pinnedWranglerVersion(path.join(repoRoot, "workers", "package-lock.json")),
    "4.99.1",
  );
  const rangeOnly = createRepo(t);
  fs.writeFileSync(
    path.join(rangeOnly, "workers/package-lock.json"),
    `${JSON.stringify({ packages: { "": { devDependencies: { wrangler: "^4.131.0" } } } })}\n`,
  );
  assert.equal(pinnedWranglerVersion(path.join(rangeOnly, "workers/package-lock.json")), "4.131.0");
  assert.throws(
    () => pinnedWranglerVersion(path.join(rangeOnly, "workers/package-lock.json.absent")),
    /cannot read the wrangler pin/,
  );
});
