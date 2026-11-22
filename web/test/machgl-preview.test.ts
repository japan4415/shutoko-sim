// 3Dプレビューへ渡すペイロード（src/map/machgl-preview.ts）のユニットテスト。
// 地図ライブラリに依存しない純粋な変換だけを検証する。
import { describe, expect, it } from "vitest";
import radialCandidateJson from "../../fixtures/candidate-v2/radial-valid.json?raw";
import { buildPreviewPayload } from "../src/map/machgl-preview";
import { toCardModel } from "../src/ui/model";
import type { LegacyCandidate, RadialCandidate } from "../src/worker/types";

function sampleCandidate(overrides: Partial<LegacyCandidate> = {}): LegacyCandidate {
  return {
    id: "cand-1",
    releaseId: "c1-real-v1",
    origin: { lat: 35.0, lon: 139.0 },
    originNodeId: "n:0",
    snappedOrigin: { nodeId: "n:0", lat: 35.0, lon: 139.0, distanceMeters: 0 },
    entry: { edgeId: "e:L1", name: "宝町入口" },
    exit: { edgeId: "e:L2", name: "宝町出口" },
    entryId: "e:L1",
    exitId: "e:L2",
    roadNames: ["首都高速都心環状線"],
    edgeIds: ["e:acc", "e:L1", "e:L2", "e:ret"],
    geometry: {
      type: "LineString",
      coordinates: [
        [139.0, 35.0],
        [139.1, 35.1],
        [139.2, 35.2],
        [139.3, 35.3],
        [139.4, 35.4],
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

describe("buildPreviewPayload", () => {
  it("旧形式の legacy 候補でも走行経路・地点・料金の説明だけを渡す", () => {
    const candidate = sampleCandidate();
    const model = toCardModel(candidate, 1);
    const payload = buildPreviewPayload(candidate, model, { reducedMotion: true });

    expect(payload.version).toBe(1);
    expect(payload.candidateIndex).toBe(1);
    expect(payload.segments.map((segment) => [segment.role, segment.coords.length])).toEqual([
      ["access", 2],
      ["loop", 3],
      ["return", 2],
    ]);
    expect(payload.charged).toEqual([]);
    expect(payload.origin).toEqual({ lon: 139, lat: 35, label: "出発地" });
    expect(payload.entry).toEqual({ lon: 139.1, lat: 35.1, label: "宝町入口" });
    expect(payload.exit).toEqual({ lon: 139.3, lat: 35.3, label: "宝町出口" });
    expect(payload.summary.planMinutes).toBe(model.planMinutes);
    expect(payload.summary.toll).toBe(model.toll);
    expect(payload.summary.chargedSection).toBe(model.chargedSection);
    expect(payload.reducedMotion).toBe(true);
    expect(payload.autoplay).toBe(false);
  });

  it("料金距離が実走距離より短い legacy でも、周回を課金線にせず実際の入口・出口を渡す", () => {
    const sample = sampleCandidate();
    const candidate = sampleCandidate({
      pairKind: "legacyRing",
      entry: { edgeId: "e:acc", name: "神田橋入口" },
      exit: { edgeId: "e:ret", name: "宝町出口" },
      entryId: "e:acc",
      exitId: "e:ret",
      geometry: {
        type: "LineString",
        coordinates: [
          [139.0, 35.0],
          [139.1, 35.1], // 入口から周回の基準点へ
          [139.2, 35.2],
          [139.1, 35.1], // 周回が同じ基準点で閉じる
          [139.4, 35.4], // 出口へ進む
        ],
      },
      distanceMeters: 16_100,
      shutokoDistanceMeters: 16_100,
      toll: { ...sample.toll, billingDistanceMeters: 1_700 },
    });
    const model = toCardModel(candidate, 1);
    const payload = buildPreviewPayload(candidate, model, { reducedMotion: true });

    expect(payload.segments).toEqual([
      { role: "access", coords: candidate.geometry.coordinates.slice(0, 2) },
      { role: "loop", coords: candidate.geometry.coordinates.slice(1, 4) },
      { role: "return", coords: candidate.geometry.coordinates.slice(3) },
    ]);
    expect(payload.charged).toEqual([]);
    expect(payload.summary.chargedSection).toBe(model.chargedSection);
    expect(payload.entry).toEqual({ lon: 139.0, lat: 35.0, label: "神田橋入口" });
    expect(payload.exit).toEqual({ lon: 139.4, lat: 35.4, label: "宝町出口" });
  });

  it.each([{ entryId: "e:missing" }, { entryId: "e:L2", exitId: "e:L1" }])(
    "入口・出口の ID が不整合なら代わりの点を描かない (%j)",
    (overrides) => {
      const candidate = sampleCandidate(overrides);
      const payload = buildPreviewPayload(candidate, toCardModel(candidate, 1), {
        reducedMotion: true,
      });
      expect(payload.entry).toBeNull();
      expect(payload.exit).toBeNull();
      expect(payload.segments).toHaveLength(3);
    },
  );

  it("routeLegs を持つ radial は role ごとの線分を渡し、端から入口・出口を近似する", () => {
    const candidate = JSON.parse(radialCandidateJson) as RadialCandidate;
    const model = toCardModel(candidate, 1);
    const payload = buildPreviewPayload(candidate, model, { reducedMotion: false });

    expect(payload.segments.map((segment) => [segment.role, segment.coords.length])).toEqual([
      ["entry_approach", 4],
      ["mandatory_lap", 3],
      ["return_corridor", 2],
      ["exit_approach", 2],
    ]);
    expect(payload.charged).toEqual([]);
    expect(payload.entry).not.toBeNull();
    expect(payload.exit).not.toBeNull();
    expect(payload.entry?.label).toBe(candidate.entry.name ?? candidate.entryId);
    expect(payload.exit?.label).toBe(candidate.exit.name ?? candidate.exitId);
    expect(payload.autoplay).toBe(true);
  });

  it("座標が壊れている点は出さない", () => {
    const candidate = sampleCandidate({
      snappedOrigin: { nodeId: "n:0", lat: Number.NaN, lon: 139.0, distanceMeters: 0 },
      origin: null,
    });
    const model = toCardModel(candidate, 1);
    const payload = buildPreviewPayload(candidate, model, { reducedMotion: false });
    expect(payload.origin).toBeNull();
    expect(payload.autoplay).toBe(true);
  });
});
