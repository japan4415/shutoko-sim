import { execSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../..");
const fixturesDir = path.join(repoRoot, "fixtures/generated");
const wasmDir = path.join(repoRoot, "dist/wasm");

const manifestPath = path.join(fixturesDir, "manifest.json");
if (!fs.existsSync(manifestPath)) {
  console.error(`manifest.json not found at ${manifestPath}`);
  process.exit(1);
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf-8"));
const releaseId = manifest.releaseId || "c1-real-v1";

console.log(`Verifying artifacts for release: ${releaseId}`);

// Verify manifest artifacts
for (const artifact of manifest.artifacts || []) {
  const filePath = path.join(fixturesDir, artifact.path);
  if (!fs.existsSync(filePath)) {
    console.error(`Required artifact ${artifact.path} not found at ${filePath}`);
    process.exit(1);
  }
  const content = fs.readFileSync(filePath);
  const hash = crypto.createHash("sha256").update(content).digest("hex");
  if (hash !== artifact.sha256) {
    console.error(
      `SHA-256 mismatch for ${artifact.path}: expected ${artifact.sha256}, got ${hash}`
    );
    process.exit(1);
  }
  if (content.length !== artifact.byteLength) {
    console.error(
      `ByteLength mismatch for ${artifact.path}: expected ${artifact.byteLength}, got ${content.length}`
    );
    process.exit(1);
  }
  console.log(`  ✓ Verified ${artifact.path}`);
}

const filesToUpload = [
  { name: "manifest.json", path: manifestPath },
  { name: "graph.json", path: path.join(fixturesDir, "graph.json") },
  { name: "snap-index.json", path: path.join(fixturesDir, "snap-index.json") },
];

const wasmFiles = [
  "shutoko_routing_bg.wasm",
  "shutoko_routing.js",
  "shutoko_routing.d.ts",
  "index.d.ts",
];

for (const wasmFile of wasmFiles) {
  const p = path.join(wasmDir, wasmFile);
  if (fs.existsSync(p)) {
    filesToUpload.push({ name: wasmFile, path: p });
  } else {
    console.warn(`  ! Note: WASM artifact ${wasmFile} not found at ${p} (skip if not built yet)`);
  }
}

console.log(`Seeding local R2 bucket 'shutoko-artifacts' for release '${releaseId}'...`);
for (const file of filesToUpload) {
  const r2Key = `shutoko-artifacts/releases/${releaseId}/${file.name}`;
  console.log(`Uploading ${file.path} to local ${r2Key}...`);
  try {
    execSync(
      `npx wrangler r2 object put "${r2Key}" --file "${file.path}" --local`,
      {
        cwd: path.resolve(__dirname, ".."),
        stdio: "inherit",
      }
    );
  } catch (err) {
    console.error(`Failed to upload ${file.name} to local R2:`, err);
    process.exit(1);
  }
}

console.log("Local R2 seeding completed successfully.");
