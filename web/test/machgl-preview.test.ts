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
  it("access/loop/return と課金区間、出発地・入口・出口を渡す", () => {
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
    expect(payload.charged).toEqual([
      [139.1, 35.1],
      [139.2, 35.2],
      [139.3, 35.3],
    ]);
    expect(payload.origin).toEqual({ lon: 139, lat: 35, label: "出発地" });
    expect(payload.entry).toEqual({ lon: 139.1, lat: 35.1, label: "宝町入口" });
    expect(payload.exit).toEqual({ lon: 139.3, lat: 35.3, label: "宝町出口" });
    expect(payload.summary.planMinutes).toBe(model.planMinutes);
    expect(payload.summary.toll).toBe(model.toll);
    expect(payload.summary.chargedSection).toBe(model.chargedSection);
    expect(payload.reducedMotion).toBe(true);
    expect(payload.autoplay).toBe(false);
  });

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
