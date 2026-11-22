// 地図セグメント導出（src/map/segments.ts）のユニットテスト。
import { describe, expect, it } from "vitest";
import radialCandidateJson from "../../fixtures/candidate-v2/radial-valid.json?raw";
import { boundsOf, deriveSegments } from "../src/map/segments";
import type { LegacyCandidate, RadialCandidate, TopologyOnlyCandidate } from "../src/worker/types";

/**
 * 4 エッジ・5 座標のサンプル。edgeIds は access(e:acc) → loop(e:L1,e:L2) → return(e:ret)。
 * loop の連続部分列は index 1..2。entry/exit は loop 内の e:L1 / e:L2。
 */
function sampleCandidate(overrides: Partial<LegacyCandidate> = {}): LegacyCandidate {
  return {
    id: "cand-1",
    releaseId: "c1-real-v1",
    origin: { lat: 35.0, lon: 139.0 },
    originNodeId: "n:0",
    snappedOrigin: { nodeId: "n:0", lat: 35.0, lon: 139.0, distanceMeters: 0 },
    entry: { edgeId: "e:L1", name: "入口" },
    exit: { edgeId: "e:L2", name: "出口" },
    entryId: "e:L1",
    exitId: "e:L2",
    roadNames: ["首都高速都心環状線"],
    edgeIds: ["e:acc", "e:L1", "e:L2", "e:ret"],
    geometry: {
      type: "LineString",
      coordinates: [
        [139.0, 35.0], // access 始点
        [139.1, 35.1], // access 終点 = loop 始点
        [139.2, 35.2], // loop 中間 = entry 終点
        [139.3, 35.3], // loop 終点 = return 始点
        [139.4, 35.4], // return 終点
      ],
    },
    duration: {
      accessSeconds: 60,
      shutokoSeconds: 1503,
      returnSeconds: 120,
      baseSeconds: 1683,
      bufferSeconds: 121,
      planSeconds: 1804,
    },
    distanceMeters: 8000,
    shutokoDistanceMeters: 5000,
    toll: {
      billingPairId: "bp:x",
      chargedSectionCount: 1,
      amountYen: 300,
      pricingAt: "2026-09-10T00:00:00Z",
      effectiveFrom: null,
      effectiveTo: null,
    },
    loop: {
      anchorNodeId: "n:1",
      edgeIds: ["e:L1", "e:L2"],
      durationSeconds: 900,
      distanceMeters: 4600,
      validated: true,
    },
    reasons: ["BEST_TIME_PER_YEN"],
    warnings: [],
    handoff: {
      origin: { lat: 35.0, lon: 139.0 },
      destination: { lat: 35.0, lon: 139.0 },
      waypoints: [],
      mapsUrl: "https://www.google.com/maps/dir/?api=1",
      verificationSetVersion: null,
    },
    ...overrides,
  };
}

describe("deriveSegments", () => {
  it("legacy の実走経路を分解し、課金区間の線は推定しない", () => {
    const segments = deriveSegments(sampleCandidate());
    expect(segments.access).toEqual([
      [139.0, 35.0],
      [139.1, 35.1],
    ]);
    expect(segments.loop).toEqual([
      [139.1, 35.1],
      [139.2, 35.2],
      [139.3, 35.3],
    ]);
    expect(segments.return).toEqual([
      [139.3, 35.3],
      [139.4, 35.4],
    ]);
    expect(segments).not.toHaveProperty("charged");
    // main はセグメントに依らず全経路。
    expect(segments.main).toEqual(sampleCandidate().geometry.coordinates);
  });

  it("radialReturn は4 roleを描くため全経路のcharged overlayを作らない", () => {
    const candidate = JSON.parse(radialCandidateJson) as RadialCandidate;
    const segments = deriveSegments(candidate);
    expect(segments.access).toHaveLength(4);
    expect(segments.loop).toEqual([
      [139.103, 35.103],
      [139.104, 35.104],
      [139.105, 35.105],
    ]);
    expect(segments.return).toEqual([
      [139.105, 35.105],
      [139.106, 35.106],
      [139.107, 35.107],
    ]);
    expect(segments.routeLegs.map((leg) => [leg.role, leg.coords.length])).toEqual([
      ["entry_approach", 4],
      ["mandatory_lap", 3],
      ["return_corridor", 2],
      ["exit_approach", 2],
    ]);
    expect(segments).not.toHaveProperty("charged");
  });

  it("topologyOnly は道路形状を描画しても課金区間のオーバーレイを生成しない", () => {
    const value = JSON.parse(radialCandidateJson) as Record<string, unknown>;
    value.pairKind = "topologyOnly";
    value.eligibilityStatus = "topology_only";
    value.loopValidationStatus = "topology_only";
    value.loop = {
      anchorNodeId: "fixture:node:merge",
      edgeIds: ["fixture:edge:lap:1", "fixture:edge:lap:2"],
      durationSeconds: 1200,
      distanceMeters: 20000,
      validated: false,
    };
    value.reasons = ["TOPOLOGY_ONLY"];
    delete value.anchor;
    delete value.routePlan;
    delete value.edgeRouteLegs;
    const segments = deriveSegments(value as unknown as TopologyOnlyCandidate);
    expect(segments.loop).toHaveLength(3);
    expect(segments.routeLegs).toEqual([]);
    expect(segments).not.toHaveProperty("charged");
  });

  it("loop が先頭・末尾でも境界が成立する", () => {
    const segments = deriveSegments(
      sampleCandidate({
        edgeIds: ["e:L1", "e:L2", "e:ret"],
        geometry: {
          type: "LineString",
          coordinates: [
            [139.1, 35.1],
            [139.2, 35.2],
            [139.3, 35.3],
            [139.4, 35.4],
          ],
        },
        loop: {
          anchorNodeId: "n:1",
          edgeIds: ["e:L1", "e:L2"],
          durationSeconds: 900,
          distanceMeters: 4600,
          validated: true,
        },
      }),
    );
    expect(segments.access).toEqual([]);
    expect(segments.loop).toHaveLength(3);
    expect(segments.return).toEqual([
      [139.3, 35.3],
      [139.4, 35.4],
    ]);
  });

  it("座標数とエッジ数が不整合なら main フォールバック", () => {
    const segments = deriveSegments(
      sampleCandidate({
        geometry: { type: "LineString", coordinates: [[139.0, 35.0]] },
      }),
    );
    expect(segments.main).toEqual([[139.0, 35.0]]);
    expect(segments.access).toEqual([]);
    expect(segments.loop).toEqual([]);
    expect(segments.return).toEqual([]);
    expect(segments).not.toHaveProperty("charged");
  });

  it("loop が edgeIds の連続部分列でないなら main フォールバック", () => {
    const geometry = sampleCandidate().geometry.coordinates;
    const segments = deriveSegments(
      sampleCandidate({
        edgeIds: ["e:acc", "e:L1", "e:ret", "e:L2"],
        geometry: { type: "LineString", coordinates: geometry },
      }),
    );
    expect(segments.main).toEqual(geometry);
    expect(segments.loop).toEqual([]);
    expect(segments.access).toEqual([]);
  });

  it("空 geometry は main も空で例外を出さない", () => {
    const segments = deriveSegments(
      sampleCandidate({ geometry: { type: "LineString", coordinates: [] } }),
    );
    expect(segments).toEqual({ access: [], loop: [], return: [], routeLegs: [], main: [] });
  });

  it.each([{ entryId: "e:zzz" }, { entryId: "e:L2", exitId: "e:L1" }])(
    "入口・出口の ID が不整合でも実走経路は分解する (%j)",
    (overrides) => {
      expect(deriveSegments(sampleCandidate(overrides))).toEqual(deriveSegments(sampleCandidate()));
    },
  );

  it("loop が edgeIds の末尾にあるとき return は空になる", () => {
    // access(e:acc) → loop(e:L1,e:L2) の順で return エッジを持たない。
    const segments = deriveSegments(
      sampleCandidate({
        edgeIds: ["e:acc", "e:L1", "e:L2"],
        geometry: {
          type: "LineString",
          coordinates: [
            [139.0, 35.0],
            [139.1, 35.1],
            [139.2, 35.2],
            [139.3, 35.3],
          ],
        },
      }),
    );
    expect(segments.return).toEqual([]);
    expect(segments.access).toEqual([
      [139.0, 35.0],
      [139.1, 35.1],
    ]);
    expect(segments.loop).toHaveLength(3);
  });
});

describe("boundsOf", () => {
  it("外接矩形を [[minLon,minLat],[maxLon,maxLat]] で返す", () => {
    expect(
      boundsOf([
        [139.3, 35.3],
        [139.0, 35.0],
        [139.4, 35.4],
      ]),
    ).toEqual([
      [139.0, 35.0],
      [139.4, 35.4],
    ]);
  });

  it("空配列は null、非有限値は無視する", () => {
    expect(boundsOf([])).toBeNull();
    expect(
      boundsOf([
        [Number.NaN, 35.0],
        [139.0, 35.0],
      ]),
    ).toEqual([
      [139.0, 35.0],
      [139.0, 35.0],
    ]);
  });

  it("全て非有限の 1 点は null、空配列も null", () => {
    expect(boundsOf([[Number.NaN, Number.NaN]])).toBeNull();
    expect(boundsOf([])).toBeNull();
  });
});
