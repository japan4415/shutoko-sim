import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const scriptDir = path.dirname(scriptPath);
const defaultRepoRoot = path.resolve(scriptDir, "../..");

export const RELEASE_ID_REGEX = /^[a-z0-9][a-z0-9.-]{0,63}$/;

const MANIFEST_ARTIFACT_NAMES = ["graph.json", "snap-index.json"];
const MANIFEST_ARTIFACT_ALLOWLIST = new Set([...MANIFEST_ARTIFACT_NAMES, "ramps.json"]);
const ENGINE_ARTIFACT_NAMES = [
  "shutoko_routing_bg.wasm",
  "shutoko_routing.js",
  "shutoko_routing.d.ts",
  "index.d.ts",
];

function sha256(content) {
  return crypto.createHash("sha256").update(content).digest("hex");
}

function contentTypeFor(fileName) {
  if (fileName.endsWith(".wasm")) return "application/wasm";
  if (fileName.endsWith(".json")) return "application/json";
  if (fileName.endsWith(".d.ts")) return "text/plain";
  if (fileName.endsWith(".js")) return "text/javascript";
  return "application/octet-stream";
}

function readRequiredFile(filePath, label) {
  if (!fs.existsSync(filePath)) {
    throw new Error(`Required artifact ${label} not found at ${filePath}`);
  }
  return fs.readFileSync(filePath);
}

function parseManifest(manifestPath) {
  const content = readRequiredFile(manifestPath, "manifest.json");
  let manifest;
  try {
    manifest = JSON.parse(content.toString("utf8"));
  } catch (error) {
    throw new Error(`manifest.json is not valid JSON: ${error.message}`);
  }

  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    throw new Error("manifest.json must contain a JSON object");
  }
  if (typeof manifest.releaseId !== "string" || !RELEASE_ID_REGEX.test(manifest.releaseId)) {
    throw new Error(
      "manifest.releaseId is required and must match /^[a-z0-9][a-z0-9.-]{0,63}$/"
    );
  }
  if (manifest.schemaVersion !== 1) {
    throw new Error("manifest.schemaVersion must be 1");
  }
  if (!Array.isArray(manifest.artifacts)) {
    throw new Error("manifest.artifacts must be an array");
  }
  return { manifest, content };
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function requiredString(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

function requiredStringArray(value, label) {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string" || item.length === 0)) {
    throw new Error(`${label} must be a non-empty string array`);
  }
  return value;
}

function optionalString(value, label) {
  if (value === undefined || value === null) return null;
  return requiredString(value, label);
}

export function canonicalRouteMemberships(value) {
  if (!Array.isArray(value)) {
    throw new Error("graph.routeMemberships must be an array");
  }
  return value.map((membership, membershipIndex) => {
    if (!isRecord(membership) || !Array.isArray(membership.segments)) {
      throw new Error(`graph.routeMemberships[${membershipIndex}] is invalid`);
    }
    return {
      membershipId: requiredString(membership.membershipId, "routeMemberships.membershipId"),
      routeId: requiredString(membership.routeId, "routeMemberships.routeId"),
      direction: requiredString(membership.direction, "routeMemberships.direction"),
      directionMappingVersion: requiredString(
        membership.directionMappingVersion,
        "routeMemberships.directionMappingVersion",
      ),
      segments: membership.segments.map((segment, segmentIndex) => {
        if (!isRecord(segment)) {
          throw new Error(
            `graph.routeMemberships[${membershipIndex}].segments[${segmentIndex}] is invalid`,
          );
        }
        return {
          segmentId: requiredString(segment.segmentId, "routeMemberships.segmentId"),
          sourceKind: requiredString(segment.sourceKind, "routeMemberships.sourceKind"),
          sourceRelationId: optionalString(
            segment.sourceRelationId,
            "routeMemberships.sourceRelationId",
          ),
          sourceSnapshotSha256: requiredString(
            segment.sourceSnapshotSha256,
            "routeMemberships.sourceSnapshotSha256",
          ),
          bindingEvidenceId: optionalString(
            segment.bindingEvidenceId,
            "routeMemberships.bindingEvidenceId",
          ),
          orderedEdgeIds: requiredStringArray(
            segment.orderedEdgeIds,
            "routeMemberships.orderedEdgeIds",
          ),
          orderedEdgeIdsSha256: requiredString(
            segment.orderedEdgeIdsSha256,
            "routeMemberships.orderedEdgeIdsSha256",
          ),
        };
      }),
    };
  });
}

export function routeMembershipsSha256(value) {
  return sha256(Buffer.from(JSON.stringify(canonicalRouteMemberships(value)), "utf8"));
}

function validateSchema4Contract(manifest, fixtureFiles) {
  if (manifest.graphSchemaVersion !== 4 && manifest.releaseId !== "all-real-v3") return;
  if (manifest.graphSchemaVersion !== 4) {
    throw new Error("manifest.graphSchemaVersion=4 is required for all-real-v3");
  }
  if (manifest.billingPairsVersion !== "v2") {
    throw new Error("manifest.billingPairsVersion=v2 is required for graph schema 4");
  }
  if (manifest.routePlanVersion !== 1) {
    throw new Error("manifest.routePlanVersion=1 is required for graph schema 4");
  }
  const expectedHash = requiredString(
    manifest.routeMembershipsSha256,
    "manifest.routeMembershipsSha256",
  );
  if (!/^[0-9a-f]{64}$/.test(expectedHash)) {
    throw new Error("manifest.routeMembershipsSha256 must be a lowercase SHA-256 value");
  }
  const graphFile = fixtureFiles.find((file) => file.name === "graph.json");
  if (graphFile === undefined) {
    throw new Error("graph.json is required for schema 4 contract validation");
  }
  let graph;
  try {
    graph = JSON.parse(graphFile.content.toString("utf8"));
  } catch (error) {
    throw new Error(`graph.json is not valid JSON: ${error.message}`);
  }
  if (!isRecord(graph) || graph.releaseId !== manifest.releaseId || graph.schemaVersion !== 4) {
    throw new Error("graph.json releaseId/schemaVersion does not match schema 4 manifest");
  }
  if (!Array.isArray(graph.billingPairs) || !Array.isArray(graph.routeMemberships)) {
    throw new Error("schema 4 graph requires billingPairs and routeMemberships arrays");
  }
  for (const [index, pair] of graph.billingPairs.entries()) {
    if (!isRecord(pair) || !["legacyRing", "radialReturn"].includes(pair.pairKind)) {
      throw new Error(`graph.billingPairs[${index}].pairKind is invalid`);
    }
    if (pair.pairKind === "radialReturn" && pair.routePlanVersion !== 1) {
      throw new Error(`graph.billingPairs[${index}].routePlanVersion must be 1`);
    }
  }
  const actualHash = routeMembershipsSha256(graph.routeMemberships);
  if (actualHash !== expectedHash) {
    throw new Error(
      `manifest.routeMembershipsSha256 mismatch: expected ${expectedHash}, got ${actualHash}`,
    );
  }
}

function validateManifestArtifacts(manifest, fixturesDir, log) {
  const seen = new Set();
  const files = [];

  for (const artifact of manifest.artifacts) {
    if (!artifact || typeof artifact !== "object" || Array.isArray(artifact)) {
      throw new Error("Every manifest artifact must be an object");
    }
    const artifactPath = artifact.path;
    if (
      typeof artifactPath !== "string" ||
      path.basename(artifactPath) !== artifactPath ||
      !MANIFEST_ARTIFACT_ALLOWLIST.has(artifactPath)
    ) {
      throw new Error(`Unsafe or unsupported manifest artifact path: ${String(artifactPath)}`);
    }
    if (seen.has(artifactPath)) {
      throw new Error(`Duplicate manifest artifact path: ${artifactPath}`);
    }
    seen.add(artifactPath);

    if (typeof artifact.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(artifact.sha256)) {
      throw new Error(`Invalid SHA-256 for ${artifactPath}`);
    }
    if (!Number.isSafeInteger(artifact.byteLength) || artifact.byteLength < 0) {
      throw new Error(`Invalid byteLength for ${artifactPath}`);
    }

    const filePath = path.join(fixturesDir, artifactPath);
    const content = readRequiredFile(filePath, artifactPath);
    const actualHash = sha256(content);
    if (actualHash !== artifact.sha256) {
      throw new Error(
        `SHA-256 mismatch for ${artifactPath}: expected ${artifact.sha256}, got ${actualHash}`
      );
    }
    if (content.length !== artifact.byteLength) {
      throw new Error(
        `ByteLength mismatch for ${artifactPath}: expected ${artifact.byteLength}, got ${content.length}`
      );
    }
    files.push({ name: artifactPath, path: filePath, content });
    log(`  ✓ Verified ${artifactPath}`);
  }

  const requiredArtifacts = manifest.releaseId.startsWith("all-real-")
    ? [...MANIFEST_ARTIFACT_NAMES, "ramps.json"]
    : MANIFEST_ARTIFACT_NAMES;
  for (const required of requiredArtifacts) {
    if (!seen.has(required)) {
      throw new Error(`manifest.artifacts is missing required artifact ${required}`);
    }
  }
  return files;
}

function buildEngineArtifacts(wasmDir) {
  return ENGINE_ARTIFACT_NAMES.map((name) => {
    const filePath = path.join(wasmDir, name);
    const content = readRequiredFile(filePath, name);
    return {
      file: { name, path: filePath, content },
      descriptor: {
        path: name,
        sha256: sha256(content),
        byteLength: content.length,
      },
    };
  });
}

function defaultRunCommand(executable, args, options) {
  return execFileSync(executable, args, options);
}

function commandErrorText(error) {
  return [error?.message, error?.stdout?.toString?.(), error?.stderr?.toString?.()]
    .filter(Boolean)
    .join("\n");
}

function isMissingObjectError(error) {
  return /specified key does not exist|object not found|nosuchkey|\b10007\b/i.test(
    commandErrorText(error)
  );
}

function wranglerArgs(operation, bucketName, releaseId, file, modeFlag) {
  const objectKey = `${bucketName}/releases/${releaseId}/${file.name}`;
  if (operation === "put") {
    return [
      "wrangler",
      "r2",
      "object",
      "put",
      objectKey,
      "--file",
      file.path,
      "--content-type",
      contentTypeFor(file.name),
      modeFlag,
    ];
  }
  return ["wrangler", "r2", "object", "get", objectKey, "--file", file.path, modeFlag];
}

function ensureRemoteReleaseIsNew({ runCommand, commandCwd, bucketName, releaseId, modeFlag }) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "shutoko-r2-preflight-"));
  const destination = path.join(tempDir, "manifest.json");
  const file = { name: "manifest.json", path: destination };
  try {
    try {
      runCommand("npx", wranglerArgs("get", bucketName, releaseId, file, modeFlag), {
        cwd: commandCwd,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch (error) {
      if (isMissingObjectError(error)) return;
      throw new Error(
        `Remote preflight failed closed for release ${releaseId}: ${commandErrorText(error)}`
      );
    }
    throw new Error(
      `Remote release ${releaseId} is already published; refusing to overwrite manifest.json`
    );
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
}

function uploadFile({ runCommand, commandCwd, bucketName, releaseId, modeFlag, modeLabel, file, log }) {
  const objectKey = `${bucketName}/releases/${releaseId}/${file.name}`;
  log(`Uploading ${file.path} to ${modeLabel} ${objectKey} (${contentTypeFor(file.name)})...`);
  try {
    runCommand("npx", wranglerArgs("put", bucketName, releaseId, file, modeFlag), {
      cwd: commandCwd,
      stdio: "inherit",
    });
  } catch (error) {
    throw new Error(`Failed to upload ${file.name} to ${modeLabel} R2: ${commandErrorText(error)}`);
  }
}

function verifyRemoteFiles({ runCommand, commandCwd, bucketName, releaseId, modeFlag, files, log }) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "shutoko-r2-readback-"));
  try {
    for (const expected of files) {
      const destination = path.join(tempDir, expected.name);
      const remoteFile = { name: expected.name, path: destination };
      try {
        runCommand("npx", wranglerArgs("get", bucketName, releaseId, remoteFile, modeFlag), {
          cwd: commandCwd,
          encoding: "utf8",
          stdio: ["ignore", "pipe", "pipe"],
        });
      } catch (error) {
        throw new Error(
          `Failed to read back ${expected.name} from remote R2: ${commandErrorText(error)}`
        );
      }
      const actual = readRequiredFile(destination, `remote read-back ${expected.name}`);
      if (
        actual.length !== expected.content.length ||
        sha256(actual) !== sha256(expected.content) ||
        !actual.equals(expected.content)
      ) {
        throw new Error(`Remote read-back mismatch for ${expected.name}`);
      }
      log(`  ✓ Read-back verified ${expected.name}`);
    }
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
}

export function seedR2({
  isRemote = false,
  repoRoot = defaultRepoRoot,
  runCommand = defaultRunCommand,
  log = console.log,
} = {}) {
  const fixturesDir = path.join(repoRoot, "fixtures/generated");
  const wasmDir = path.join(repoRoot, "dist/wasm");
  const commandCwd = path.join(repoRoot, "workers");
  const manifestPath = path.join(fixturesDir, "manifest.json");
  const { manifest, content: manifestContent } = parseManifest(manifestPath);
  const releaseId = manifest.releaseId;

  log(`Verifying artifacts for release: ${releaseId}`);
  const fixtureFiles = validateManifestArtifacts(manifest, fixturesDir, log);
  validateSchema4Contract(manifest, fixtureFiles);
  const engineEntries = buildEngineArtifacts(wasmDir);
  const engineArtifacts = engineEntries.map((entry) => entry.descriptor);
  const engineJsonPath = path.join(wasmDir, "engine.json");
  const engineContent = Buffer.from(
    `${JSON.stringify({ schemaVersion: 1, releaseId, artifacts: engineArtifacts }, null, 2)}\n`,
    "utf8"
  );
  fs.writeFileSync(engineJsonPath, engineContent);
  log(`  ✓ Generated engine.json (${engineArtifacts.length} artifacts)`);
  for (const artifact of engineArtifacts) {
    log(`      ${artifact.path}: sha256=${artifact.sha256} byteLength=${artifact.byteLength}`);
  }

  const engineFile = { name: "engine.json", path: engineJsonPath, content: engineContent };
  const manifestFile = { name: "manifest.json", path: manifestPath, content: manifestContent };
  const payloadFiles = [...fixtureFiles, ...engineEntries.map((entry) => entry.file)];
  const filesInPublishOrder = [...payloadFiles, engineFile, manifestFile];

  const modeFlag = isRemote ? "--remote" : "--local";
  const modeLabel = isRemote ? "remote" : "local";
  const bucketName = isRemote ? "shutoko-artifacts" : "shutoko-artifacts-preview";

  if (isRemote) {
    ensureRemoteReleaseIsNew({
      runCommand,
      commandCwd,
      bucketName,
      releaseId,
      modeFlag,
    });
  }

  log(`Seeding ${modeLabel} R2 bucket '${bucketName}' for release '${releaseId}'...`);
  const filesBeforePublish = [...payloadFiles, engineFile];
  for (const file of filesBeforePublish) {
    uploadFile({
      runCommand,
      commandCwd,
      bucketName,
      releaseId,
      modeFlag,
      modeLabel,
      file,
      log,
    });
  }

  if (isRemote) {
    verifyRemoteFiles({
      runCommand,
      commandCwd,
      bucketName,
      releaseId,
      modeFlag,
      files: filesBeforePublish,
      log,
    });
  }

  uploadFile({
    runCommand,
    commandCwd,
    bucketName,
    releaseId,
    modeFlag,
    modeLabel,
    file: manifestFile,
    log,
  });

  if (isRemote) {
    verifyRemoteFiles({
      runCommand,
      commandCwd,
      bucketName,
      releaseId,
      modeFlag,
      files: [manifestFile],
      log,
    });
  }

  log(`${modeLabel[0].toUpperCase()}${modeLabel.slice(1)} R2 seeding completed successfully.`);
  return { releaseId, files: filesInPublishOrder.map((file) => file.name) };
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  try {
    seedR2({ isRemote: process.argv.includes("--remote") });
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
