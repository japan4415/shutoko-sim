import { execSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../..");
const fixturesDir = path.join(repoRoot, "fixtures/generated");
const wasmDir = path.join(repoRoot, "dist/wasm");

const isRemote = process.argv.includes("--remote");

function contentTypeFor(fileName) {
  if (fileName.endsWith(".wasm")) return "application/wasm";
  if (fileName.endsWith(".json")) return "application/json";
  if (fileName.endsWith(".d.ts")) return "text/plain";
  if (fileName.endsWith(".js")) return "text/javascript";
  return "application/octet-stream";
}

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

const modeFlag = isRemote ? "--remote" : "--local";
const modeLabel = isRemote ? "remote" : "local";
// wrangler のローカル miniflare は preview_bucket_name を状態ディレクトリ名に使うため、
// --local の投入先は preview バケットへ合わせる（本番名 shutoko-artifacts は使わない）。
// --remote は本番バケットへの明示的な投入なので bucket_name を使う。
const bucketName = isRemote ? "shutoko-artifacts" : "shutoko-artifacts-preview";
console.log(`Seeding ${modeLabel} R2 bucket '${bucketName}' for release '${releaseId}'...`);
for (const file of filesToUpload) {
  const r2Key = `${bucketName}/releases/${releaseId}/${file.name}`;
  const contentType = contentTypeFor(file.name);
  console.log(`Uploading ${file.path} to ${modeLabel} ${r2Key} (${contentType})...`);
  try {
    execSync(
      `npx wrangler r2 object put "${r2Key}" --file "${file.path}" --content-type "${contentType}" ${modeFlag}`,
      {
        cwd: path.resolve(__dirname, ".."),
        stdio: "inherit",
      }
    );
  } catch (err) {
    console.error(`Failed to upload ${file.name} to ${modeLabel} R2:`, err);
    process.exit(1);
  }
}

console.log(`${modeLabel[0].toUpperCase()}${modeLabel.slice(1)} R2 seeding completed successfully.`);
