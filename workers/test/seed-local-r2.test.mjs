import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "vitest";
import { seedR2 } from "../scripts/seed-local-r2.mjs";

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

test("all-real-v1 requires ramps.json while c1-real-v1/v2 remain backward compatible", (t) => {
  for (const releaseId of ["c1-real-v1", "c1-real-v2"]) {
    const repoRoot = createRepo(t, { releaseId });
    assert.doesNotThrow(() => seedR2({ repoRoot, runCommand: () => {}, log: quiet }));
  }

  const missingRampsRoot = createRepo(t, { releaseId: "all-real-v1" });
  assert.throws(
    () => seedR2({ repoRoot: missingRampsRoot, runCommand: () => assert.fail("must not publish"), log: quiet }),
    /missing required artifact ramps\.json/
  );
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
