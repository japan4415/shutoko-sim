// release allowlist の同期と rollback の手順を固定する。
//
// 異常時は Web の既定（web/src/worker/artifact-hashes.ts の DEFAULT_RELEASE_ID）を前の版へ
// 戻す。Worker の allowlist（workers/wrangler.toml の ALLOWED_RELEASES）は狭めず、古い版を
// 残したまま運用するので、戻すのは Web の既定 1 行である。
//
// 公開 fixture（fixtures/generated）は既定を戻しても新しい版のままであるため、
// 「既定 = fixture の releaseId」という等価は rollback 後に必ず壊れる。代わりに
// 「Worker と Web が同じ集合を配信し、既定も公開 fixture も allowlist に残り、直前の版が
// rollback 先として使える」という rollback 不変の関係を固定する。default を 1 行戻した
// rollback commit でもこのファイルは green のままであることが、rollback 手順の動作確認に
// なる。
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

/** KNOWN_RELEASES（古い順）の並びから、ある既定の版を 1 行戻した先（rollback 先）を導く。 */
function rollbackTargetFrom(currentReleaseId: string): string {
  const index = KNOWN_RELEASES.indexOf(currentReleaseId);
  if (index < 1) {
    throw new Error(`${currentReleaseId} has no predecessor to roll back to`);
  }
  return KNOWN_RELEASES[index - 1] as string;
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

  it("公開 fixture の release も allowlist に残り、rollback で配信を止めない", async () => {
    const manifest = JSON.parse(
      await readFile(new URL("fixtures/generated/manifest.json", root), "utf8"),
    ) as { releaseId: string };
    const allowed = await workerAllowedReleases();
    // 既定を 1 行戻しても公開済み release は allowlist に残る（allowlist を狭めない）。
    expect(allowed).toContain(manifest.releaseId);
    expect(KNOWN_RELEASES).toContain(manifest.releaseId);
  });

  it("既定の直前の版が rollback 先で、allowlist に残る", async () => {
    const allowed = await workerAllowedReleases();
    const target = rollbackTargetFrom(DEFAULT_RELEASE_ID);
    expect(target).not.toBe(DEFAULT_RELEASE_ID);
    expect(allowed).toContain(target);
    // allowlist は狭めないので、新しい版へ戻る余地も残る。
    expect(allowed).toContain(DEFAULT_RELEASE_ID);
  });

  it("文書化された rollback（既定を all-real-v3 へ 1 行戻す）が allowlist 内で完結する", async () => {
    const allowed = await workerAllowedReleases();
    // docs/delivery.md の rollback: DEFAULT_RELEASE_ID 1 行の変更だけで前の版へ戻る。
    expect(rollbackTargetFrom("all-real-v4")).toBe("all-real-v3");
    // その 1 行だけ変えた rollback commit でも、Worker と Web の集合は一致したまま。
    expect(allowed).toContain("all-real-v3");
    expect(new Set(allowed)).toEqual(new Set(KNOWN_RELEASES));
  });
});
