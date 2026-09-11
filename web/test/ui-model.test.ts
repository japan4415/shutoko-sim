// UI 純粋関数（src/ui/model.ts）のユニットテスト。
import { describe, expect, it } from "vitest";
import {
  SEARCH_TIMEOUT_MS,
  TIMEOUT_TEXT,
  errorMessage,
  minutesFromSeconds,
  statusMessage,
  tollText,
  toCardModel,
  validateInputs,
  warningText,
} from "../src/ui/model";
import type { Candidate, SearchResult } from "../src/worker/types";

function sampleCandidate(overrides: Partial<Candidate> = {}): Candidate {
  return {
    id: "cand-1",
    releaseId: "c1-real-v1",
    origin: { lat: 35.6896727, lon: 139.7644248 },
    originNodeId: "n:1070862943",
    snappedOrigin: {
      nodeId: "n:1070862943",
      lat: 35.6896727,
      lon: 139.7644248,
      distanceMeters: 0,
    },
    entry: { edgeId: "e:1", name: "神田橋入口" },
    exit: { edgeId: "e:2", name: "宝町出口" },
    entryId: "e:1",
    exitId: "e:2",
    roadNames: ["首都高速都心環状線"],
    edgeIds: ["e:1"],
    geometry: { type: "LineString", coordinates: [] },
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
      billingPairId: "bp:c1-outer:kandabashi-takaracho",
      chargedSectionCount: 1,
      amountYen: 300,
      pricingAt: "2026-09-10T00:00:00Z",
      effectiveFrom: null,
      effectiveTo: null,
    },
    loop: {
      anchorNodeId: "n:1",
      edgeIds: ["e:1"],
      durationSeconds: 900,
      distanceMeters: 4600,
      validated: true,
    },
    reasons: ["BEST_TIME_PER_YEN", "ONE_SECTION_TOLL"],
    warnings: ["HANDOFF_WAYPOINTS_UNVERIFIED", "STATIC_TRAVEL_TIME"],
    handoff: {
      origin: { lat: 35.6896727, lon: 139.7644248 },
      destination: { lat: 35.6896727, lon: 139.7644248 },
      waypoints: [],
      mapsUrl: "https://www.google.com/maps/dir/?api=1&origin=35.689673,139.764425",
      verificationSetVersion: null,
    },
    ...overrides,
  };
}

describe("validateInputs", () => {
  it("正常範囲（神田橋 15〜60）はエラー無し", () => {
    expect(validateInputs("35.6896727", "139.7644248", "15", "60")).toEqual([]);
  });

  it("空欄・非有限・範囲外を指摘する", () => {
    const errors = validateInputs("", "abc", "15", "60");
    expect(errors.some((e) => e.includes("緯度"))).toBe(true);
    expect(errors.some((e) => e.includes("経度"))).toBe(true);
    expect(validateInputs("91", "139.7", "15", "60").some((e) => e.includes("緯度"))).toBe(true);
  });

  it("時間の逆転・小数・240 超過を弾く（1 ≤ min ≤ max ≤ 240）", () => {
    expect(validateInputs("35.7", "139.8", "60", "15").length).toBe(1);
    expect(validateInputs("35.7", "139.8", "15.5", "60").length).toBe(1);
    expect(validateInputs("35.7", "139.8", "1", "241").length).toBe(1);
    expect(validateInputs("35.7", "139.8", "0", "60").length).toBe(1);
  });
});

describe("statusMessage / errorMessage", () => {
  function result(reason: string | null): SearchResult {
    return {
      requestId: "r1",
      releaseId: "c1-real-v1",
      status: reason === null ? "ok" : "no_candidates",
      reason,
      rankingMode: "time_per_yen",
      expandedStates: 10,
      candidates: [],
    };
  }

  it("TIME_WINDOW は実測の n〜m を含む文言", () => {
    const text = statusMessage(result("TIME_WINDOW"), 60, 90);
    expect(text).toContain("指定時間枠（60〜90 分）に収まる候補がありません");
  });

  it("reason コードごとの対応表", () => {
    expect(statusMessage(result("NO_CONNECTION"), 15, 60)).toContain("200m 以内");
    expect(statusMessage(result("NO_BILLING_PAIR"), 15, 60)).toContain("検証済み課金ペア");
    expect(statusMessage(result("NO_LOOP"), 15, 60)).toContain("周回ルート");
    expect(statusMessage(result("NO_HANDOFF"), 15, 60)).toContain("上限超過");
    expect(statusMessage(result("SEARCH_LIMIT"), 15, 60)).toContain("上限に達し");
  });

  it("ARTIFACT_MISMATCH / FETCH_FAILED は再読み込み案内、TIMEOUT は 10 秒文言", () => {
    expect(errorMessage("ARTIFACT_MISMATCH")).toContain("再読み込み");
    expect(errorMessage("ARTIFACT_MISMATCH")).toContain("ARTIFACT_MISMATCH");
    expect(errorMessage("FETCH_FAILED")).toContain("再読み込み");
    expect(errorMessage("TIMEOUT")).toBe(TIMEOUT_TEXT);
    expect(SEARCH_TIMEOUT_MS).toBe(10_000);
  });
});

describe("toCardModel", () => {
  it("planSeconds を分に換算し、料金・経路・課金対象・警告を作る", () => {
    const model = toCardModel(sampleCandidate());
    expect(model.planMinutes).toBe(30); // 1804s → 30 分
    expect(model.toll).toBe("料金額: 300 円");
    expect(model.route).toBe("神田橋入口 → 宝町出口");
    expect(model.chargedSection).toBe("課金対象: 神田橋入口 → 宝町出口 の1区間");
    expect(model.warnings).toEqual([
      "経由地点の選定ルールは暫定（実機検証未了）",
      "静的速度に基づく推定（渋滞・規制は未反映）",
    ]);
    expect(model.mapsUrl.startsWith("https://www.google.com/maps/dir/?api=1")).toBe(true);
  });

  it("料金 null は「料金額: 未算出」", () => {
    const model = toCardModel(
      sampleCandidate({
        toll: {
          billingPairId: "bp:x",
          chargedSectionCount: 1,
          amountYen: null,
          pricingAt: "2026-09-10T00:00:00Z",
          effectiveFrom: null,
          effectiveTo: null,
        },
      }),
    );
    expect(model.toll).toBe("料金額: 未算出");
  });

  it("warningText と minutesFromSeconds の境界", () => {
    expect(warningText("UNKNOWN_CODE")).toBe("UNKNOWN_CODE");
    expect(minutesFromSeconds(0)).toBe(0);
    expect(minutesFromSeconds(90)).toBe(2);
  });
});
