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
const MANIFEST_ARTIFACT_ALLOWLIST = new Set([
  ...MANIFEST_ARTIFACT_NAMES,
  "ramps.json",
  // all-real-v4 が manifest に結ぶ入力由来の成果物。
  "od-tariffs.json",
  "pair-candidates.json",
]);
const ENGINE_ARTIFACT_NAMES = [
  "shutoko_routing_bg.wasm",
  "shutoko_routing.js",
  "shutoko_routing.d.ts",
  "index.d.ts",
];

/** schema 4 graph が記録できる billingPairsVersion（v2=all-real-v3, v3=all-real-v4）。 */
const SCHEMA4_BILLING_PAIRS_VERSIONS = new Set(["v2", "v3"]);
/** billingPairsVersion=v3 が要求する tariffModelVersion。 */
const TARIFF_MODEL_VERSION = 1;

/**
 * R2 object の投入に要する wrangler CLI の最小版。
 * `wrangler r2 object put|get --file --content-type` が使える 4.x のうち
 * 最も古い版を下限とし、これ未満は機能不足として停止する。
 */
const MINIMUM_WRANGLER_VERSION = "4.0.0";

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

function optionalNonNegativeIntegerArray(value, label) {
  if (value === undefined || value === null) return undefined;
  if (!Array.isArray(value) || value.some((item) => !Number.isSafeInteger(item) || item < 0)) {
    throw new Error(`${label} must be a non-negative integer array`);
  }
  return value;
}

function optionalBoolean(value, label) {
  if (value === undefined || value === null) return undefined;
  if (typeof value !== "boolean") {
    throw new Error(`${label} must be a boolean`);
  }
  return value;
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
        const memberIndexes = optionalNonNegativeIntegerArray(
          segment.memberIndexes,
          "routeMemberships.memberIndexes",
        );
        const memberOrderMatchesRelation = optionalBoolean(
          segment.memberOrderMatchesRelation,
          "routeMemberships.memberOrderMatchesRelation",
        );
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
          ...(memberIndexes === undefined ? {} : { memberIndexes }),
          ...(memberOrderMatchesRelation === undefined
            ? {}
            : { memberOrderMatchesRelation }),
        };
      }),
    };
  });
}

export function routeMembershipsSha256(value) {
  return sha256(Buffer.from(JSON.stringify(canonicalRouteMemberships(value)), "utf8"));
}

function validateSchema4Contract(manifest, fixtureFiles) {
  if (manifest.graphSchemaVersion !== 4) return;
  if (!SCHEMA4_BILLING_PAIRS_VERSIONS.has(manifest.billingPairsVersion)) {
    throw new Error(
      `manifest.billingPairsVersion must be one of ${[...SCHEMA4_BILLING_PAIRS_VERSIONS].join(", ")} for graph schema 4`,
    );
  }
  if (manifest.billingPairsVersion === "v3" && manifest.tariffModelVersion !== TARIFF_MODEL_VERSION) {
    throw new Error(
      `manifest.tariffModelVersion=${TARIFF_MODEL_VERSION} is required for billingPairsVersion=v3`,
    );
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

/**
 * PATH 上の実行ファイルを名前に解決する（npx へはフォールバックしない）。
 * 実行可否は preflight の `wrangler --version` が実際に起動できるかで確定する
 * ため、ここでは「その名前のファイルがあるか」までだけを見る。
 */
function resolveOnPath(name, env = process.env) {
  if (name.includes(path.sep)) {
    return isExecutableFile(name) ? path.resolve(name) : null;
  }
  for (const dir of (env.PATH ?? "").split(path.delimiter).filter(Boolean)) {
    const candidate = path.join(dir, name);
    if (isExecutableFile(candidate)) return candidate;
  }
  return null;
}

function isExecutableFile(candidate) {
  try {
    return fs.statSync(candidate).isFile();
  } catch {
    return false;
  }
}

/** wrangler の実行ファイルを WRANGLER_BIN → PATH の順で解決する。 */
export function resolveWranglerBinary({ env = process.env, cwd = defaultRepoRoot } = {}) {
  const override = env.WRANGLER_BIN;
  if (override !== undefined && override !== "") {
    const resolved = resolveOnPath(override, env);
    if (resolved === null) {
      throw new Error(
        `WRANGLER_BIN=${override} is not an executable file; fix the override or unset it`,
      );
    }
    return { binary: resolved, source: "WRANGLER_BIN" };
  }
  const onPath = resolveOnPath("wrangler", env);
  if (onPath !== null) {
    return { binary: onPath, source: "PATH" };
  }
  throw new Error(
    "wrangler was not found on PATH; install it (npm --prefix workers ci adds workers/node_modules/.bin to PATH) or set WRANGLER_BIN. npx is not used as a fallback.",
  );
}

/** workers/package-lock.json が固定する wrangler 版。 */
export function pinnedWranglerVersion(lockPath) {
  let lock;
  try {
    lock = JSON.parse(fs.readFileSync(lockPath, "utf8"));
  } catch (error) {
    throw new Error(`cannot read the wrangler pin from ${lockPath}: ${error.message}`);
  }
  const resolved = lock?.packages?.["node_modules/wrangler"]?.version;
  if (typeof resolved === "string" && resolved !== "") return resolved;
  const declared = lock?.packages?.[""]?.devDependencies?.wrangler;
  if (typeof declared === "string") return declared.replace(/^[\^~>=<\s]*/, "");
  throw new Error(`workers/package-lock.json does not pin wrangler (${lockPath})`);
}

function compareVersions(left, right) {
  const parse = (value) => String(value).split(".").map((part) => Number.parseInt(part, 10) || 0);
  const a = parse(left);
  const b = parse(right);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const diff = (a[index] ?? 0) - (b[index] ?? 0);
    if (diff !== 0) return diff < 0 ? -1 : 1;
  }
  return 0;
}

function readWranglerVersion({ runCommand, commandCwd, binary }) {
  let stdout;
  try {
    stdout = runCommand(binary, ["--version"], {
      cwd: commandCwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    throw new Error(
      `wrangler preflight failed: \`${binary} --version\` did not run: ${commandErrorText(error)}`,
    );
  }
  const match = /\d+\.\d+\.\d+/.exec(String(stdout ?? ""));
  if (match === null) {
    throw new Error(
      `wrangler preflight failed: \`${binary} --version\` printed no version: ${String(stdout ?? "").trim()}`,
    );
  }
  return match[0];
}

/**
 * 投入前に wrangler の実行ファイルと版を確定する。
 * - 実行ファイルは WRANGLER_BIN → PATH（npx フォールバックなし）
 * - workers/package-lock.json の固定版と並べ、差があれば必ずログへ出す
 * - 固定版未満（機能不足）と、SHUTOKO_REQUIRE_PINNED_WRANGLER=1 時の不一致は停止
 */
export function preflightWrangler({ runCommand, commandCwd, repoRoot, env = process.env, log }) {
  const { binary, source } = resolveWranglerBinary({ env });
  const pinned = pinnedWranglerVersion(path.join(repoRoot, "workers", "package-lock.json"));
  const version = readWranglerVersion({ runCommand, commandCwd, binary });
  const matchesPin = version === pinned;
  log(`  ✓ wrangler ${version} (${source}: ${binary})`);
  log(`      pinned ${pinned} (workers/package-lock.json)`);
  if (!matchesPin) {
    const detail = `wrangler ${version} differs from the pinned ${pinned}`;
    if (env.SHUTOKO_REQUIRE_PINNED_WRANGLER === "1") {
      throw new Error(
        `${detail}; SHUTOKO_REQUIRE_PINNED_WRANGLER=1 requires an exact match. Install the pinned version or drop the variable.`,
      );
    }
    log(`      ! version differs: ${detail} (continuing; the pinned build stays reproducible)`);
  }
  if (compareVersions(version, MINIMUM_WRANGLER_VERSION) < 0) {
    throw new Error(
      `wrangler ${version} is older than the required ${MINIMUM_WRANGLER_VERSION} (r2 object put/get); install a newer wrangler`,
    );
  }
  return { binary, source, version, pinnedVersion: pinned, matchesPin };
}

function wranglerArgs(operation, bucketName, releaseId, file, modeFlag) {
  const objectKey = `${bucketName}/releases/${releaseId}/${file.name}`;
  if (operation === "put") {
    return [
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
  return ["r2", "object", "get", objectKey, "--file", file.path, modeFlag];
}

function ensureRemoteReleaseIsNew({
  runCommand,
  commandCwd,
  wrangler,
  bucketName,
  releaseId,
  modeFlag,
}) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "shutoko-r2-preflight-"));
  const destination = path.join(tempDir, "manifest.json");
  const file = { name: "manifest.json", path: destination };
  try {
    try {
      runCommand(wrangler, wranglerArgs("get", bucketName, releaseId, file, modeFlag), {
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

function uploadFile({
  runCommand,
  commandCwd,
  wrangler,
  bucketName,
  releaseId,
  modeFlag,
  modeLabel,
  file,
  log,
}) {
  const objectKey = `${bucketName}/releases/${releaseId}/${file.name}`;
  log(`Uploading ${file.path} to ${modeLabel} ${objectKey} (${contentTypeFor(file.name)})...`);
  try {
    runCommand(wrangler, wranglerArgs("put", bucketName, releaseId, file, modeFlag), {
      cwd: commandCwd,
      stdio: "inherit",
    });
  } catch (error) {
    throw new Error(`Failed to upload ${file.name} to ${modeLabel} R2: ${commandErrorText(error)}`);
  }
}

function verifyRemoteFiles({
  runCommand,
  commandCwd,
  wrangler,
  bucketName,
  releaseId,
  modeFlag,
  files,
  log,
}) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "shutoko-r2-readback-"));
  try {
    for (const expected of files) {
      const destination = path.join(tempDir, expected.name);
      const remoteFile = { name: expected.name, path: destination };
      try {
        runCommand(wrangler, wranglerArgs("get", bucketName, releaseId, remoteFile, modeFlag), {
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
  env = process.env,
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

  // ローカルの検証（manifest / graph 契約 / engine 成果物）が全部通ってから wrangler を
  // 触る。成果物が欠けていれば wrangler を実行せずに停止する。
  log("Checking the wrangler CLI before any upload...");
  const wranglerInfo = preflightWrangler({ runCommand, commandCwd, repoRoot, env, log });
  const wrangler = wranglerInfo.binary;

  if (isRemote) {
    ensureRemoteReleaseIsNew({
      runCommand,
      commandCwd,
      wrangler,
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
      wrangler,
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
      wrangler,
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
    wrangler,
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
      wrangler,
      bucketName,
      releaseId,
      modeFlag,
      files: [manifestFile],
      log,
    });
  }

  log(`${modeLabel[0].toUpperCase()}${modeLabel.slice(1)} R2 seeding completed successfully.`);
  return {
    releaseId,
    files: filesInPublishOrder.map((file) => file.name),
    wrangler: wranglerInfo,
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  try {
    seedR2({ isRemote: process.argv.includes("--remote") });
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
