// UI 純粋関数（src/ui/model.ts）のユニットテスト。
import { describe, expect, it } from "vitest";
import {
  MAX_PRODUCT_MINUTES,
  MAX_PRODUCT_SECONDS,
  SEARCH_TIMEOUT_MS,
  SUPPORTED_AREA_TEXT,
  TIMEOUT_TEXT,
  accessMinutesFromMeters,
  accessSecondsFromMeters,
  classifyNoCandidates,
  coordinateLabel,
  distanceText,
  errorMessage,
  formatLatLng,
  formatRank,
  geocodeErrorMessage,
  geolocationErrorMessage,
  minutesFromSeconds,
  nearestAccessText,
  reasonText,
  recommendedLabel,
  selectCandidate,
  statusMessage,
  timeBreakdownText,
  tollText,
  toCardModel,
  unreachableText,
  validateAddressQuery,
  validateInputFields,
  validateInputs,
  warningText,
} from "../src/ui/model";
import type { Candidate, SearchResult, SnappedOrigin } from "../src/worker/types";

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
      nearestAccess: null,
      minPlanSeconds: null,
    };
  }

  function snapped(distanceMeters: number): SnappedOrigin {
    return { nodeId: "n:1", lat: 35.67, lon: 139.4, distanceMeters };
  }

  it("TIME_WINDOW は実測の n〜m を含む文言", () => {
    const text = statusMessage(result("TIME_WINDOW"), 60, 90);
    expect(text).toContain("指定時間枠（60〜90 分）に収まる候補がありません");
  });

  it("reason コードごとの対応表", () => {
    expect(statusMessage(result("NO_BILLING_PAIR"), 15, 60)).toContain("検証済み課金ペア");
    expect(statusMessage(result("NO_LOOP"), 15, 60)).toContain("周回ルート");
    expect(statusMessage(result("NO_HANDOFF"), 15, 60)).toContain("上限超過");
    expect(statusMessage(result("SEARCH_LIMIT"), 15, 60)).toContain("上限に達し");
  });

  it("対応範囲外は supported area を明示する（requirements 39）", () => {
    const text = statusMessage(result("NO_CONNECTION"), 15, 60);
    expect(text).toContain("対応範囲外");
    expect(text).toContain(SUPPORTED_AREA_TEXT);
    expect(SUPPORTED_AREA_TEXT).toContain("都心環状線");
  });

  it("対応範囲の文言は実装に忠実（一般道は探索しない・直線距離の概算）", () => {
    // PR #30 で一般道エッジはグラフから除外済み。「一般道を含む」と読める表現は誤り。
    expect(SUPPORTED_AREA_TEXT).not.toContain("一般道です");
    expect(SUPPORTED_AREA_TEXT).toContain("直線距離の概算");
    expect(SUPPORTED_AREA_TEXT).toContain("検証済み入出口");
  });

  it("NO_CONNECTION + nearestAccess は距離とアクセス往復の分数を示す（30km 固定文言を出さない）", () => {
    const text = statusMessage({ ...result("NO_CONNECTION"), nearestAccess: snapped(55_000) }, 15, 60);
    expect(text).toContain("対応範囲外");
    expect(text).toContain("最寄り入口まで直線 約 55.0 km");
    expect(text).toContain("片道 約 143 分（概算）");
    expect(text).toContain("最大 4 時間では周回できません");
    expect(text).not.toContain("30km");
    expect(text).toContain(SUPPORTED_AREA_TEXT);
  });

  it("TIME_WINDOW で最短計画が 240 分を超えるなら数値根拠つきで到達不能を示す", () => {
    const text = statusMessage(
      { ...result("TIME_WINDOW"), nearestAccess: snapped(36_134), minPlanSeconds: 14_867 },
      15,
      240,
    );
    expect(text).toContain("最短の計画時間でも 約 248 分");
    expect(text).toContain("最寄り入口まで直線 約 36.1 km");
    expect(text).toContain("最大 4 時間では周回できません");
    // 時間枠を広げても届かないので「広げる」導線を促さない。
    expect(text).not.toContain("時間枠を広げる");
  });

  it("TIME_WINDOW で 240 分以内に収まるなら時間枠を広げる文言のまま", () => {
    const text = statusMessage(
      { ...result("TIME_WINDOW"), nearestAccess: snapped(29_575), minPlanSeconds: 12_450 },
      15,
      60,
    );
    expect(text).toContain("時間枠を広げる");
    expect(text).not.toContain("最大 4 時間");
  });

  it("minPlanSeconds が無いときは到達不能を主張しない", () => {
    expect(statusMessage(result("TIME_WINDOW"), 15, 60)).not.toContain("最大 4 時間");
    expect(statusMessage({ ...result("NO_LOOP"), nearestAccess: snapped(31_035) }, 15, 60)).not.toContain(
      "最大 4 時間",
    );
  });

  it("unreachableText / nearestAccessText は km と分だけを出す", () => {
    expect(nearestAccessText(snapped(31_035))).toBe(
      "最寄り入口まで直線 約 31.0 km・片道 約 81 分（概算）。",
    );
    const text = unreachableText(snapped(36_134), 14_867);
    expect(text).toContain("周回できる最短の計画時間でも 約 248 分");
    expect(text).toContain("最寄り入口までのアクセス往復だけで 約 188 分");
    expect(text).toContain("最大 4 時間では周回できません");
  });

  it("classifyNoCandidates は復帰導線を分ける", () => {
    expect(classifyNoCandidates(result("NO_CONNECTION"))).toBe("unsupported_area");
    expect(classifyNoCandidates(result("NO_LOOP"))).toBe("unsupported_area");
    expect(classifyNoCandidates(result("NO_BILLING_PAIR"))).toBe("unsupported_area");
    expect(
      classifyNoCandidates({ ...result("TIME_WINDOW"), minPlanSeconds: MAX_PRODUCT_SECONDS + 1 }),
    ).toBe("unreachable");
    expect(
      classifyNoCandidates({ ...result("TIME_WINDOW"), minPlanSeconds: MAX_PRODUCT_SECONDS }),
    ).toBe("time_window");
    expect(classifyNoCandidates({ ...result("TIME_WINDOW"), minPlanSeconds: 12_450 })).toBe(
      "time_window",
    );
    expect(classifyNoCandidates(result("TIME_WINDOW"))).toBe("time_window");
    expect(classifyNoCandidates({ ...result(null), status: "truncated" })).toBe("other");
  });

  it("アクセス概算はエンジンと同じ式（直線×1.3÷30km/h 切り上げ）", () => {
    expect(accessSecondsFromMeters(46_000)).toBe(7176);
    expect(accessMinutesFromMeters(31_035)).toBe(81);
    expect(accessMinutesFromMeters(0)).toBe(0);
    // cap 46km は片道往復で 240 分を使い切る距離として導出されている。
    expect(accessMinutesFromMeters(46_000) * 2).toBe(MAX_PRODUCT_MINUTES);
    expect(MAX_PRODUCT_MINUTES).toBe(240);
    expect(MAX_PRODUCT_SECONDS).toBe(14_400);
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

  it("4 桁以上の料金は 3 桁区切りで表示する（design-review-002 C3）", () => {
    const model = toCardModel(
      sampleCandidate({
        toll: {
          billingPairId: "bp:x",
          chargedSectionCount: 2,
          amountYen: 1320,
          pricingAt: "2026-09-10T00:00:00Z",
          effectiveFrom: null,
          effectiveTo: null,
        },
      }),
    );
    expect(model.toll).toBe("料金額: 1,320 円");
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

describe("toCardModel の新フィールド", () => {
  it("index・内訳・距離・短縮料金・効率・reasons・geometry・エッジ列を埋める", () => {
    const model = toCardModel(sampleCandidate(), 2);
    expect(model.index).toBe(2);
    expect(model.accessMinutes).toBe(1); // 60s
    expect(model.shutokoMinutes).toBe(25); // 1503s → 25 分
    expect(model.returnMinutes).toBe(2); // 120s
    expect(model.bufferMinutes).toBe(2); // 121s → 2 分
    expect(model.distanceKm).toBe(8); // 8000m → 8 km（小数 1 桁）
    expect(model.tollShort).toBe("300 円");
    expect(model.timePerYen).toBe("1 円あたり 約 0.08 分");
    expect(model.reasons).toEqual(["時間あたりの料金効率が最良", "1区間料金（最低料金）"]);
    expect(model.rankLabel).toBeNull();
    expect(model.geometry).toEqual({ type: "LineString", coordinates: [] });
    expect(model.entryId).toBe("e:1");
    expect(model.exitId).toBe("e:2");
    expect(model.loopEdgeIds).toEqual(["e:1"]);
    expect(model.edgeIds).toEqual(["e:1"]);
  });

  it("index 省略時は 1、距離は小数 1 桁へ丸める", () => {
    const model = toCardModel(sampleCandidate({ distanceMeters: 12345 }));
    expect(model.index).toBe(1);
    expect(model.distanceKm).toBe(12.3);
    expect(distanceText(12345)).toBe("12.3 km");
  });

  it("料金 null は timePerYen・tollShort ともに未算出表現", () => {
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
    expect(model.tollShort).toBe("未算出");
    expect(model.timePerYen).toBeNull();
  });
});

describe("formatRank / recommendedLabel", () => {
  it("全候補の料金が確定しているときだけ順位を出す", () => {
    expect(formatRank(false, 1)).toBeNull();
    expect(formatRank(false, 2)).toBeNull();
    expect(formatRank(true, 1)).toBe("最安順位 1 位");
    expect(formatRank(true, 3)).toBe("最安順位 3 位");
  });

  it("BEST_* の理由で推薦ラベルを付ける", () => {
    expect(recommendedLabel({ reasons: ["BEST_TIME_PER_YEN", "ONE_SECTION_TOLL"] })).toBe("推薦");
    expect(recommendedLabel({ reasons: ["BEST_SHUTOKO_TIME"] })).toBe("推薦");
    expect(recommendedLabel({ reasons: ["ONE_SECTION_TOLL"] })).toBeNull();
    expect(recommendedLabel({ reasons: [] })).toBeNull();
  });
});

describe("reasonText / timeBreakdownText", () => {
  it("推薦理由コードの対応表と未知コードの透過", () => {
    expect(reasonText("BEST_TIME_PER_YEN")).toBe("時間あたりの料金効率が最良");
    expect(reasonText("BEST_SHUTOKO_TIME")).toBe("首都高滞在時間が最長");
    expect(reasonText("ONE_SECTION_TOLL")).toBe("1区間料金（最低料金）");
    expect(reasonText("UNKNOWN")).toBe("UNKNOWN");
  });

  it("時間内訳を 1 行にまとめる（入り・帰りは概算を明示）", () => {
    expect(timeBreakdownText(toCardModel(sampleCandidate()))).toBe(
      "内訳: 入り 1分（概算） / 首都高 25分 / 帰り 2分（概算） / 余裕 2分",
    );
  });
});

describe("validateAddressQuery", () => {
  it("空・空白のみは必須エラー", () => {
    expect(validateAddressQuery("")).not.toBeNull();
    expect(validateAddressQuery("   ")).not.toBeNull();
  });

  it("200 文字超はエラー、1〜200 文字は正常", () => {
    expect(validateAddressQuery("あ".repeat(200))).toBeNull();
    expect(validateAddressQuery("あ".repeat(201))).not.toBeNull();
    expect(validateAddressQuery("東京都千代田区")).toBeNull();
  });
});

describe("geocodeErrorMessage", () => {
  it("Worker のエラーコードごとの文言", () => {
    expect(geocodeErrorMessage("INVALID_QUERY")).toContain("1〜200 文字");
    expect(geocodeErrorMessage("PAYLOAD_TOO_LARGE")).toContain("長すぎ");
    expect(geocodeErrorMessage("RATE_LIMITED")).toContain("上限");
    expect(geocodeErrorMessage("GEOCODER_TIMEOUT")).toContain("タイムアウト");
    expect(geocodeErrorMessage("GEOCODER_UNAVAILABLE")).toContain("一時的");
    expect(geocodeErrorMessage("RATE_LIMITER_UNAVAILABLE")).toContain("受け付けられません");
    expect(geocodeErrorMessage("FETCH_FAILED")).toContain("通信");
  });

  it("未知コードは既定文言", () => {
    const text = geocodeErrorMessage("SOMETHING_ELSE");
    expect(text).toContain("失敗");
    expect(text).not.toContain("SOMETHING_ELSE");
  });
});

describe("coordinateLabel / formatLatLng", () => {
  it("小数 5 桁で整形する", () => {
    expect(coordinateLabel(35.6896727, 139.7644248)).toBe("35.68967, 139.76442");
    expect(formatLatLng(35.6896727, 139.7644248)).toBe("35.68967,139.76442");
  });
});

describe("geolocationErrorMessage", () => {
  it("1/2/3 のコードを対応付け、未知コードは既定文言", () => {
    expect(geolocationErrorMessage(1)).toContain("許可されていません");
    expect(geolocationErrorMessage(2)).toContain("取得できません");
    expect(geolocationErrorMessage(3)).toContain("タイムアウト");
    expect(geolocationErrorMessage(99)).toContain("取得できません");
  });
});

describe("selectCandidate", () => {
  const candidates = [sampleCandidate({ id: "a" }), sampleCandidate({ id: "b" })];
  it("ID 一致で引き当て、無ければ null", () => {
    expect(selectCandidate(candidates, "b")?.id).toBe("b");
    expect(selectCandidate(candidates, "zzz")).toBeNull();
  });
});

describe("timePerYen の料金ゲーティング", () => {
  function withAmount(amountYen: number | null): Candidate {
    return sampleCandidate({
      toll: {
        billingPairId: "bp:x",
        chargedSectionCount: 1,
        amountYen,
        pricingAt: "2026-09-10T00:00:00Z",
        effectiveFrom: null,
        effectiveTo: null,
      },
    });
  }

  it("amountYen 0 と負値は timePerYen を出さない", () => {
    expect(toCardModel(withAmount(0)).timePerYen).toBeNull();
    expect(toCardModel(withAmount(-100)).timePerYen).toBeNull();
  });

  it("正の amountYen は「円あたり」を含む効率文字列を出す", () => {
    const text = toCardModel(withAmount(300)).timePerYen;
    expect(text).not.toBeNull();
    expect(text).toContain("円あたり");
  });
});

describe("validateInputFields の境界", () => {
  it("正常な整数入力（下限 1・上限 240）は全項目 null", () => {
    expect(validateInputFields("35.7", "139.8", "1", "240")).toEqual({
      lat: null,
      lon: null,
      minMinutes: null,
      maxMinutes: null,
      range: null,
    });
    expect(validateInputFields("35.7", "139.8", "240", "240")).toEqual({
      lat: null,
      lon: null,
      minMinutes: null,
      maxMinutes: null,
      range: null,
    });
  });

  it("15.0 は整数値のため最小時間エラーにならない", () => {
    // Number("15.0") === 15 で Number.isInteger が true。表記上の小数は弾かない。
    const fields = validateInputFields("35.7", "139.8", "15.0", "60");
    expect(fields.minMinutes).toBeNull();
    expect(fields.range).toBeNull();
  });

  it("15.5 は最小時間の整数エラーになる", () => {
    const fields = validateInputFields("35.7", "139.8", "15.5", "60");
    expect(fields.minMinutes).not.toBeNull();
    expect(fields.range).toBeNull(); // 欄単位エラーがある間は範囲条件を重ねない
  });

  it("範囲外の 0 / 241 は範囲条件エラーになる", () => {
    const zero = validateInputFields("35.7", "139.8", "0", "60");
    expect(zero.minMinutes).toBeNull();
    expect(zero.range).not.toBeNull();
    const over = validateInputFields("35.7", "139.8", "1", "241");
    expect(over.maxMinutes).toBeNull();
    expect(over.range).not.toBeNull();
  });
});

describe("toCardModel の rampName フォールバック", () => {
  it("入口・出口名が null のときは edge ID を経路表記に使う", () => {
    const model = toCardModel(
      sampleCandidate({
        entry: { edgeId: "e:entry-x", name: null },
        exit: { edgeId: "e:exit-y", name: null },
        entryId: "e:entry-x",
        exitId: "e:exit-y",
      }),
    );
    expect(model.route).toBe("e:entry-x → e:exit-y");
    expect(model.route).toContain("e:entry-x");
    expect(model.route).toContain("e:exit-y");
  });
});
