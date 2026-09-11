// パイプライン純粋関数のユニットテスト（node 環境、fetch/import はモック注入）。
import { describe, expect, it } from "vitest";
import { ARTIFACT_HASHES } from "../src/worker/artifact-hashes";
import {
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
import type { UiSearchMessage } from "../src/worker/types";

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

describe("loadRelease（モック fetch）", () => {
  const graphBytes = encoder.encode('{"nodes":[],"edges":[]}');
  const wasmBytes = new Uint8Array([0, 0x61, 0x73, 0x6d]);
  const glueText = "export default function(){};export function search(){return '{}'}";

  async function buildFiles(tamperGraph = false): Promise<Record<string, Uint8Array>> {
    const servedGraph = tamperGraph ? encoder.encode('{"nodes":[],"edges":[],}') : graphBytes;
    const graphExpected = await expectationOf(graphBytes);
    const manifest = JSON.stringify({
      releaseId: "c1-real-v1",
      artifacts: [{ path: "graph.json", sha256: graphExpected.sha256, byteLength: graphExpected.byteLength }],
    });
    return {
      "/releases/c1-real-v1/manifest.json": encoder.encode(manifest),
      "/releases/c1-real-v1/graph.json": servedGraph,
      "/releases/c1-real-v1/shutoko_routing_bg.wasm": wasmBytes,
      "/releases/c1-real-v1/shutoko_routing.js": encoder.encode(glueText),
    };
  }

  it("manifest → graph → wasm → glue の順で取得し、初期化して検索境界を返す", async () => {
    const files = await buildFiles();
    // wasm/glue の期待ハッシュはモック内容に合わせ、注入用のハッシュ表を作る
    const hashes = {
      wasm: await expectationOf(wasmBytes),
      glue: await expectationOf(encoder.encode(glueText)),
    };
    const { fetch, calls } = mockFetch(files);
    const initCalls: unknown[] = [];
    const state = await loadRelease(fetch, "c1-real-v1", hashes, async () => stubGlue(initCalls));

    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
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
    const files = await buildFiles(true);
    const hashes = {
      wasm: await expectationOf(wasmBytes),
      glue: await expectationOf(encoder.encode(glueText)),
    };
    const { fetch, calls } = mockFetch(files);
    const initCalls: unknown[] = [];
    await expect(
      loadRelease(fetch, "c1-real-v1", hashes, async () => stubGlue(initCalls)),
    ).rejects.toMatchObject({ code: "ARTIFACT_MISMATCH" });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/graph.json",
    ]);
    expect(initCalls).toHaveLength(0);
  });

  it("wasm 固定ハッシュ不一致でも ARTIFACT_MISMATCH で停止する", async () => {
    const files = await buildFiles();
    const wrongHashes = {
      wasm: { sha256: "0".repeat(64), byteLength: wasmBytes.byteLength },
      glue: await expectationOf(encoder.encode(glueText)),
    };
    const { fetch, calls } = mockFetch(files);
    const initCalls: unknown[] = [];
    await expect(
      loadRelease(fetch, "c1-real-v1", wrongHashes, async () => stubGlue(initCalls)),
    ).rejects.toMatchObject({ code: "ARTIFACT_MISMATCH" });
    expect(calls).toEqual([
      "/releases/c1-real-v1/manifest.json",
      "/releases/c1-real-v1/graph.json",
      "/releases/c1-real-v1/shutoko_routing_bg.wasm",
    ]);
  });

  it("fetch 失敗は FETCH_FAILED", async () => {
    const failing: FetchLike = async (url: string) => {
      throw new Error(`offline: ${url}`);
    };
    const hashes = ARTIFACT_HASHES["c1-real-v1"];
    if (hashes === undefined) {
      throw new Error("hash table missing");
    }
    await expect(loadRelease(failing, "c1-real-v1", hashes)).rejects.toBeInstanceOf(PipelineError);
    await expect(
      loadRelease(failing, "c1-real-v1", hashes),
    ).rejects.toMatchObject({ code: "FETCH_FAILED" });
  });
});
