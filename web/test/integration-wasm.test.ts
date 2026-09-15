// 実 WASM を node で動かす統合テスト。
// dist/wasm が無ければ scripts/build-wasm.sh で生成し、fetch モックで成果物を配信して
// loadRelease → search の全経路を検証する。
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { beforeAll, describe, expect, it } from "vitest";
import {
  buildSearchRequest,
  hexDigest,
  loadRelease,
  parseSearchResult,
  SEARCH_LIMITS_JSON,
  type ArtifactExpectation,
  type FetchLike,
  type FetchResponseLike,
} from "../src/worker/pipeline";
import { accessSecondsFromMeters } from "../src/ui/model";
import type { UiSearchMessage } from "../src/worker/types";

const root = new URL("../../", import.meta.url);
const wasmDir = new URL("dist/wasm/", root);
const gluePath = new URL("shutoko_routing.js", wasmDir);

function toBytes(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function toBinary(bytes: Uint8Array): Uint8Array {
  return bytes;
}

beforeAll(async () => {
  if (!existsSync(gluePath) || !existsSync(new URL("shutoko_routing_bg.wasm", wasmDir))) {
    execFileSync("bash", ["scripts/build-wasm.sh"], {
      cwd: fileURLToPathSafe(root),
      stdio: "inherit",
    });
  }
}, 120_000);

function fileURLToPathSafe(url: URL): string {
  // node:url の fileURLToPath 相当（import.meta.url は file: スキーマ）
  return decodeURIComponent(url.href.replace(/^file:\/\//, ""));
}

describe("実 WASM 統合（fetch モック → loadRelease → search）", () => {
  it("神田橋近傍・15〜60 分で status ok と候補（mapsUrl プレフィックス付き）が返る", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glueBytes = toBytes(await readFile(gluePath, "utf8"));

    // engine.json の期待値は固定値ではなく、配信する実ファイルから計算する。
    // こうしておくと wasm のビルドが環境をまたいでバイト一致しなくても CI で通る。
    const expectationOf = async (bytes: Uint8Array): Promise<ArtifactExpectation> => ({
      sha256: await hexDigest(bytes.slice().buffer as ArrayBuffer),
      byteLength: bytes.byteLength,
    });

    const files: Record<string, { bytes: Uint8Array; kind: "text" | "binary" }> = {
      "/releases/c1-real-v1/manifest.json": {
        bytes: toBytes(await readFile(new URL("fixtures/generated/manifest.json", root), "utf8")),
        kind: "text",
      },
      "/releases/c1-real-v1/engine.json": {
        bytes: toBytes(
          JSON.stringify({
            schemaVersion: 1,
            releaseId: "c1-real-v1",
            artifacts: [
              { path: "shutoko_routing_bg.wasm", ...(await expectationOf(wasmBytes)) },
              { path: "shutoko_routing.js", ...(await expectationOf(glueBytes)) },
            ],
          }),
        ),
        kind: "text",
      },
      "/releases/c1-real-v1/graph.json": {
        bytes: toBytes(await readFile(new URL("fixtures/generated/graph.json", root), "utf8")),
        kind: "text",
      },
      "/releases/c1-real-v1/shutoko_routing_bg.wasm": {
        bytes: wasmBytes,
        kind: "binary",
      },
      "/releases/c1-real-v1/shutoko_routing.js": {
        bytes: glueBytes,
        kind: "text",
      },
    };

    const calls: string[] = [];
    const fetchImpl: FetchLike = async (url: string): Promise<FetchResponseLike> => {
      calls.push(url);
      const entry = files[url];
      if (entry === undefined) {
        throw new Error(`unexpected url: ${url}`);
      }
      return {
        ok: true,
        status: 200,
        async arrayBuffer(): Promise<ArrayBuffer> {
          return entry.bytes.slice().buffer as ArrayBuffer;
        },
        async text(): Promise<string> {
          return new TextDecoder().decode(entry.bytes);
        },
      } satisfies FetchResponseLike & { status: number };
    };

    const glue = await import("../../dist/wasm/shutoko_routing.js");
    const state = await loadRelease(fetchImpl, "c1-real-v1", async () => glue);

    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
      "/releases/c1-real-v1/graph.json",
      "/releases/c1-real-v1/shutoko_routing_bg.wasm",
      "/releases/c1-real-v1/shutoko_routing.js",
    ]);

    const msg: UiSearchMessage = {
      type: "search",
      requestId: "integration-1",
      releaseId: "c1-real-v1",
      pricingAt: "2026-09-10T00:00:00Z",
      origin: { lat: 35.6896727, lon: 139.7644248 },
      minMinutes: 15,
      maxMinutes: 60,
      vehicleProfile: "passenger-car-etc",
    };

    const started = performance.now();
    const result = parseSearchResult(state.searchPrepared(JSON.stringify(buildSearchRequest(msg))));
    const elapsedMs = performance.now() - started;

    expect(result.status).toBe("ok");
    expect(result.candidates.length).toBeGreaterThan(0);
    expect(result.candidates[0]?.handoff.mapsUrl.startsWith("https://www.google.com/maps/dir/?api=1")).toBe(true);
    console.info(
      `[integration-wasm] candidates=${String(result.candidates.length)} elapsed=${elapsedMs.toFixed(1)}ms`,
    );
  }, 30_000);

  it("不正な入力（maxMinutes=0）は INVALID_INPUT をスローする", async () => {
    const glue = await import("../../dist/wasm/shutoko_routing.js");
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    glue.default;
    // 前テストと同じプロセス内で初期化済みのはずだが、単独実行にも耐えるよう再度初期化する
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const pg = glue.prepare(graphJson, "{}");
    try {
      const err = (() => {
        try {
          glue.searchPrepared(
            pg,
            JSON.stringify({
              requestId: "integration-2",
              releaseId: "c1-real-v1",
              origin: { lat: 35.6896727, lon: 139.7644248 },
              minMinutes: 15,
              maxMinutes: 0,
              vehicleProfile: "passenger-car-etc",
              pricingAt: "2026-09-10T00:00:00Z",
            }),
          );
          return null;
        } catch (e) {
          return e;
        }
      })();
      expect(err).toBeInstanceOf(Error);
      expect(JSON.parse((err as Error).message)).toMatchObject({ code: "INVALID_INPUT" });
    } finally {
      pg.free();
    }
  }, 30_000);

  it("実エンジンの accessSeconds は web の概算式（直線×1.3÷30km/h 切り上げ）と一致する", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glue = await import("../../dist/wasm/shutoko_routing.js");
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      const msg: UiSearchMessage = {
        type: "search",
        requestId: "integration-access-contract",
        releaseId: "c1-real-v1",
        pricingAt: "2026-09-10T00:00:00Z",
        // 立川駅: 最寄り入口まで約 29.6 km で、アクセス時間が 0 でない候補が返る。
        origin: { lat: 35.6979, lon: 139.4139 },
        minMinutes: 15,
        maxMinutes: 240,
        vehicleProfile: "passenger-car-etc",
      };
      const result = parseSearchResult(
        glue.searchPrepared(pg, JSON.stringify(buildSearchRequest(msg))),
      );
      expect(result.status).toBe("ok");
      expect(result.candidates.length).toBeGreaterThan(0);
      for (const candidate of result.candidates) {
        const distance = candidate.snappedOrigin.distanceMeters;
        expect(distance).toBeGreaterThan(0);
        // Rust の estimated_access_seconds と web の accessSecondsFromMeters が一致すること。
        expect(candidate.duration.accessSeconds).toBe(accessSecondsFromMeters(distance));
      }
    } finally {
      pg.free();
    }
  }, 30_000);

  it("狭い 60 分窓でも実エンジンが minPlanSeconds を返す（診断の上界 240 分）", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glue = await import("../../dist/wasm/shutoko_routing.js");
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      const msg: UiSearchMessage = {
        type: "search",
        requestId: "integration-narrow-minplan",
        releaseId: "c1-real-v1",
        pricingAt: "2026-09-10T00:00:00Z",
        origin: { lat: 35.6979, lon: 139.4139 },
        minMinutes: 15,
        maxMinutes: 60,
        vehicleProfile: "passenger-car-etc",
      };
      const result = parseSearchResult(
        glue.searchPrepared(pg, JSON.stringify(buildSearchRequest(msg))),
      );
      expect(result.status).toBe("no_candidates");
      expect(result.reason).toBe("TIME_WINDOW");
      // 60 分窓では候補にならないが、240 分以内の合法周回の最短計画は診断として返る。
      expect(result.minPlanSeconds).not.toBeNull();
      expect(result.minPlanSeconds ?? 0).toBeGreaterThan(60 * 60);
      expect(result.minPlanSeconds ?? 0).toBeLessThanOrEqual(240 * 60);
    } finally {
      pg.free();
    }
  }, 30_000);
});
