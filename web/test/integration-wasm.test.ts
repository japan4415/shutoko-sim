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

interface WasmBuildContract {
  contractVersion: number;
  engineVersion: string;
  graphSchemaVersion: number;
  supportedGraphSchemaVersions: number[];
  requiredGraphFields: string[];
  requiredGraphSchema4Fields: string[];
}

function toBytes(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function toBinary(bytes: Uint8Array): Uint8Array {
  return bytes;
}

export function isWasmContractCompatible(
  current: WasmBuildContract,
  built: WasmBuildContract,
  graph: Record<string, unknown>,
  manifest: Record<string, unknown>,
): boolean {
  if (JSON.stringify(current) !== JSON.stringify(built)) {
    return false;
  }
  if (
    !current.supportedGraphSchemaVersions.includes(Number(graph.schemaVersion)) ||
    manifest.engineVersion !== current.engineVersion ||
    !Array.isArray(graph.odTariffs) ||
    !Array.isArray(graph.ramps)
  ) {
    return false;
  }
  if (
    graph.schemaVersion === 4 &&
    (!Array.isArray(graph.routeMemberships) || !Array.isArray(graph.billingPairs))
  ) {
    return false;
  }
  if (
    graph.schemaVersion === 4 &&
    !(graph.billingPairs as unknown[]).every((value) => {
      if (typeof value !== "object" || value === null) return false;
      const pair = value as Record<string, unknown>;
      if (typeof pair.pairKind !== "string") return false;
      if (pair.pairKind === "legacyRing") {
        return (
          typeof pair.anchor === "object" &&
          pair.anchor !== null &&
          (pair.anchor as Record<string, unknown>).anchorKind === "sameNode"
        );
      }
      if (pair.pairKind !== "radialReturn") return false;
      const routePlan = pair.routePlan;
      return (
        typeof routePlan === "object" &&
        routePlan !== null &&
        typeof (routePlan as Record<string, unknown>).anchor === "object" &&
        (routePlan as Record<string, unknown>).anchor !== null &&
        ((routePlan as Record<string, unknown>).anchor as Record<string, unknown>).anchorKind ===
          "directedJunction"
      );
    })
  ) {
    return false;
  }
  return graph.ramps.every(
    (ramp) =>
      typeof ramp === "object" &&
      ramp !== null &&
      typeof (ramp as Record<string, unknown>).id === "string" &&
      typeof (ramp as Record<string, unknown>).mainlineNodeId === "string",
  );
}

/** Explicit build metadata is compared with the graph/manifest contract. */
async function wasmBuildIsCurrent(): Promise<boolean> {
  const wasmPath = new URL("shutoko_routing_bg.wasm", wasmDir);
  const builtContractPath = new URL("wasm-contract.json", wasmDir);
  if (!existsSync(wasmPath) || !existsSync(gluePath) || !existsSync(builtContractPath)) {
    return false;
  }
  const [current, built, graph, manifest] = await Promise.all([
    readFile(new URL("crates/routing-wasm/wasm-contract.json", root), "utf8"),
    readFile(builtContractPath, "utf8"),
    readFile(new URL("fixtures/generated/graph.json", root), "utf8"),
    readFile(new URL("fixtures/generated/manifest.json", root), "utf8"),
  ]);
  return isWasmContractCompatible(
    JSON.parse(current) as WasmBuildContract,
    JSON.parse(built) as WasmBuildContract,
    JSON.parse(graph) as Record<string, unknown>,
    JSON.parse(manifest) as Record<string, unknown>,
  );
}

beforeAll(async () => {
  if (!(await wasmBuildIsCurrent())) {
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
  it("旧graph契約のbuild metadataをstaleとして判定する", () => {
    const current: WasmBuildContract = {
      contractVersion: 2,
      engineVersion: "0.1.0",
      graphSchemaVersion: 4,
      supportedGraphSchemaVersions: [2, 3, 4],
      requiredGraphFields: ["odTariffs", "ramps[].id", "ramps[].mainlineNodeId"],
      requiredGraphSchema4Fields: [
        "routeMemberships",
        "billingPairs[].pairKind",
        "billingPairs[].anchor.anchorKind",
        "billingPairs[].routePlan.anchor.anchorKind",
      ],
    };
    const stale = { ...current, graphSchemaVersion: 1 };
    const graph = {
      schemaVersion: 2,
      odTariffs: [],
      ramps: [{ id: "ramp:test", mainlineNodeId: "n:1" }],
    };
    expect(isWasmContractCompatible(current, stale, graph, { engineVersion: "0.1.0" })).toBe(
      false,
    );
  });

  it("神田橋近傍・15〜60 分で status ok と候補（mapsUrl プレフィックス付き）が返る", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glueBytes = toBytes(await readFile(gluePath, "utf8"));

    // engine.json の期待値は固定値ではなく、配信する実ファイルから計算する。
    // こうしておくと wasm のビルドが環境をまたいでバイト一致しなくても CI で通る。
    const expectationOf = async (bytes: Uint8Array): Promise<ArtifactExpectation> => ({
      sha256: await hexDigest(bytes.slice().buffer as ArrayBuffer),
      byteLength: bytes.byteLength,
    });

    const graphJsonText = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const graphJsonObj = JSON.parse(graphJsonText);
    const releaseId = graphJsonObj.releaseId as string;

    const files: Record<string, { bytes: Uint8Array; kind: "text" | "binary" }> = {
      [`/releases/${releaseId}/manifest.json`]: {
        bytes: toBytes(await readFile(new URL("fixtures/generated/manifest.json", root), "utf8")),
        kind: "text",
      },
      [`/releases/${releaseId}/engine.json`]: {
        bytes: toBytes(
          JSON.stringify({
            schemaVersion: 1,
            releaseId,
            artifacts: [
              { path: "shutoko_routing_bg.wasm", ...(await expectationOf(wasmBytes)) },
              { path: "shutoko_routing.js", ...(await expectationOf(glueBytes)) },
            ],
          }),
        ),
        kind: "text",
      },
      [`/releases/${releaseId}/graph.json`]: {
        bytes: toBytes(graphJsonText),
        kind: "text",
      },
      [`/releases/${releaseId}/shutoko_routing_bg.wasm`]: {
        bytes: wasmBytes,
        kind: "binary",
      },
      [`/releases/${releaseId}/shutoko_routing.js`]: {
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

    const glue = await import(gluePath.href);
    const state = await loadRelease(fetchImpl, releaseId, async () => glue);

    expect(calls).toEqual([
      `/releases/${releaseId}/manifest.json`,
      `/releases/${releaseId}/engine.json`,
      `/releases/${releaseId}/graph.json`,
      `/releases/${releaseId}/shutoko_routing_bg.wasm`,
      `/releases/${releaseId}/shutoko_routing.js`,
    ]);

    const msg: UiSearchMessage = {
      type: "search",
      requestId: "integration-1",
      releaseId,
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
    const firstCandidate = result.candidates[0];
    expect(firstCandidate).toBeDefined();
    expect(firstCandidate?.pairKind).not.toBe("radialReturn");
    if (firstCandidate !== undefined && firstCandidate.pairKind !== "radialReturn") {
      expect(firstCandidate.handoff.mapsUrl.startsWith("https://www.google.com/maps/dir/?api=1")).toBe(true);
    }
    console.info(
      `[integration-wasm] candidates=${String(result.candidates.length)} elapsed=${elapsedMs.toFixed(1)}ms`,
    );
  }, 30_000);

  it("不正な入力（maxMinutes=0）は INVALID_INPUT をスローする", async () => {
    const glue = await import(gluePath.href);
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    glue.default;
    // 前テストと同じプロセス内で初期化済みのはずだが、単独実行にも耐えるよう再度初期化する
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphObj = JSON.parse(graphJson) as { releaseId: string };
    const releaseId = graphObj.releaseId;
    const pg = glue.prepare(graphJson, "{}");
    try {
      const err = (() => {
        try {
          glue.searchPrepared(
            pg,
            JSON.stringify({
              requestId: "integration-2",
              releaseId,
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
    const glue = await import(gluePath.href);
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const graphObj = JSON.parse(graphJson) as { releaseId: string };
    const releaseId = graphObj.releaseId;
    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      const msg: UiSearchMessage = {
        type: "search",
        requestId: "integration-access-contract",
        releaseId,
        pricingAt: "2026-09-10T00:00:00Z",
        // 立川駅: 全線fixtureの最寄り入口まで約18.6kmで、アクセス時間が0でない。
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

  it("明示した神奈川の verified-bound OD を課金ペアseedなしで探索できる", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glue = await import(gluePath.href);
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const graphObj = JSON.parse(graphJson) as { releaseId: string };
    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      const msg: UiSearchMessage = {
        type: "search",
        requestId: "integration-explicit-k1",
        releaseId: graphObj.releaseId,
        pricingAt: "2026-09-10T00:00:00Z",
        entryRampId: "ramp:k1-inbound:daishi-entry",
        exitRampId: "ramp:k1-inbound:minato-mirai-exit",
        minMinutes: 1,
        maxMinutes: 240,
        vehicleProfile: "passenger-car-etc",
      };
      const requestJson = JSON.stringify(buildSearchRequest(msg));
      const first = glue.searchPrepared(pg, requestJson);
      const second = glue.searchPrepared(pg, requestJson);
      expect(second).toBe(first);
      const result = parseSearchResult(first);
      expect(result.status).toBe("ok");
      expect(result.reason).toBeNull();
      expect(result.expandedStates).toBeLessThan(100_000);
      expect(result.candidates[0]?.entry.rampId).toBe(msg.entryRampId);
      expect(result.candidates[0]?.exit.rampId).toBe(msg.exitRampId);
      const candidate = result.candidates[0];
      expect(candidate).toBeDefined();
      if (candidate !== undefined && candidate.pairKind !== "radialReturn") {
        expect(candidate.loop.distanceMeters).toBeGreaterThanOrEqual(5_000);
      }
      expect(result.candidates[0]?.toll.amountYen).toBeNull();
    } finally {
      pg.free();
    }
  }, 30_000);

  it("狭い 60 分窓の座標検索は最近接 tier の TIME_WINDOW 診断を返す", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glue = await import(gluePath.href);
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const graphObj = JSON.parse(graphJson) as { releaseId: string };
    const releaseId = graphObj.releaseId;
    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      const msg: UiSearchMessage = {
        type: "search",
        requestId: "integration-narrow-minplan",
        releaseId,
        pricingAt: "2026-09-10T00:00:00Z",
        origin: { lat: 35.6979, lon: 139.4139 },
        minMinutes: 15,
        maxMinutes: 60,
        vehicleProfile: "passenger-car-etc",
      };
      const result = parseSearchResult(
        glue.searchPrepared(pg, JSON.stringify(buildSearchRequest(msg))),
      );
      // Issue #57: 立川駅の最近接入口 tier (4号高井戸) は完全評価され、合法周回を
      // 持つが 60 分窓に収まらない。最近接入口優先の診断として TIME_WINDOW と
      // 証明済み minPlanSeconds を返し、遠方の入口へは縮退しない。これにより UI の
      // 時間枠復帰導線（上限を広げる / 最小時間を下げる）が座標検索でも機能する。
      expect(result.status).toBe("no_candidates");
      expect(result.reason).toBe("TIME_WINDOW");
      expect(result.minPlanSeconds).toBe(10_727);
      expect(result.candidates).toHaveLength(0);
      expect(result.expandedStates).toBeLessThan(100_000);
    } finally {
      pg.free();
    }
  }, 30_000);

  it("目黒座標は最近接の目黒入口を動的 OD として選び shutoko_time を返す", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glue = await import(gluePath.href);
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const graphObj = JSON.parse(graphJson) as { releaseId: string };
    const releaseId = graphObj.releaseId;
    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      // 目黒入口直上と目黒近傍の 2 座標 × 3 時間窓（Issue #57 受入条件）。
      const cases: [number, number, number, number][] = [
        [35.635681, 139.718489, 15, 60],
        [35.635681, 139.718489, 30, 120],
        [35.635681, 139.718489, 52, 120],
        [35.63239, 139.71524, 15, 60],
        [35.63239, 139.71524, 30, 120],
        [35.63239, 139.71524, 52, 120],
      ];
      for (const [lat, lon, minMinutes, maxMinutes] of cases) {
        const msg: UiSearchMessage = {
          type: "search",
          requestId: `integration-meguro-${String(lat)}-${String(lon)}-${String(minMinutes)}-${String(maxMinutes)}`,
          releaseId,
          pricingAt: "2026-09-10T00:00:00Z",
          origin: { lat, lon },
          minMinutes,
          maxMinutes,
          vehicleProfile: "passenger-car-etc",
        };
        const requestJson = JSON.stringify(buildSearchRequest(msg));
        const first = glue.searchPrepared(pg, requestJson);
        const second = glue.searchPrepared(pg, requestJson);
        // 同一入力の再実行はバイト完全一致（決定論）。
        expect(second).toBe(first);
        const result = parseSearchResult(first);
        expect(result.status).toBe("ok");
        expect(result.candidates.length).toBeGreaterThan(0);
        expect(result.expandedStates).toBeLessThanOrEqual(100_000);
        expect(result.rankingMode).toBe("shutoko_time");
        for (const candidate of result.candidates) {
          // 最近接入口優先: 返る入口はすべて目黒入口、出口は同施設の目黒出口。
          expect(candidate.entry.rampId).toBe("ramp:2-inbound:meguro-entry");
          expect(candidate.exit.rampId).toBe("ramp:2-outbound:meguro-exit");
          // 動的 OD は料金未算出（amountYen=null）。
          expect(candidate.toll.amountYen).toBeNull();
        }
      }
    } finally {
      pg.free();
    }
  }, 60_000);
});
