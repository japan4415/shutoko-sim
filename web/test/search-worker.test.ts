import { describe, expect, it } from "vitest";
import {
  hexDigest,
  ReleaseStore,
  type FetchLike,
  type FetchResponseLike,
  type WasmGlueModule,
  type WasmPreparedGraphLike,
} from "../src/worker/pipeline";
import { createSearchWorkerHandler } from "../src/worker/search-worker";
import type { UiSearchMessage, WorkerResponse } from "../src/worker/types";

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
  };
}

async function expectationOf(bytes: Uint8Array): Promise<{ sha256: string; byteLength: number }> {
  return {
    sha256: await hexDigest(bytes.slice().buffer as ArrayBuffer),
    byteLength: bytes.byteLength,
  };
}

async function buildReleaseFiles(releaseId: string): Promise<Record<string, Uint8Array>> {
  const graphBytes = encoder.encode(JSON.stringify({ releaseId, nodes: [], edges: [] }));
  const wasmBytes = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
  const glueBytes = encoder.encode(`export default function init() { return Promise.resolve(); }`);

  const graphExpected = await expectationOf(graphBytes);
  const wasmExpected = await expectationOf(wasmBytes);
  const glueExpected = await expectationOf(glueBytes);

  const manifest = JSON.stringify({
    releaseId,
    artifacts: [{ path: "graph.json", sha256: graphExpected.sha256, byteLength: graphExpected.byteLength }],
  });
  const engine = JSON.stringify({
    schemaVersion: 1,
    releaseId,
    artifacts: [
      { path: "shutoko_routing_bg.wasm", ...wasmExpected },
      { path: "shutoko_routing.js", ...glueExpected },
    ],
  });

  return {
    [`/releases/${releaseId}/manifest.json`]: encoder.encode(manifest),
    [`/releases/${releaseId}/engine.json`]: encoder.encode(engine),
    [`/releases/${releaseId}/graph.json`]: graphBytes,
    [`/releases/${releaseId}/shutoko_routing_bg.wasm`]: wasmBytes,
    [`/releases/${releaseId}/shutoko_routing.js`]: glueBytes,
  };
}

describe("search-worker: 並行リリース切り替え時のライフサイクル検証 (issue #31)", () => {
  it("loaded=null から release-a と release-b を同時ロードし同一ターン解決しても use-after-free が起きない", async () => {
    const filesA = await buildReleaseFiles("release-a");
    const filesB = await buildReleaseFiles("release-b");
    const allFiles = { ...filesA, ...filesB };

    const fetchImpl: FetchLike = async (url: string) => {
      const bytes = allFiles[url];
      if (bytes === undefined) {
        return {
          ok: false,
          status: 404,
          async arrayBuffer() {
            return new ArrayBuffer(0);
          },
          async text() {
            return "";
          },
        };
      }
      return responseFrom(bytes);
    };

    let freedA = 0;
    let freedB = 0;
    let searchedA = 0;
    let searchedB = 0;

    const createGlue = (id: string): WasmGlueModule => {
      let isFreed = false;
      const pg: WasmPreparedGraphLike = {
        free() {
          if (isFreed) {
            throw new Error(`DOUBLE FREE on ${id}`);
          }
          isFreed = true;
          if (id === "release-a") freedA += 1;
          if (id === "release-b") freedB += 1;
        },
      };

      return {
        default: async () => {},
        prepare: () => pg,
        searchPrepared: () => {
          if (isFreed) {
            throw new Error(`use after free: ${id}`);
          }
          if (id === "release-a") searchedA += 1;
          if (id === "release-b") searchedB += 1;
          return JSON.stringify({
            status: "ok",
            candidates: [
              {
                edgeIds: ["e1"],
                duration: { baseSeconds: 100, planSeconds: 120 },
                toll: { amountYen: 300, chargedSectionCount: 1 },
                geometry: { type: "LineString", coordinates: [] },
                handoff: { mapsUrl: "https://maps.example.com" },
                snappedOrigin: { nodeId: "n1", distanceMeters: 0 },
                warnings: [],
              },
            ],
          });
        },
      };
    };

    const importImpl = async (url: string): Promise<WasmGlueModule> => {
      if (url.includes("release-a")) {
        return createGlue("release-a");
      }
      return createGlue("release-b");
    };

    const store = new ReleaseStore({
      fetchImpl,
      importImpl,
      knownReleases: ["release-a", "release-b"],
    });

    const responses: WorkerResponse[] = [];
    const handler = createSearchWorkerHandler({
      post: (msg) => responses.push(msg),
      store,
      isBench: true, // boot の自動ロードを無効化
    });

    const msgA: UiSearchMessage = {
      type: "search",
      requestId: "req-a",
      releaseId: "release-a",
      pricingAt: "2026-09-10T00:00:00Z",
      vehicleProfile: "passenger-car-etc",
      originNodeId: "n1",
      minMinutes: 10,
      maxMinutes: 60,
    };

    const msgB: UiSearchMessage = {
      type: "search",
      requestId: "req-b",
      releaseId: "release-b",
      pricingAt: "2026-09-10T00:00:00Z",
      vehicleProfile: "passenger-car-etc",
      originNodeId: "n1",
      minMinutes: 10,
      maxMinutes: 60,
    };

    // release-a と release-b の検索を loaded=null から並行に開始
    const searchPromiseA = handler.handleSearch(msgA);
    const searchPromiseB = handler.handleSearch(msgB);

    // 両方の検索の完了を待機
    await Promise.all([searchPromiseA, searchPromiseB]);

    // 検証:
    // 1. エラーメッセージは一切返っていないこと（use-after-free やダブルフリーが発生していないこと）
    const errors = responses.filter((r) => r.type === "error");
    expect(errors).toEqual([]);

    // 2. req-a と req-b の両方について正常な result 応答が返っていること
    const results = responses.filter((r) => r.type === "result");
    expect(results).toHaveLength(2);

    const resultA = results.find((r) => r.type === "result" && r.requestId === "req-a");
    const resultB = results.find((r) => r.type === "result" && r.requestId === "req-b");
    expect(resultA).toBeDefined();
    expect(resultB).toBeDefined();
    expect(resultA?.type === "result" && resultA.result.status).toBe("ok");
    expect(resultB?.type === "result" && resultB.result.status).toBe("ok");

    // 3. 検索が両方とも PreparedGraph で実行されたこと
    expect(searchedA).toBe(1);
    expect(searchedB).toBe(1);

    // 4. release-a は検索完了後に安全に解放されたこと（free() がちょうど1回呼ばれていること）
    expect(freedA).toBe(1);

    // 5. release-b は最新の loaded キャッシュとして保持され、まだ free されていないこと
    expect(freedB).toBe(0);
    expect(store.currentLoaded?.releaseId).toBe("release-b");

    // 6. store.dispose() でキャッシュされていた release-b も安全に1回だけ free されること
    store.dispose();
    expect(freedB).toBe(1);
    expect(freedA).toBe(1); // release-a が二重解放されないこと
  });

  it("同一 releaseId への並行検索要求で二重ロードが発生せず両リクエストが成功する", async () => {
    const filesA = await buildReleaseFiles("release-a");
    let fetchCount = 0;
    const fetchImpl: FetchLike = async (url: string) => {
      if (url.endsWith("manifest.json")) {
        fetchCount += 1;
      }
      return responseFrom(filesA[url]!);
    };

    let searchCount = 0;
    let freedCount = 0;
    const glue: WasmGlueModule = {
      default: async () => {},
      prepare: () => ({
        free() {
          freedCount += 1;
        },
      }),
      searchPrepared: () => {
        searchCount += 1;
        return JSON.stringify({ status: "ok", candidates: [] });
      },
    };

    const store = new ReleaseStore({
      fetchImpl,
      importImpl: async () => glue,
      knownReleases: ["release-a"],
    });

    const responses: WorkerResponse[] = [];
    const handler = createSearchWorkerHandler({
      post: (msg) => responses.push(msg),
      store,
      isBench: true,
    });

    const search1 = handler.handleSearch({
      type: "search",
      requestId: "r1",
      releaseId: "release-a",
      pricingAt: "2026-09-10T00:00:00Z",
      vehicleProfile: "passenger-car-etc",
      minMinutes: 10,
      maxMinutes: 30,
    });
    const search2 = handler.handleSearch({
      type: "search",
      requestId: "r2",
      releaseId: "release-a",
      pricingAt: "2026-09-10T00:00:00Z",
      vehicleProfile: "passenger-car-etc",
      minMinutes: 10,
      maxMinutes: 30,
    });

    await Promise.all([search1, search2]);

    expect(fetchCount).toBe(1); // 二重 fetch なし
    expect(searchCount).toBe(2); // 両方探索実行
    expect(responses).toHaveLength(3); // ready + 2 results
    expect(freedCount).toBe(0); // キャッシュされているのでまだ free されない

    store.dispose();
    expect(freedCount).toBe(1); // dispose で 1 回だけ free
  });

  it("未知の releaseId に対しては ARTIFACT_MISMATCH エラーを返す", async () => {
    const store = new ReleaseStore({
      knownReleases: ["release-a"],
    });
    const responses: WorkerResponse[] = [];
    const handler = createSearchWorkerHandler({
      post: (msg) => responses.push(msg),
      store,
      isBench: true,
    });

    await handler.handleSearch({
      type: "search",
      requestId: "r-unknown",
      releaseId: "unknown-release",
      pricingAt: "2026-09-10T00:00:00Z",
      vehicleProfile: "passenger-car-etc",
      minMinutes: 10,
      maxMinutes: 30,
    });

    expect(responses).toHaveLength(1);
    expect(responses[0]).toMatchObject({
      type: "error",
      requestId: "r-unknown",
      code: "ARTIFACT_MISMATCH",
    });
  });
});
