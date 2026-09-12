// パイプライン純粋関数のユニットテスト（node 環境、fetch/import はモック注入）。
import { describe, expect, it } from "vitest";
import {
  buildResultResponse,
  buildSearchRequest,
  hexDigest,
  loadRelease,
  parseWasmError,
  PipelineError,
  verifyArtifact,
  type FetchLike,
  type FetchResponseLike,
  type WasmGlueModule,
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
  const graphBytes = encoder.encode('{"nodes":[],"edges":[]}');
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
    expect(state.graphJson).toBe('{"nodes":[],"edges":[]}');
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
  const graphBytes = encoder.encode('{"nodes":[],"edges":[]}');
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
    expect(state.graphJson).toBe('{"nodes":[],"edges":[]}');
    expect(JSON.parse(state.search(state.graphJson, "{}", "{}"))).toEqual({
      status: "ok",
      candidates: [],
    });
    expect(initCalls).toHaveLength(1);
    expect(initCalls[0]).toBeInstanceOf(Uint8Array);
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
});
