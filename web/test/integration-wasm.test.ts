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
  routeMembershipsSha256,
  SEARCH_LIMITS_JSON,
  type ArtifactExpectation,
  type FetchLike,
  type FetchResponseLike,
} from "../src/worker/pipeline";
import representativeLocations from "../../fixtures/representative-locations.json";
import {
  MAX_DISPLAY_CANDIDATES,
  accessSecondsFromMeters,
  isProductEligible,
  recommendedLabel,
  toCardModel,
} from "../src/ui/model";
import {
  OFFICIAL_DISTANCE_RULE_SOURCE,
  PRODUCT_FARE_BASIS,
  PRODUCT_FARE_LABEL,
  PRODUCT_PAYMENT_METHOD,
  PRODUCT_VEHICLE_CLASS,
  TARIFF_MODEL_VERSION,
} from "../src/worker/tariff-contract";
import type { Candidate, UiSearchMessage } from "../src/worker/types";
import type { OdTariffsFileV3, TariffPriceV3 } from "../../crates/routing-wasm/types/index.d";

const root = new URL("../../", import.meta.url);
const wasmDir = new URL("dist/wasm/", root);
const gluePath = new URL("shutoko_routing.js", wasmDir);

interface WasmBuildContract {
  contractVersion: number;
  engineVersion: string;
  graphSchemaVersion: number;
  supportedGraphSchemaVersions: number[];
  tariffModelVersion: number;
  requiredGraphFields: string[];
  requiredGraphSchema4Fields: string[];
  requiredCandidateTollFields: string[];
  productTariff: {
    vehicleClass: string;
    paymentMethod: string;
    fareBasis: string;
    fareLabel: string;
    tollSource: string;
    discountsExcluded: boolean;
  };
}

function toBytes(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function toBinary(bytes: Uint8Array): Uint8Array {
  return bytes;
}

/**
 * graph.json の billingPairs[].tariff が build contract の製品スコープと一致するか。
 * tariff を持たない pair（旧 release）は後方互換のためそのまま受け入れる。
 */
function matchesProductTariff(
  value: unknown,
  product: WasmBuildContract["productTariff"],
): boolean {
  if (value === undefined || value === null) return true;
  if (typeof value !== "object" || Array.isArray(value)) return false;
  const tariff = value as Record<string, unknown>;
  return (
    tariff.vehicleClass === product.vehicleClass &&
    tariff.paymentMethod === product.paymentMethod &&
    tariff.fareBasis === product.fareBasis &&
    tariff.fareLabel === product.fareLabel &&
    tariff.discountsExcluded === product.discountsExcluded &&
    (tariff.status !== "priced" || tariff.tollSource === product.tollSource)
  );
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
    manifest.schemaVersion !== 1 ||
    manifest.releaseId !== graph.releaseId ||
    manifest.engineVersion !== current.engineVersion ||
    !Array.isArray(graph.odTariffs) ||
    !Array.isArray(graph.ramps)
  ) {
    return false;
  }
  if (
    graph.schemaVersion === 4 &&
    (!Array.isArray(graph.routeMemberships) ||
      !Array.isArray(graph.billingPairs) ||
      manifest.graphSchemaVersion !== 4 ||
      manifest.routePlanVersion !== 1 ||
      manifest.billingPairsVersion !== "v2" ||
      typeof manifest.routeMembershipsSha256 !== "string")
  ) {
    return false;
  }
  if (manifest.tariffModelVersion !== undefined && manifest.tariffModelVersion !== current.tariffModelVersion) {
    return false;
  }
  if (
    graph.schemaVersion === 4 &&
    !(graph.billingPairs as unknown[]).every((value) => {
      if (typeof value !== "object" || value === null) return false;
      const pair = value as Record<string, unknown>;
      if (typeof pair.pairKind !== "string") return false;
      if (!matchesProductTariff(pair.tariff, current.productTariff)) return false;
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
  const parsedGraph = JSON.parse(graph) as Record<string, unknown>;
  const parsedManifest = JSON.parse(manifest) as Record<string, unknown>;
  if (!isWasmContractCompatible(
    JSON.parse(current) as WasmBuildContract,
    JSON.parse(built) as WasmBuildContract,
    parsedGraph,
    parsedManifest,
  )) {
    return false;
  }
  if (parsedGraph.schemaVersion !== 4) return true;
  return (await routeMembershipsSha256(parsedGraph.routeMemberships)) === parsedManifest.routeMembershipsSha256;
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

describe("build contract と Web の製品スコープ", () => {
  it("wasm-contract.json の productTariff が Web の定数と一致する", async () => {
    const contract = JSON.parse(
      await readFile(new URL("crates/routing-wasm/wasm-contract.json", root), "utf8"),
    ) as WasmBuildContract;
    expect(contract.tariffModelVersion).toBe(TARIFF_MODEL_VERSION);
    expect(contract.productTariff).toEqual({
      vehicleClass: PRODUCT_VEHICLE_CLASS,
      paymentMethod: PRODUCT_PAYMENT_METHOD,
      fareBasis: PRODUCT_FARE_BASIS,
      fareLabel: PRODUCT_FARE_LABEL,
      tollSource: OFFICIAL_DISTANCE_RULE_SOURCE,
      discountsExcluded: true,
    });
    // 候補の toll に期待するフィールドが、build contract の宣言とずれていないこと。
    expect(contract.requiredCandidateTollFields).toContain("fareLabel");
    expect(contract.requiredCandidateTollFields).toContain("ruleId");
    expect(contract.requiredCandidateTollFields).toContain("evidenceId");
    expect(contract.requiredCandidateTollFields).toContain("assignmentId");
  });
});

describe("実 WASM 統合（fetch モック → loadRelease → search）", () => {
  it("旧graph契約のbuild metadataをstaleとして判定する", () => {
    const current: WasmBuildContract = {
      contractVersion: 5,
      engineVersion: "0.1.0",
      graphSchemaVersion: 4,
      supportedGraphSchemaVersions: [2, 3, 4],
      tariffModelVersion: 1,
      requiredGraphFields: ["odTariffs", "ramps[].id", "ramps[].mainlineNodeId"],
      requiredGraphSchema4Fields: [
        "routeMemberships",
        "billingPairs[].pairKind",
        "billingPairs[].anchor.anchorKind",
        "billingPairs[].routePlan.anchor.anchorKind",
      ],
      requiredCandidateTollFields: [
        "amountYen",
        "pricingAt",
        "effectiveFrom",
        "effectiveTo",
        "billingDistanceMeters",
        "tollSource",
        "assignmentId",
        "ruleId",
        "evidenceId",
        "distanceEvidenceId",
        "fareLabel",
        "vehicleClass",
        "paymentMethod",
        "fareBasis",
        "discountsExcluded",
      ],
      productTariff: {
        vehicleClass: "ordinary",
        paymentMethod: "etc",
        fareBasis: "base_toll_excluding_discounts",
        fareLabel: "普通車ETC基本料金（割引適用前）",
        tollSource: "official_distance_rule",
        discountsExcluded: true,
      },
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
    const result = await parseSearchResult(state.searchPrepared(JSON.stringify(buildSearchRequest(msg))));
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
      const result = await parseSearchResult(
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
      const result = await parseSearchResult(first);
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
      const result = await parseSearchResult(
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

  it("release gate の passed manifest を WASM 境界と Web reader まで通す", async () => {
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    const glue = await import(gluePath.href);
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(
      new URL("fixtures/graph-v4/graph-radial-fixture.json", root),
      "utf8",
    );
    const manifest = JSON.parse(
      await readFile(new URL("data/device-verification-manifest.json", root), "utf8"),
    ) as Record<string, unknown>;
    for (const record of manifest.verifications as Record<string, unknown>[]) {
      record.osVersion = "test-os";
      record.clientVersion = "test-client";
      record.verifiedAt = "2026-09-24T00:00:00Z";
      record.result = "passed";
      record.expiresAt = "2026-10-24T00:00:00Z";
    }
    const pg = glue.prepare(
      graphJson,
      JSON.stringify({
        deviceVerification: {
          manifestJson: JSON.stringify(manifest),
          evaluatedAt: "2026-09-25T00:00:00Z",
        },
      }),
    );
    try {
      const result = await parseSearchResult(
        glue.searchPrepared(
          pg,
          JSON.stringify({
            requestId: "device-gate-wasm",
            releaseId: "graph-v4-fixture-v1",
            originNodeId: "fixture:node:entry:ground",
            minMinutes: 1,
            maxMinutes: 60,
            vehicleProfile: "passenger-car-etc",
            pricingAt: "2020-01-01T00:00:00Z",
          }),
        ),
      );
      const candidate = result.candidates[0];
      expect(candidate?.pairKind).toBe("radialReturn");
      if (candidate?.pairKind === "radialReturn") {
        expect(candidate.handoff.enabled).toBe(true);
        if (candidate.handoff.enabled) {
          expect(candidate.handoff.legUrls).toHaveLength(3);
        }
      }
    } finally {
      pg.free();
    }
  }, 30_000);

  it("目黒座標は最近接の目黒入口を動的 OD として選び time_per_yen のradial候補を返す", async () => {
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
        const result = await parseSearchResult(first);
        expect(result.expandedStates).toBeLessThanOrEqual(100_000);
        if (minMinutes === 15) {
          expect(result.status).toBe("ok");
          expect(result.candidates.length).toBeGreaterThan(0);
          expect(result.rankingMode).toBe("time_per_yen");
          for (const candidate of result.candidates) {
            // 最近接入口優先: 返る入口はすべて目黒入口、radial出口は天現寺。
            expect(candidate.entry.rampId).toBe("ramp:2-inbound:meguro-entry");
            expect(candidate.exit.rampId).toBe("ramp:2-outbound:tengenji-exit");
            // 2026-09-10のradial ODは790円、19400mの公式セルで価格付け済み。
            expect(candidate.toll.amountYen).toBe(790);
            expect(candidate.toll.billingDistanceMeters).toBe(19400);
          }
        } else {
          expect(result.status).toBe("no_candidates");
          expect(result.reason).toBe("TIME_WINDOW");
          expect(result.candidates).toHaveLength(0);
          expect(result.rankingMode).toBe("shutoko_time");
        }
      }
    } finally {
      pg.free();
    }
  }, 60_000);
});

/**
 * 4 地点（東京駅・目黒駅・銀座・六本木）の fixture 検証。
 *
 * 地点の座標・時間枠・期待する候補は fixtures/representative-locations.json に
 * 置いてある。実 WASM の探索結果を行ごとに照合し、さらに次を必ず確かめる。
 *
 * - 公開 release と fixture の releaseId が一致する（release を差し替えたら
 *   fixture も更新する。さもないと期待値が現状の検証を黙って通してしまう）
 * - 表示する候補は最大 3 件（カタログの検証済み OD 数とは別の制限）
 * - 推薦バッジは先頭 1 件だけに付く
 * - 金額が確定した候補は data/od-tariffs.json（公式 PDF 由来の料金表）の
 *   pricingAt 時点の記録と金額・規則 ID・証拠 ID・適用期間が一致する
 * - 画面には基本料金（割引適用前）のラベルと、円あたり効率が基本料金での
 *   比較であることを示す
 */
interface LocationFixture {
  id: string;
  label: string;
  origin: { lat: number; lon: number };
  minMinutes: number;
  maxMinutes: number;
  expectedStatus: string;
  expectedRankingMode: string;
  expectedNearestAccessNodeId: string;
  expectedRecommendedPairId: string | null;
  expectedCandidates: {
    pairId: string;
    pairKind: string;
    entryRampId: string | null;
    exitRampId: string | null;
    entryName: string | null;
    exitName: string | null;
    productEligible: boolean;
    tariffStatus: string;
    amountYen: number | null;
    billingDistanceMeters: number | null;
    assignmentId: string | null;
    ruleId: string | null;
    evidenceId: string | null;
    effectiveFrom: string | null;
    effectiveTo: string | null;
  }[];
}

interface RepresentativeLocationsFixture {
  releaseId: string;
  vehicleProfile: string;
  pricingAt: string;
  fareLabel: string;
  tollSource: string;
  maxDisplayedCandidates: number;
  locations: LocationFixture[];
}

const locationsFixture = representativeLocations as RepresentativeLocationsFixture;

/** pricingAt の時点で適用されている od-tariffs.json の価格レコードを探す。 */
function activePrice(
  catalog: OdTariffsFileV3,
  assignmentId: string,
  pricingAt: string,
): TariffPriceV3 | null {
  const assignment = catalog.assignments.find((item) => item.assignmentId === assignmentId);
  if (assignment === undefined) return null;
  const at = Date.parse(pricingAt);
  return (
    assignment.prices.find((price) => {
      const from = Date.parse(price.effectiveFrom);
      const to = price.effectiveTo === null ? Number.POSITIVE_INFINITY : Date.parse(price.effectiveTo);
      return from <= at && at < to;
    }) ?? null
  );
}

describe("代表 4 地点の fixture（実 WASM）", () => {
  it("公開 release が fixture と一致し、evidence は料金表 v3 の記録と一致する", async () => {
    const catalog = JSON.parse(
      await readFile(new URL("data/od-tariffs.json", root), "utf8"),
    ) as OdTariffsFileV3;
    // 料金表そのものは tariff v3 で、評価する engine の tariffModelVersion と揃える。
    expect(catalog.version).toBe(3);
    expect(catalog.fareLabel).toBe(locationsFixture.fareLabel);
    expect(catalog.vehicleProfile).toBe(locationsFixture.vehicleProfile);

    const glue = await import(gluePath.href);
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const releaseId = (JSON.parse(graphJson) as { releaseId: string }).releaseId;
    expect(releaseId).toBe(locationsFixture.releaseId);
    expect(locationsFixture.fareLabel).toBe(PRODUCT_FARE_LABEL);
    expect(locationsFixture.maxDisplayedCandidates).toBe(MAX_DISPLAY_CANDIDATES);

    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      for (const location of locationsFixture.locations) {
        const msg: UiSearchMessage = {
          type: "search",
          requestId: `representative-${location.id}`,
          releaseId,
          pricingAt: locationsFixture.pricingAt,
          origin: location.origin,
          minMinutes: location.minMinutes,
          maxMinutes: location.maxMinutes,
          vehicleProfile: locationsFixture.vehicleProfile,
        };
        const result = await parseSearchResult(
          glue.searchPrepared(pg, JSON.stringify(buildSearchRequest(msg))),
        );
        const where = `${location.label}（${location.id}）`;

        expect(result.status, where).toBe(location.expectedStatus);
        expect(result.rankingMode, where).toBe(location.expectedRankingMode);
        expect(result.nearestAccess?.nodeId ?? null, where).toBe(location.expectedNearestAccessNodeId);

        // 画面に出すのは先頭 3 件まで。engine がさらに返しても表示は 3 件に留める。
        const displayed = result.candidates.slice(0, MAX_DISPLAY_CANDIDATES);
        expect(displayed.length, where).toBeLessThanOrEqual(MAX_DISPLAY_CANDIDATES);
        expect(displayed.length, where).toBe(location.expectedCandidates.length);
        expect(
          displayed.map((candidate) => candidate.toll.billingPairId),
          where,
        ).toEqual(location.expectedCandidates.map((expected) => expected.pairId));

        // 推薦は先頭 1 件だけ。商品対象外の候補は推薦しない。
        const recommended = displayed
          .map((candidate, index) => ({ candidate, index }))
          .filter(({ candidate }) => recommendedLabel(candidate) !== null);
        expect(recommended.length, where).toBeLessThanOrEqual(1);
        expect(recommended[0]?.candidate.toll.billingPairId ?? null, where).toBe(
          location.expectedRecommendedPairId,
        );

        displayed.forEach((candidate, index) => {
          const expected = location.expectedCandidates[index];
          if (expected === undefined) return;
          const toll = candidate.toll;
          expect(candidate.pairKind ?? "legacyRing", where).toBe(expected.pairKind);
          expect(candidate.entry.rampId ?? null, where).toBe(expected.entryRampId);
          expect(candidate.exit.rampId ?? null, where).toBe(expected.exitRampId);
          expect(candidate.entry.name ?? null, where).toBe(expected.entryName);
          expect(candidate.exit.name ?? null, where).toBe(expected.exitName);
          expect(isProductEligible(candidate), where).toBe(expected.productEligible);
          const status = "tariffStatus" in candidate ? candidate.tariffStatus : undefined;
          expect(status ?? null, where).toBe(expected.tariffStatus);
          expect(toll.amountYen, where).toBe(expected.amountYen);
          expect(toll.billingDistanceMeters ?? null, where).toBe(expected.billingDistanceMeters);
          expect(toll.effectiveFrom, where).toBe(expected.effectiveFrom);
          expect(toll.effectiveTo, where).toBe(expected.effectiveTo);
          // 金額は必ず「普通車ETC基本料金（割引適用前）」で、割引を適用しない。
          expect(toll.fareLabel ?? null, where).toBe(locationsFixture.fareLabel);
          expect(toll.vehicleClass ?? null, where).toBe("ordinary");
          expect(toll.paymentMethod ?? null, where).toBe("etc");
          expect(toll.fareBasis ?? null, where).toBe("base_toll_excluding_discounts");
          expect(toll.discountsExcluded ?? false, where).toBe(true);

          if (expected.tariffStatus !== "priced") {
            // 確定しない候補は金額も証拠も持たない（parseSearchResult も検査している）。
            expect(toll.amountYen, where).toBeNull();
            expect(toll.assignmentId ?? null, where).toBeNull();
            expect(toll.ruleId ?? null, where).toBeNull();
            expect(toll.evidenceId ?? null, where).toBeNull();
            expect(toll.tollSource ?? null, where).toBeNull();
            return;
          }

          // 確定した金額は公式 PDF 由来の料金表（data/od-tariffs.json）の
          // 当該時点の記録と、金額・規則 ID・証拠 ID・適用期間まで一致する。
          expect(toll.tollSource ?? null, where).toBe(locationsFixture.tollSource);
          expect(toll.assignmentId ?? null, where).toBe(expected.assignmentId);
          expect(toll.ruleId ?? null, where).toBe(expected.ruleId);
          expect(toll.evidenceId ?? null, where).toBe(expected.evidenceId);
          expect(toll.distanceEvidenceId ?? null, where).toBe(expected.evidenceId);
          const price = activePrice(catalog, expected.assignmentId ?? "", locationsFixture.pricingAt);
          expect(price, `${where} ${expected.assignmentId ?? ""}`).not.toBeNull();
          if (price === null) return;
          expect(price.tariffStatus, where).toBe("priced");
          expect(price.amountYen, where).toBe(toll.amountYen);
          expect(price.ruleId, where).toBe(toll.ruleId);
          expect(price.evidenceId, where).toBe(toll.evidenceId);
          expect(price.effectiveFrom, where).toBe(toll.effectiveFrom);
          expect(price.effectiveTo, where).toBe(toll.effectiveTo);
          expect(price.observedDistanceMeters, where).toBe(toll.billingDistanceMeters);
          const assignment = catalog.assignments.find(
            (item) => item.assignmentId === expected.assignmentId,
          );
          expect(assignment?.vehicleProfile ?? null, where).toBe(locationsFixture.vehicleProfile);
          expect(assignment?.fareBasis ?? null, where).toBe("base_toll_excluding_discounts");
        });
      }
    } finally {
      pg.free();
    }
  }, 60_000);

  it("4 地点の候補カードが基本料金のラベルと効率の注記を出す", async () => {
    const glue = await import(gluePath.href);
    const wasmBytes = new Uint8Array(await readFile(new URL("shutoko_routing_bg.wasm", wasmDir)));
    await glue.default({ module_or_path: toBinary(wasmBytes) });
    const graphJson = await readFile(new URL("fixtures/generated/graph.json", root), "utf8");
    const releaseId = (JSON.parse(graphJson) as { releaseId: string }).releaseId;
    const pg = glue.prepare(graphJson, SEARCH_LIMITS_JSON);
    try {
      let pricedCards = 0;
      for (const location of locationsFixture.locations) {
        const msg: UiSearchMessage = {
          type: "search",
          requestId: `representative-card-${location.id}`,
          releaseId,
          pricingAt: locationsFixture.pricingAt,
          origin: location.origin,
          minMinutes: location.minMinutes,
          maxMinutes: location.maxMinutes,
          vehicleProfile: locationsFixture.vehicleProfile,
        };
        const result = await parseSearchResult(
          glue.searchPrepared(pg, JSON.stringify(buildSearchRequest(msg))),
        );
        const displayed = result.candidates
          .slice(0, MAX_DISPLAY_CANDIDATES)
          .map((candidate: Candidate, index) => toCardModel(candidate, index + 1));
        expect(displayed.length, location.label).toBe(location.expectedCandidates.length);
        for (const [index, model] of displayed.entries()) {
          const expected = location.expectedCandidates[index];
          if (expected === undefined) return;
          if (expected.tariffStatus !== "priced") {
            // 金額が未算出なら、料金のラベルも効率の注記も出さない。
            expect(model.fareLabelNote, location.label).toBeNull();
            expect(model.timePerYen, location.label).toBeNull();
            expect(model.timePerYenNote, location.label).toBeNull();
            continue;
          }
          pricedCards += 1;
          expect(model.toll, location.label).toContain(expected.amountYen?.toLocaleString("ja-JP") ?? "");
          expect(model.fareLabelNote, location.label).toBe(`上記は${PRODUCT_FARE_LABEL}です`);
          // 効率の比較は基本料金だと注記で明示する。
          expect(model.timePerYen, location.label).toContain("円あたり");
          expect(model.timePerYenNote, location.label).toBe(`（${PRODUCT_FARE_LABEL}で比較）`);
        }
      }
      // 4 地点のうち少なくとも 1 地点は金額を提示し、表示の検査が成立している。
      expect(pricedCards).toBeGreaterThan(0);
    } finally {
      pg.free();
    }
  }, 60_000);
});
