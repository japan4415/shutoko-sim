// parseSearchResult の実行時契約検証（旧エンジンが新診断フィールドを欠落させた場合の防御）。
// 欠落した undefined を null と誤判定すると UI が「約 NaN 分」や実行時例外に至るため、
// 明確な契約不一致エラー（RESULT_CONTRACT_MISMATCH）として停止することを固定する。
import { describe, expect, it } from "vitest";
import radialCandidate from "../../fixtures/candidate-v2/radial-valid.json?raw";
import duplicateLegs from "../../fixtures/candidate-v2/invalid-edge-route-legs-duplicate.json?raw";
import missingLegs from "../../fixtures/candidate-v2/invalid-edge-route-legs-missing.json?raw";
import {
  parseSearchResult,
  PipelineError,
  RESULT_CONTRACT_MISMATCH,
} from "../src/worker/pipeline";
import { errorMessage } from "../src/ui/model";

/** 契約を満たす最小の探索結果（候補なし・診断 null）。 */
function validResult(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    requestId: "r1",
    releaseId: "c1-real-v1",
    status: "no_candidates",
    reason: "TIME_WINDOW",
    rankingMode: "time_per_yen",
    expandedStates: 10,
    candidates: [],
    nearestAccess: null,
    minPlanSeconds: null,
    ...overrides,
  };
}

describe("parseSearchResult の実行時検証", () => {
  it("契約を満たす結果はそのまま通す", () => {
    const result = parseSearchResult(JSON.stringify(validResult()));
    expect(result.status).toBe("no_candidates");
    expect(result.minPlanSeconds).toBeNull();
    expect(result.nearestAccess).toBeNull();
  });

  it("診断値が実数でも通す", () => {
    const result = parseSearchResult(
      JSON.stringify(
        validResult({
          status: "truncated",
          reason: "SEARCH_LIMIT",
          nearestAccess: { nodeId: "n:1", lat: 35.68, lon: 139.76, distanceMeters: 1234 },
          minPlanSeconds: 2252,
        }),
      ),
    );
    expect(result.minPlanSeconds).toBe(2252);
    expect(result.nearestAccess?.distanceMeters).toBe(1234);
  });

  it("旧エンジンの最小 JSON（診断フィールド無し）は RESULT_CONTRACT_MISMATCH で停止する", () => {
    // exactOptionalPropertyTypes 下でも undefined は null と等価でないため、
    // 旧 WASM の応答がここに到達すると「約 NaN 分」へ進んでいた。
    const legacy = JSON.stringify({ status: "ok", candidates: [] });
    try {
      parseSearchResult(legacy);
      throw new Error("should have thrown");
    } catch (err) {
      expect(err).toBeInstanceOf(PipelineError);
      expect((err as PipelineError).code).toBe(RESULT_CONTRACT_MISMATCH);
    }
  });

  it("nearestAccess だけ欠落しても停止する", () => {
    const json = JSON.stringify(
      Object.fromEntries(Object.entries(validResult()).filter(([key]) => key !== "nearestAccess")),
    );
    expect(() => parseSearchResult(json)).toThrowError(/nearestAccess/);
  });

  it("minPlanSeconds だけ欠落しても停止する", () => {
    const json = JSON.stringify(
      Object.fromEntries(Object.entries(validResult()).filter(([key]) => key !== "minPlanSeconds")),
    );
    expect(() => parseSearchResult(json)).toThrowError(/minPlanSeconds/);
  });

  it("型・有限性・非負性の違反を拒否する", () => {
    // JSON は NaN / Infinity を表現できないため、非数値（文字列・null）と負値で検証する。
    expect(() => parseSearchResult(JSON.stringify(validResult({ minPlanSeconds: "2252" })))).toThrowError(
      /minPlanSeconds/,
    );
    expect(() => parseSearchResult(JSON.stringify(validResult({ minPlanSeconds: -1 })))).toThrowError(
      /minPlanSeconds/,
    );
    expect(() =>
      parseSearchResult(
        JSON.stringify(
          validResult({
            nearestAccess: { nodeId: "n:1", lat: 35.68, lon: 139.76, distanceMeters: -5 },
          }),
        ),
      ),
    ).toThrowError(/distanceMeters/);
    expect(() =>
      parseSearchResult(
        JSON.stringify(
          validResult({
            nearestAccess: { nodeId: "n:1", lat: null, lon: 139.76, distanceMeters: 5 },
          }),
        ),
      ),
    ).toThrowError(/lat/);
    expect(() =>
      parseSearchResult(
        JSON.stringify(validResult({ nearestAccess: { nodeId: 7, lat: 35.68, lon: 139.76, distanceMeters: 5 } })),
      ),
    ).toThrowError(/nodeId/);
  });

  it("candidates が配列でない・JSON でない応答も契約不一致として停止する", () => {
    expect(() => parseSearchResult(JSON.stringify(validResult({ candidates: null })))).toThrowError(
      /candidates/,
    );
    expect(() => parseSearchResult("not json")).toThrowError(PipelineError);
  });

  it("契約不一致のコードは UI のエラー導線へ渡せる文言を持つ", () => {
    const text = errorMessage(RESULT_CONTRACT_MISMATCH);
    expect(text).toContain("RESULT_CONTRACT_MISMATCH");
    expect(text).toContain("再読み込み");
  });

  it("radialReturn Candidate v2 を受け入れる", () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    const result = parseSearchResult(
      JSON.stringify(validResult({ status: "ok", reason: null, candidates: [candidate] })),
    );
    expect(result.candidates[0]?.pairKind).toBe("radialReturn");
  });

  it("edgeRouteLegs の重複・欠落と legacy 残存フィールドを拒否する", () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    for (const fragment of [duplicateLegs, missingLegs]) {
      const invalid = { ...candidate, edgeRouteLegs: JSON.parse(fragment).edgeRouteLegs };
      expect(() =>
        parseSearchResult(
          JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
        ),
      ).toThrowError(/edgeRouteLegs/);
    }
    const charged = {
      ...candidate,
      toll: { ...(candidate.toll as Record<string, unknown>), chargedSectionCount: 1 },
    };
    expect(() =>
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [charged] })),
      ),
    ).toThrowError(/chargedSectionCount/);
  });

  it("未知の pairKind を拒否する", () => {
    const candidate = { ...(JSON.parse(radialCandidate) as Record<string, unknown>), pairKind: "futurePair" };
    expect(() =>
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [candidate] })),
      ),
    ).toThrowError(/pairKind/);
  });
});
