// パイプライン純粋関数のユニットテスト（node 環境、fetch/import はモック注入）。
import inspector from "node:inspector";
import { describe, expect, it } from "vitest";
import pendingDeviceManifest from "../../data/device-verification-manifest.json?raw";
import generatedGraph from "../../fixtures/generated/graph.json?raw";
import generatedManifest from "../../fixtures/generated/manifest.json?raw";
import schema4Graph from "../../fixtures/graph-v4/graph-radial-fixture.json?raw";
import {
  buildResultResponse,
  buildSearchLimitsJson,
  buildSearchRequest,
  DEVICE_VERIFICATION_EVALUATED_AT,
  DEVICE_VERIFICATION_MANIFEST_JSON,
  hexDigest,
  loadRelease,
  MAX_ACCESS_DISTANCE_METERS,
  parseGraphDocument,
  parseWasmError,
  PipelineError,
  routeMembershipsSha256,
  ReleaseStore,
  SEARCH_LIMITS_JSON,
  verifyArtifact,
  type FetchLike,
  type FetchResponseLike,
  type LoadedRelease,
  type WasmGlueModule,
  type WasmPreparedGraphLike,
} from "../src/worker/pipeline";
import type { SearchResult, UiSearchMessage } from "../src/worker/types";

const encoder = new TextEncoder();

function responseFrom(bytes: Uint8Array): FetchResponseLike {
  return {
    ok: true,
    status: 200,
    async arrayBuffer(): Promise<ArrayBuffer> {
      return bytes.slice().buffer as ArrayBuffer;
    },
    async text(): Promise<string> {
      return new TextDecoder().decode(bytes);
    },
  } satisfies FetchResponseLike & { status: number };
}

interface MockFetch {
  fetch: FetchLike;
  calls: string[];
}

function mockFetch(files: Record<string, Uint8Array>): MockFetch {
  const calls: string[] = [];
  const fetchImpl: FetchLike = async (url: string) => {
    calls.push(url);
    const bytes = files[url];
    if (bytes === undefined) {
      return {
        ok: false,
        status: 404,
        async arrayBuffer(): Promise<ArrayBuffer> {
          return new ArrayBuffer(0);
        },
        async text(): Promise<string> {
          return "";
        },
      } satisfies FetchResponseLike & { status: number };
    }
    return responseFrom(bytes);
  };
  return { fetch: fetchImpl, calls };
}

async function expectationOf(bytes: Uint8Array): Promise<{ sha256: string; byteLength: number }> {
  return {
    sha256: await hexDigest(bytes.slice().buffer as ArrayBuffer),
    byteLength: bytes.byteLength,
  };
}

const stubGlue = (calls: unknown[]): WasmGlueModule => ({
  async default(params: { module_or_path: Uint8Array | ArrayBuffer }): Promise<unknown> {
    calls.push(params.module_or_path);
    return undefined;
  },
  prepare(): WasmPreparedGraphLike {
    return {
      free() {},
    };
  },
  searchPrepared(): string {
    return JSON.stringify({ status: "ok", candidates: [] });
  },
  search(): string {
    return JSON.stringify({ status: "ok", candidates: [] });
  },
});

describe("hexDigest / verifyArtifact", () => {
  const bytes = encoder.encode("hello");

  it("SHA-256 hex とバイト長の一致で true", async () => {
    const expected = await expectationOf(bytes);
    expect(expected.sha256).toBe(
      "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    );
    expect(await verifyArtifact(bytes, expected)).toBe(true);
  });

  it("バイト長不一致では false", async () => {
    const expected = await expectationOf(bytes);
    expect(await verifyArtifact(bytes, { ...expected, byteLength: expected.byteLength + 1 })).toBe(
      false,
    );
  });

  it("sha256 不一致では false（バイト長は一致）", async () => {
    const expected = await expectationOf(bytes);
    const tampered = new Uint8Array(bytes);
    tampered[0] = 106; // "hello" -> "jello"（'j'=106）、長さ同一
    const other = await expectationOf(tampered);
    expect(await verifyArtifact(tampered, { sha256: expected.sha256, byteLength: other.byteLength })).toBe(
      false,
    );
  });
});

describe("parseGraphDocument", () => {
  it("schema 4 の legacyRing / radialReturn billingPairs を受け取る", () => {
    const graph = parseGraphDocument(schema4Graph);
    expect(graph.schemaVersion).toBe(4);
    expect(graph.billingPairs).toHaveLength(2);
    expect(graph.routeMemberships).toHaveLength(4);
  });

  it("生成済み graph と manifest の route membership hash が一致する", async () => {
    const graph = JSON.parse(generatedGraph) as { routeMemberships: unknown };
    const manifest = JSON.parse(generatedManifest) as { routeMembershipsSha256: string };
    expect(await routeMembershipsSha256(graph.routeMemberships)).toBe(
      manifest.routeMembershipsSha256,
    );
  });

  it("未知 version、未知 pairKind、routeMemberships 欠落を拒否する", () => {
    const graph = JSON.parse(schema4Graph) as Record<string, unknown>;
    expect(() => parseGraphDocument(JSON.stringify({ ...graph, schemaVersion: 5 }))).toThrowError(
      /schemaVersion/,
    );
    const billingPairs = graph.billingPairs as Record<string, unknown>[];
    expect(() =>
      parseGraphDocument(
        JSON.stringify({
          ...graph,
          billingPairs: [{ ...billingPairs[0], pairKind: "futurePair" }],
        }),
      ),
    ).toThrowError(/pairKind/);
    const { routeMemberships: _routeMemberships, ...partial } = graph;
    expect(() => parseGraphDocument(JSON.stringify(partial))).toThrowError(/routeMemberships/);
  });

  it("loadRelease が schema 4 graph を WASM prepare へ渡せる", async () => {
    const releaseId = "graph-v4-fixture-v1";
    const graphBytes = encoder.encode(schema4Graph);
    const wasmBytes = new Uint8Array([0, 0x61, 0x73, 0x6d]);
    const glueBytes = encoder.encode("export default function(){}");
    const graphExpected = await expectationOf(graphBytes);
    const wasmExpected = await expectationOf(wasmBytes);
    const glueExpected = await expectationOf(glueBytes);
    const routeHash = await routeMembershipsSha256(
      (JSON.parse(schema4Graph) as { routeMemberships: unknown }).routeMemberships,
    );
    const files = {
      [`/releases/${releaseId}/manifest.json`]: encoder.encode(
        JSON.stringify({
          schemaVersion: 1,
          releaseId,
          graphSchemaVersion: 4,
          routePlanVersion: 1,
          billingPairsVersion: "v2",
          routeMembershipsSha256: routeHash,
          artifacts: [{ path: "graph.json", ...graphExpected }],
        }),
      ),
      [`/releases/${releaseId}/engine.json`]: encoder.encode(
        JSON.stringify({
          schemaVersion: 1,
          releaseId,
          artifacts: [
            { path: "shutoko_routing_bg.wasm", ...wasmExpected },
            { path: "shutoko_routing.js", ...glueExpected },
          ],
        }),
      ),
      [`/releases/${releaseId}/graph.json`]: graphBytes,
      [`/releases/${releaseId}/shutoko_routing_bg.wasm`]: wasmBytes,
      [`/releases/${releaseId}/shutoko_routing.js`]: glueBytes,
    };
    const { fetch } = mockFetch(files);
    let preparedGraphJson = "";
    const glue: WasmGlueModule = {
      default: async () => {},
      prepare: (graphJson: string) => {
        preparedGraphJson = graphJson;
        return { free() {} };
      },
      searchPrepared: () => "{}",
    };
    const loaded = await loadRelease(fetch, releaseId, async () => glue);
    expect(JSON.parse(preparedGraphJson).schemaVersion).toBe(4);
    expect(JSON.parse(preparedGraphJson).billingPairs).toHaveLength(2);
    loaded.free();
  });
});

describe("buildSearchRequest", () => {
  it("index.d.ts の 8 フィールドのみ、type を含まない", () => {
    const msg: UiSearchMessage = {
      type: "search",
      requestId: "request-1",
      releaseId: "c1-real-v1",
      pricingAt: "2026-09-10T00:00:00Z",
      origin: { lat: 35.6896727, lon: 139.7644248 },
      originNodeId: "n:1",
      minMinutes: 15,
      maxMinutes: 60,
      vehicleProfile: "passenger-car-etc",
    };
    const request = buildSearchRequest(msg);
    expect(Object.keys(request)).toEqual([
      "requestId",
      "releaseId",
      "originNodeId",
      "origin",
      "minMinutes",
      "maxMinutes",
      "vehicleProfile",
      "pricingAt",
    ]);
    expect(Object.hasOwn(request, "type")).toBe(false);
  });

  it("origin 指定時は originNodeId を省略した 7 フィールド", () => {
    const msg: UiSearchMessage = {
      type: "search",
      requestId: "request-2",
      releaseId: "c1-real-v1",
      pricingAt: "2026-09-10T00:00:00Z",
      origin: { lat: 35.6896727, lon: 139.7644248 },
      minMinutes: 15,
      maxMinutes: 60,
      vehicleProfile: "passenger-car-etc",
    };
    const request = buildSearchRequest(msg);
    expect(Object.keys(request)).toHaveLength(7);
    expect(Object.hasOwn(request, "originNodeId")).toBe(false);
    expect(JSON.parse(JSON.stringify(request))).toEqual({
      requestId: "request-2",
      releaseId: "c1-real-v1",
      origin: { lat: 35.6896727, lon: 139.7644248 },
      minMinutes: 15,
      maxMinutes: 60,
      vehicleProfile: "passenger-car-etc",
      pricingAt: "2026-09-10T00:00:00Z",
    });
  });

  it("明示ランプ ID を WASM SearchRequest へ欠落なく渡す", () => {
    const msg: UiSearchMessage = {
      type: "search",
      requestId: "request-explicit-od",
      releaseId: "all-real-v1",
      pricingAt: "2026-09-10T00:00:00Z",
      entryRampId: "ramp:k1-inbound:daishi-entry",
      exitRampId: "ramp:k1-inbound:minato-mirai-exit",
      minMinutes: 1,
      maxMinutes: 240,
      vehicleProfile: "passenger-car-etc",
    };
    expect(buildSearchRequest(msg)).toMatchObject({
      entryRampId: msg.entryRampId,
      exitRampId: msg.exitRampId,
    });
  });
});

describe("parseWasmError", () => {
  it("RoutingErrorPayload JSON を展開して code をそのまま返す", () => {
    const err = new Error('{"code":"INVALID_INPUT","message":"maxMinutes must be >= minMinutes"}');
    expect(parseWasmError(err)).toEqual({
      code: "INVALID_INPUT",
      message: "maxMinutes must be >= minMinutes",
    });
  });

  it("JSON でない message は WASM_ERROR にフォールバック", () => {
    expect(parseWasmError(new Error("linear memory exhausted"))).toEqual({
      code: "WASM_ERROR",
      message: "linear memory exhausted",
    });
  });
});

describe("loadRelease の cacheBust（bench 計測フック）", () => {
  const graphBytes = encoder.encode(JSON.stringify({ schemaVersion: 2, releaseId: "c1-real-v1", vehicleProfile: "passenger-car-etc", nodes: [], edges: [], billingPairs: [] }));
  const wasmBytes = new Uint8Array([0, 0x61, 0x73, 0x6d]);
  const glueText = "export default function(){};export function search(){return '{}'}";
  const glueBytes = encoder.encode(glueText);

  /** クエリ付き URL を、クエリ無しのキーで引くモックへ委譲しつつ実 URL を記録する。 */
  function recordingFetch(files: Record<string, Uint8Array>): { fetch: FetchLike; seen: string[] } {
    const { fetch: inner } = mockFetch(files);
    const seen: string[] = [];
    const fetchImpl: FetchLike = async (url, init) => {
      seen.push(url);
      return inner(url.split("?")[0] ?? url, init);
    };
    return { fetch: fetchImpl, seen };
  }

  async function artifactFiles(): Promise<Record<string, Uint8Array>> {
    const graphExpected = await expectationOf(graphBytes);
    const manifest = JSON.stringify({
      releaseId: "c1-real-v1",
      artifacts: [{ path: "graph.json", ...graphExpected }],
    });
    const engine = JSON.stringify({
      schemaVersion: 1,
      releaseId: "c1-real-v1",
      artifacts: [
        { path: "shutoko_routing_bg.wasm", ...(await expectationOf(wasmBytes)) },
        { path: "shutoko_routing.js", ...(await expectationOf(glueBytes)) },
      ],
    });
    return {
      "/releases/c1-real-v1/manifest.json": encoder.encode(manifest),
      "/releases/c1-real-v1/engine.json": encoder.encode(engine),
      "/releases/c1-real-v1/graph.json": graphBytes,
      "/releases/c1-real-v1/shutoko_routing_bg.wasm": wasmBytes,
      "/releases/c1-real-v1/shutoko_routing.js": glueBytes,
    };
  }

  it("cacheBust 指定時は 5 成果物の URL すべてに ?bench=<nonce> が付く", async () => {
    const { fetch, seen } = recordingFetch(await artifactFiles());
    const initCalls: unknown[] = [];
    const state = await loadRelease(fetch, "c1-real-v1", async () => stubGlue(initCalls), undefined, {
      cacheBust: "nonce-1",
    });
    expect(seen).toEqual([
      "/releases/c1-real-v1/manifest.json?bench=nonce-1",
      "/releases/c1-real-v1/engine.json?bench=nonce-1",
      "/releases/c1-real-v1/graph.json?bench=nonce-1",
      "/releases/c1-real-v1/shutoko_routing_bg.wasm?bench=nonce-1",
      "/releases/c1-real-v1/shutoko_routing.js?bench=nonce-1",
    ]);
    // クエリ付与でも照合・初期化は従来どおり成功する。
    expect(state.preparedGraph).toBeDefined();
    expect(initCalls).toHaveLength(1);
  });

  it("cacheBust 未指定・空文字では URL にクエリが付かない", async () => {
    const files = await artifactFiles();
    const withoutOptions = recordingFetch(files);
    await loadRelease(withoutOptions.fetch, "c1-real-v1", async () => stubGlue([]));
    expect(withoutOptions.seen).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
      "/releases/c1-real-v1/graph.json",
      "/releases/c1-real-v1/shutoko_routing_bg.wasm",
      "/releases/c1-real-v1/shutoko_routing.js",
    ]);

    const emptyBust = recordingFetch(files);
    await loadRelease(emptyBust.fetch, "c1-real-v1", async () => stubGlue([]), undefined, {
      cacheBust: "",
    });
    expect(emptyBust.seen.some((url) => url.includes("bench="))).toBe(false);
  });

  it("nonce は URL エンコードして載せる", async () => {
    const { fetch, seen } = recordingFetch(await artifactFiles());
    await loadRelease(fetch, "c1-real-v1", async () => stubGlue([]), undefined, {
      cacheBust: "a b&c",
    });
    expect(seen[0]).toBe("/releases/c1-real-v1/manifest.json?bench=a%20b%26c");
  });
});

describe("buildResultResponse", () => {
  const result = { status: "ok", candidates: [] } as unknown as SearchResult;

  it("bench 未指定のときは bench キーを持たない（通常 UI の応答形）", () => {
    const response = buildResultResponse("request-1", result);
    expect(Object.keys(response)).toEqual(["type", "requestId", "result"]);
    expect(Object.hasOwn(response, "bench")).toBe(false);
    expect(JSON.stringify(response)).not.toContain("bench");
  });

  it("bench 指定時はそのまま載せる", () => {
    const bench = {
      marks: {
        loadStartEpochMs: 1,
        loadEndEpochMs: 2,
        searchStartEpochMs: 3,
        searchEndEpochMs: 4,
      },
      resources: [],
      memory: { beforeMiB: 1, afterMiB: 2 },
    };
    const response = buildResultResponse("request-2", result, bench);
    expect(response).toMatchObject({ type: "result", requestId: "request-2", bench });
  });
});

describe("loadRelease（モック fetch）", () => {
  const graphBytes = encoder.encode(JSON.stringify({ schemaVersion: 2, releaseId: "c1-real-v1", vehicleProfile: "passenger-car-etc", nodes: [], edges: [], billingPairs: [] }));
  const wasmBytes = new Uint8Array([0, 0x61, 0x73, 0x6d]);
  const glueText = "export default function(){};export function search(){return '{}'}";

  const glueBytes = encoder.encode(glueText);

  interface BuildOptions {
    tamperGraph?: boolean;
    /** 配信する wasm を別バイト列に差し替える（engine.json の期待値は正しいまま）。 */
    tamperWasm?: boolean;
    /** engine.json の中身を差し替える（形式不正・エントリ欠落などの異常系用）。 */
    engineOverride?: string;
    /** engine.json 自体を配信しない（404 相当）。 */
    omitEngine?: boolean;
  }

  async function buildFiles(options: BuildOptions = {}): Promise<Record<string, Uint8Array>> {
    const { tamperGraph = false, tamperWasm = false } = options;
    const servedGraph = tamperGraph ? encoder.encode('{"nodes":[],"edges":[],}') : graphBytes;
    const servedWasm = tamperWasm ? new Uint8Array([0, 0x61, 0x73, 0x6e]) : wasmBytes;
    const graphExpected = await expectationOf(graphBytes);
    const manifest = JSON.stringify({
      releaseId: "c1-real-v1",
      artifacts: [{ path: "graph.json", sha256: graphExpected.sha256, byteLength: graphExpected.byteLength }],
    });
    // engine.json は実ファイルから計算する（固定値を書かない）。
    const engine = JSON.stringify({
      schemaVersion: 1,
      releaseId: "c1-real-v1",
      artifacts: [
        { path: "shutoko_routing_bg.wasm", ...(await expectationOf(wasmBytes)) },
        { path: "shutoko_routing.js", ...(await expectationOf(glueBytes)) },
      ],
    });
    const files: Record<string, Uint8Array> = {
      "/releases/c1-real-v1/manifest.json": encoder.encode(manifest),
      "/releases/c1-real-v1/graph.json": servedGraph,
      "/releases/c1-real-v1/shutoko_routing_bg.wasm": servedWasm,
      "/releases/c1-real-v1/shutoko_routing.js": glueBytes,
    };
    if (!options.omitEngine) {
      files["/releases/c1-real-v1/engine.json"] = encoder.encode(options.engineOverride ?? engine);
    }
    return files;
  }

  it("manifest → engine.json → graph → wasm → glue の順で取得し、初期化して検索境界を返す", async () => {
    const files = await buildFiles();
    const { fetch, calls } = mockFetch(files);
    const initCalls: unknown[] = [];
    const state = await loadRelease(fetch, "c1-real-v1", async () => stubGlue(initCalls));

    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
      "/releases/c1-real-v1/graph.json",
      "/releases/c1-real-v1/shutoko_routing_bg.wasm",
      "/releases/c1-real-v1/shutoko_routing.js",
    ]);
    expect(state.preparedGraph).toBeDefined();
    expect(JSON.parse(state.searchPrepared("{}"))).toEqual({
      status: "ok",
      candidates: [],
    });
    expect(initCalls).toHaveLength(1);
    expect(initCalls[0]).toBeInstanceOf(Uint8Array);
  });

  it("prepare には 46km cap の明示 limits を渡し、既定 30km に依存しない", async () => {
    const files = await buildFiles();
    const { fetch } = mockFetch(files);
    const prepareCalls: { graphJson: string; limitsJson: string }[] = [];
    const glue: WasmGlueModule = {
      default: async () => {},
      prepare(graphJson: string, limitsJson: string): WasmPreparedGraphLike {
        prepareCalls.push({ graphJson, limitsJson });
        return { free() {} };
      },
      searchPrepared: () => "{}",
    };
    await loadRelease(fetch, "c1-real-v1", async () => glue);

    expect(prepareCalls).toHaveLength(1);
    expect(prepareCalls[0]?.limitsJson).toBe(SEARCH_LIMITS_JSON);
    expect(JSON.parse(prepareCalls[0]?.limitsJson ?? "{}")).toEqual({
      maxAccessDistanceMeters: MAX_ACCESS_DISTANCE_METERS,
      deviceVerification: {
        manifestJson: DEVICE_VERIFICATION_MANIFEST_JSON,
        evaluatedAt: DEVICE_VERIFICATION_EVALUATED_AT,
      },
    });
    // 導出（240*60/2*(30/3.6)/1.3 ≒ 46 153.8 m）を下回り、旧既定 30km を上回ること。
    // 解析上界 46 153.8 m より小さい cap は「アクセス往復だけで製品上限に届く」地点を
    // 除くだけなので、時間窓で成立し得る候補を隠さない。
    expect(MAX_ACCESS_DISTANCE_METERS).toBe(46_000);
    expect(MAX_ACCESS_DISTANCE_METERS).toBeLessThan(46_154);
    expect(MAX_ACCESS_DISTANCE_METERS).toBeGreaterThan(30_000);
  });

  it("合格した synthetic manifest を loadRelease の build-time limits に渡せる", async () => {
    const files = await buildFiles();
    const { fetch } = mockFetch(files);
    const manifest = JSON.parse(pendingDeviceManifest) as Record<string, unknown>;
    for (const record of manifest.verifications as Record<string, unknown>[]) {
      record.osVersion = "test-os";
      record.clientVersion = "test-client";
      record.verifiedAt = "2026-09-24T00:00:00Z";
      record.result = "passed";
      record.expiresAt = "2026-10-24T00:00:00Z";
    }
    let limitsJson = "";
    const glue: WasmGlueModule = {
      default: async () => {},
      prepare(_graphJson: string, limits: string): WasmPreparedGraphLike {
        limitsJson = limits;
        return { free() {} };
      },
      searchPrepared: () => "{}",
    };
    await loadRelease(fetch, "c1-real-v1", async () => glue, undefined, {
      deviceVerificationManifestJson: JSON.stringify(manifest),
      deviceVerificationEvaluatedAt: "2026-09-25T00:00:00Z",
    });
    expect(JSON.parse(limitsJson).deviceVerification.manifestJson).toBe(JSON.stringify(manifest));
  });

  it("graph.json 改ざん時は ARTIFACT_MISMATCH で停止し、以降の fetch を呼ばない", async () => {
    const files = await buildFiles({ tamperGraph: true });
    const { fetch, calls } = mockFetch(files);
    const initCalls: unknown[] = [];
    await expect(
      loadRelease(fetch, "c1-real-v1", async () => stubGlue(initCalls)),
    ).rejects.toMatchObject({ code: "ARTIFACT_MISMATCH" });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
      "/releases/c1-real-v1/graph.json",
    ]);
    expect(initCalls).toHaveLength(0);
  });

  it("engine.json の wasm 期待値と不一致なら ARTIFACT_MISMATCH で停止する", async () => {
    const files = await buildFiles({ tamperWasm: true });
    const { fetch, calls } = mockFetch(files);
    const initCalls: unknown[] = [];
    await expect(
      loadRelease(fetch, "c1-real-v1", async () => stubGlue(initCalls)),
    ).rejects.toMatchObject({ code: "ARTIFACT_MISMATCH" });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
      "/releases/c1-real-v1/graph.json",
      "/releases/c1-real-v1/shutoko_routing_bg.wasm",
    ]);
    expect(initCalls).toHaveLength(0);
  });

  it("engine.json が 404 なら FETCH_FAILED", async () => {
    const files = await buildFiles({ omitEngine: true });
    const { fetch, calls } = mockFetch(files);
    await expect(loadRelease(fetch, "c1-real-v1")).rejects.toMatchObject({
      code: "FETCH_FAILED",
    });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
    ]);
  });

  it("engine.json が JSON でなければ ARTIFACT_MISMATCH", async () => {
    const files = await buildFiles({ engineOverride: "not json" });
    const { fetch, calls } = mockFetch(files);
    await expect(loadRelease(fetch, "c1-real-v1")).rejects.toMatchObject({
      code: "ARTIFACT_MISMATCH",
    });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
    ]);
  });

  it("engine.json の releaseId/schemaVersion 不一致は ARTIFACT_MISMATCH", async () => {
    const files = await buildFiles({
      engineOverride: JSON.stringify({
        schemaVersion: 1,
        releaseId: "c1-real-v2",
        artifacts: [],
      }),
    });
    const { fetch, calls } = mockFetch(files);
    await expect(loadRelease(fetch, "c1-real-v1")).rejects.toMatchObject({
      code: "ARTIFACT_MISMATCH",
    });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
    ]);
  });

  it("engine.json に glue のエントリが無ければ ARTIFACT_MISMATCH（graph 以降は取得しない）", async () => {
    const files = await buildFiles({
      engineOverride: JSON.stringify({
        schemaVersion: 1,
        releaseId: "c1-real-v1",
        artifacts: [
          { path: "shutoko_routing_bg.wasm", ...(await expectationOf(wasmBytes)) },
        ],
      }),
    });
    const { fetch, calls } = mockFetch(files);
    await expect(loadRelease(fetch, "c1-real-v1")).rejects.toMatchObject({
      code: "ARTIFACT_MISMATCH",
    });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/engine.json",
    ]);
  });

  it("fetch 失敗は FETCH_FAILED", async () => {
    const failing: FetchLike = async (url: string) => {
      throw new Error(`offline: ${url}`);
    };
    await expect(loadRelease(failing, "c1-real-v1")).rejects.toBeInstanceOf(PipelineError);
    await expect(loadRelease(failing, "c1-real-v1")).rejects.toMatchObject({
      code: "FETCH_FAILED",
    });
  });

  function getClosureVariables(fn: Function): Promise<Record<string, unknown>> {
    return new Promise((resolve) => {
      const session = new inspector.Session();
      session.connect();
      const vars: Record<string, unknown> = {};
      session.post("Runtime.enable", () => {
        (globalThis as unknown as { __closure_target?: Function }).__closure_target = fn;
        session.post("Runtime.evaluate", { expression: "globalThis.__closure_target" }, (err, evalRes: any) => {
          if (err || !evalRes?.result?.objectId) {
            session.disconnect();
            delete (globalThis as unknown as { __closure_target?: unknown }).__closure_target;
            return resolve(vars);
          }
          session.post("Runtime.getProperties", { objectId: evalRes.result.objectId }, (err, propRes: any) => {
            const scopes = propRes?.internalProperties?.find((p: any) => p.name === "[[Scopes]]");
            if (!scopes?.value?.objectId) {
              session.disconnect();
              delete (globalThis as unknown as { __closure_target?: unknown }).__closure_target;
              return resolve(vars);
            }
            session.post("Runtime.getProperties", { objectId: scopes.value.objectId }, (err, scopeList: any) => {
              const closureScopes = (scopeList?.result || []).filter(
                (s: any) =>
                  s?.value?.description?.startsWith("Closure") ||
                  s?.value?.description?.startsWith("Block"),
              );
              if (closureScopes.length === 0) {
                session.disconnect();
                delete (globalThis as unknown as { __closure_target?: unknown }).__closure_target;
                return resolve(vars);
              }
              let pending = closureScopes.length;
              for (const scope of closureScopes) {
                if (!scope?.value?.objectId) {
                  pending--;
                  if (pending === 0) {
                    session.disconnect();
                    delete (globalThis as unknown as { __closure_target?: unknown }).__closure_target;
                    resolve(vars);
                  }
                  continue;
                }
                session.post("Runtime.getProperties", { objectId: scope.value.objectId }, (err, varList: any) => {
                  for (const v of varList?.result || []) {
                    vars[v.name] = v.value?.value;
                  }
                  pending--;
                  if (pending === 0) {
                    session.disconnect();
                    delete (globalThis as unknown as { __closure_target?: unknown }).__closure_target;
                    resolve(vars);
                  }
                });
              }
            });
          });
        });
      });
    });
  }

  it("loadRelease 後に graphJson がプロパティおよびクロージャ変数として保持されていない", async () => {
    const files = await buildFiles();
    const { fetch } = mockFetch(files);
    const state = await loadRelease(fetch, "c1-real-v1", async () => stubGlue([]));

    // 1. プロパティとして保持されていないこと
    expect(state).not.toHaveProperty("graphJson");
    expect((state as unknown as { graphJson?: unknown }).graphJson).toBeUndefined();

    // 2. searchPrepared のクロージャ変数として保持されていないこと
    const closureVars = await getClosureVariables(state.searchPrepared);
    expect(closureVars).not.toHaveProperty("graphJson");
    expect(closureVars).not.toHaveProperty("graphBytes");

    // 3. 別名ローカル変数を介した保持も検出（文字列内容がクロージャ変数値に含まれていないこと）
    const graphJsonString = new TextDecoder().decode(graphBytes);
    const leakedValue = Object.values(closureVars).some(
      (val) => typeof val === "string" && val.includes(graphJsonString),
    );
    expect(leakedValue).toBe(false);

    state.free();
  });

  it("LoadedRelease.free() 後の searchPrepared は USE_AFTER_FREE エラーを投げる", async () => {
    const files = await buildFiles();
    const { fetch } = mockFetch(files);
    const state = await loadRelease(fetch, "c1-real-v1", async () => stubGlue([]));

    state.free();

    expect(() => state.searchPrepared("{}")).toThrowError(PipelineError);
    expect(() => state.searchPrepared("{}")).toThrowError(/already been freed/);
    try {
      state.searchPrepared("{}");
    } catch (err) {
      expect((err as PipelineError).code).toBe("USE_AFTER_FREE");
    }

    expect(() => state.retain()).toThrowError(PipelineError);
  });

  it("LoadedRelease の retain / release による参照カウント管理で安全に解放される", async () => {
    const files = await buildFiles();
    let freedCount = 0;
    const glueStub: WasmGlueModule = {
      default: async () => {},
      prepare: () => ({
        free() {
          freedCount += 1;
        },
      }),
      searchPrepared: () => "{}",
    };
    const { fetch } = mockFetch(files);
    const state = await loadRelease(fetch, "c1-real-v1", async () => glueStub);

    state.retain(); // refCount: 1 -> 2
    state.release(); // refCount: 2 -> 1
    expect(freedCount).toBe(0); // まだ解放されない

    // 検索可能
    expect(state.searchPrepared("{}")).toBe("{}");

    state.release(); // refCount: 1 -> 0
    expect(freedCount).toBe(1); // ここで解放

    // 解放後の追加 release や free は二重 free しない
    state.release();
    state.free();
    expect(freedCount).toBe(1);
  });

  it("ReleaseStore: releaseId 切り替え時に古いリリースの free() が呼ばれる", async () => {
    const files1 = await buildFiles();
    let freed1 = 0;
    let freed2 = 0;
    const glueStub = (id: string): WasmGlueModule => ({
      default: async () => {},
      prepare: () => ({
        free() {
          if (id === "v1") freed1 += 1;
          if (id === "v2") freed2 += 1;
        },
      }),
      searchPrepared: () => "{}",
    });

    const files2: Record<string, Uint8Array> = {};
    for (const [key, val] of Object.entries(files1)) {
      files2[key.replace("c1-real-v1", "c1-real-v2")] = val;
    }
    // engine.json と manifest.json の releaseId を更新
    const graph2 = JSON.parse(new TextDecoder().decode(files2["/releases/c1-real-v2/graph.json"]));
    graph2.releaseId = "c1-real-v2";
    files2["/releases/c1-real-v2/graph.json"] = encoder.encode(JSON.stringify(graph2));
    const engine2 = JSON.parse(new TextDecoder().decode(files2["/releases/c1-real-v2/engine.json"]));
    engine2.releaseId = "c1-real-v2";
    files2["/releases/c1-real-v2/engine.json"] = encoder.encode(JSON.stringify(engine2));
    const manifest2 = JSON.parse(new TextDecoder().decode(files2["/releases/c1-real-v2/manifest.json"]));
    manifest2.releaseId = "c1-real-v2";
    manifest2.artifacts[0] = {
      ...manifest2.artifacts[0],
      ...(await expectationOf(files2["/releases/c1-real-v2/graph.json"])),
    };
    files2["/releases/c1-real-v2/manifest.json"] = encoder.encode(JSON.stringify(manifest2));

    const allFiles = { ...files1, ...files2 };
    const { fetch } = mockFetch(allFiles);

    const store = new ReleaseStore({
      fetchImpl: fetch,
      importImpl: async (url) => {
        if (url.includes("c1-real-v1")) return glueStub("v1");
        return glueStub("v2");
      },
      knownReleases: ["c1-real-v1", "c1-real-v2"],
    });

    const state1 = await store.acquire("c1-real-v1");
    expect(store.currentLoaded?.releaseId).toBe("c1-real-v1");
    state1.release(); // caller 分を手放す（store キャッシュ分が保持）
    expect(freed1).toBe(0);

    const state2 = await store.acquire("c1-real-v2");
    expect(store.currentLoaded?.releaseId).toBe("c1-real-v2");
    expect(freed1).toBe(1); // v1 が解放された
    expect(freed2).toBe(0);

    state2.release();
    store.dispose();
    expect(freed2).toBe(1);
  });

  it("ReleaseStore: 新リリースの取得失敗時は古いリリースの free() は呼ばれない", async () => {
    const files = await buildFiles();
    let freedCount = 0;
    const glueStub: WasmGlueModule = {
      default: async () => {},
      prepare: () => ({
        free() {
          freedCount += 1;
        },
      }),
      searchPrepared: () => "{}",
    };
    const { fetch } = mockFetch(files);
    let failNext = false;
    const conditionalFetch: FetchLike = async (url, init) => {
      if (failNext && url.includes("c1-real-v2")) {
        throw new Error("network error");
      }
      return fetch(url, init);
    };

    const store = new ReleaseStore({
      fetchImpl: conditionalFetch,
      importImpl: async () => glueStub,
      knownReleases: ["c1-real-v1", "c1-real-v2"],
    });

    const state1 = await store.acquire("c1-real-v1");
    state1.release();
    expect(freedCount).toBe(0);

    failNext = true;
    await expect(store.acquire("c1-real-v2")).rejects.toThrow();
    expect(freedCount).toBe(0); // v1 は解放されていない
    expect(store.currentLoaded?.releaseId).toBe("c1-real-v1");

    store.dispose();
    expect(freedCount).toBe(1);
  });

  it("ReleaseStore: ロード進行中に dispose() された場合、ロード完了時にキャッシュへ載せずその場で PreparedGraph を解放する", async () => {
    const files = await buildFiles();
    let freedCount = 0;
    let isFreed = false;
    const glueStub: WasmGlueModule = {
      default: async () => {},
      prepare: () => ({
        free() {
          if (isFreed) {
            throw new Error("DOUBLE FREE");
          }
          isFreed = true;
          freedCount += 1;
        },
      }),
      searchPrepared: () => "{}",
    };

    let resolveFetch: (() => void) | null = null;
    const fetchPromise = new Promise<void>((resolve) => {
      resolveFetch = resolve;
    });

    const { fetch } = mockFetch(files);
    const controlledFetch: FetchLike = async (url, init) => {
      if (url.includes("graph.json")) {
        // graph.json の取得を保留
        await fetchPromise;
      }
      return fetch(url, init);
    };

    const store = new ReleaseStore({
      fetchImpl: controlledFetch,
      importImpl: async () => glueStub,
      knownReleases: ["c1-real-v1"],
    });

    // 1. ロード開始（in-flight にする）
    const acquirePromise = store.acquire("c1-real-v1");

    // 2. ロード進行中に store を破棄
    store.dispose();
    expect(store.currentLoaded).toBeNull();
    expect(freedCount).toBe(0);

    // 3. fetch を解放してロードを完了させる
    resolveFetch!();

    // 4. acquirePromise は DISPOSED で reject されること
    await expect(acquirePromise).rejects.toThrow();

    // 5. キャッシュが復活していないこと
    expect(store.currentLoaded).toBeNull();

    // 6. dispose 後に完了した PreparedGraph がその場で解放されていること（メモリリーク防止）
    expect(freedCount).toBe(1);

    // 7. dispose 後の acquire() は即座に拒否されること
    await expect(store.acquire("c1-real-v1")).rejects.toThrow();
  });
});
