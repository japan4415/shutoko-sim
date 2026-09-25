// release allowlist の同期と rollback の手順を固定する。
//
// 異常時は Web の既定（web/src/worker/artifact-hashes.ts の DEFAULT_RELEASE_ID）と
// Worker の allowlist（workers/wrangler.toml の ALLOWED_RELEASES）を同時に前の版へ
// 戻す。片方だけ戻すと「Worker は配るが Web が取得できない」「Web は要求するが
// Worker が 404」になる。ここでは両者が同じ集合を持ち、既定が Worker 側に通ることを
// 検査し、前の版が allowlist に残ってる（= 戻せる）ことを確認する。
import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import { DEFAULT_RELEASE_ID, KNOWN_RELEASES } from "../src/worker/artifact-hashes";

const root = new URL("../../", import.meta.url);

/** workers/wrangler.toml の ALLOWED_RELEASES を読む。 */
async function workerAllowedReleases(): Promise<string[]> {
  const toml = await readFile(new URL("workers/wrangler.toml", root), "utf8");
  const match = /^ALLOWED_RELEASES\s*=\s*"([^"]*)"/m.exec(toml);
  if (match === null) throw new Error("workers/wrangler.toml has no ALLOWED_RELEASES");
  return (match[1] ?? "")
    .split(",")
    .map((value) => value.trim())
    .filter((value) => value !== "");
}

describe("release allowlist の同期（Web / Worker）", () => {
  it("Worker の allowlist は Web の既知 release を含む", async () => {
    const allowed = await workerAllowedReleases();
    expect(new Set(allowed)).toEqual(new Set(KNOWN_RELEASES));
  });

  it("Web の既定は Worker の allowlist に通る", async () => {
    const allowed = await workerAllowedReleases();
    expect(allowed).toContain(DEFAULT_RELEASE_ID);
  });

  it("公開中の release は manifest の releaseId と一致する", async () => {
    const manifest = JSON.parse(
      await readFile(new URL("fixtures/generated/manifest.json", root), "utf8"),
    ) as { releaseId: string };
    expect(DEFAULT_RELEASE_ID).toBe(manifest.releaseId);
  });

  it("rollback 先 all-real-v3 が allowlist に残っており、既定を 1 行戻せる", async () => {
    const allowed = await workerAllowedReleases();
    expect(allowed).toContain("all-real-v3");
    // rollback は DEFAULT_RELEASE_ID 1 行的変更で完結する（Worker 側は allowlist を狭めない）。
    expect(allowed.filter((releaseId) => releaseId !== DEFAULT_RELEASE_ID)).toContain("all-real-v3");
  });
});
