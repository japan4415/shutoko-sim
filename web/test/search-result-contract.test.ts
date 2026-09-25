// parseSearchResult の実行時契約検証（旧エンジンが新診断フィールドを欠落させた場合の防御）。
// 欠落した undefined を null と誤判定すると UI が「約 NaN 分」や実行時例外に至るため、
// 明確な契約不一致エラー（RESULT_CONTRACT_MISMATCH）として停止することを固定する。
import { describe, expect, it } from "vitest";
import radialCandidate from "../../fixtures/candidate-v2/radial-valid.json?raw";
import duplicateLegs from "../../fixtures/candidate-v2/invalid-edge-route-legs-duplicate.json?raw";
import missingLegs from "../../fixtures/candidate-v2/invalid-edge-route-legs-missing.json?raw";
import hashMismatch from "../../fixtures/candidate-v2/invalid-resolved-segment-hash-mismatch.json?raw";
import {
  hexDigest,
  parseSearchResult,
  PipelineError,
  RESULT_CONTRACT_MISMATCH,
} from "../src/worker/pipeline";
import { errorMessage } from "../src/ui/model";
import type { SearchResult } from "../src/worker/types";

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

function legacyCandidateWithUrl(mapsUrl: string): Record<string, unknown> {
  const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
  candidate.pairKind = "legacyRing";
  candidate.loop = {
    anchorNodeId: "fixture:node:merge",
    edgeIds: ["fixture:edge:lap:1", "fixture:edge:lap:2"],
    durationSeconds: 1200,
    distanceMeters: 20000,
    validated: true,
  };
  candidate.toll = {
    ...(candidate.toll as Record<string, unknown>),
    chargedSectionCount: 1,
  };
  candidate.handoff = {
    origin: { lat: 35.1, lon: 139.1 },
    destination: { lat: 35.1, lon: 139.1 },
    waypoints: [],
    mapsUrl,
    verificationSetVersion: null,
  };
  return candidate;
}

describe("parseSearchResult の実行時検証", () => {
  it("契約を満たす結果はそのまま通す", async () => {
    const result = await parseSearchResult(JSON.stringify(validResult()));
    expect(result.status).toBe("no_candidates");
    expect(result.minPlanSeconds).toBeNull();
    expect(result.nearestAccess).toBeNull();
  });

  it("診断値が実数でも通す", async () => {
    const result = await parseSearchResult(
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

  it("旧エンジンの最小 JSON（診断フィールド無し）は RESULT_CONTRACT_MISMATCH で停止する", async () => {
    // exactOptionalPropertyTypes 下でも undefined は null と等価でないため、
    // 旧 WASM の応答がここに到達すると「約 NaN 分」へ進んでいた。
    const legacy = JSON.stringify({ status: "ok", candidates: [] });
    try {
      await parseSearchResult(legacy);
      throw new Error("should have thrown");
    } catch (err) {
      expect(err).toBeInstanceOf(PipelineError);
      expect((err as PipelineError).code).toBe(RESULT_CONTRACT_MISMATCH);
    }
  });

  it("nearestAccess だけ欠落しても停止する", async () => {
    const json = JSON.stringify(
      Object.fromEntries(Object.entries(validResult()).filter(([key]) => key !== "nearestAccess")),
    );
    await expect(parseSearchResult(json)).rejects.toThrowError(/nearestAccess/);
  });

  it("minPlanSeconds だけ欠落しても停止する", async () => {
    const json = JSON.stringify(
      Object.fromEntries(Object.entries(validResult()).filter(([key]) => key !== "minPlanSeconds")),
    );
    await expect(parseSearchResult(json)).rejects.toThrowError(/minPlanSeconds/);
  });

  it("型・有限性・非負性の違反を拒否する", async () => {
    // JSON は NaN / Infinity を表現できないため、非数値（文字列・null）と負値で検証する。
    await expect(
      parseSearchResult(JSON.stringify(validResult({ minPlanSeconds: "2252" }))),
    ).rejects.toThrowError(/minPlanSeconds/);
    await expect(
      parseSearchResult(JSON.stringify(validResult({ minPlanSeconds: -1 }))),
    ).rejects.toThrowError(/minPlanSeconds/);
    await expect(
      parseSearchResult(
        JSON.stringify(
          validResult({
            nearestAccess: { nodeId: "n:1", lat: 35.68, lon: 139.76, distanceMeters: -5 },
          }),
        ),
      ),
    ).rejects.toThrowError(/distanceMeters/);
    await expect(
      parseSearchResult(
        JSON.stringify(
          validResult({
            nearestAccess: { nodeId: "n:1", lat: null, lon: 139.76, distanceMeters: 5 },
          }),
        ),
      ),
    ).rejects.toThrowError(/lat/);
    await expect(
      parseSearchResult(
        JSON.stringify(validResult({ nearestAccess: { nodeId: 7, lat: 35.68, lon: 139.76, distanceMeters: 5 } })),
      ),
    ).rejects.toThrowError(/nodeId/);
  });

  it("candidates が配列でない・JSON でない応答も契約不一致として停止する", async () => {
    await expect(
      parseSearchResult(JSON.stringify(validResult({ candidates: null }))),
    ).rejects.toThrowError(/candidates/);
    await expect(parseSearchResult("not json")).rejects.toThrowError(PipelineError);
  });

  it("契約不一致のコードは UI のエラー導線へ渡せる文言を持つ", () => {
    const text = errorMessage(RESULT_CONTRACT_MISMATCH);
    expect(text).toContain("RESULT_CONTRACT_MISMATCH");
    expect(text).toContain("再読み込み");
  });

  it("radialReturn Candidate v2 を受け入れる", async () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    const result = await parseSearchResult(
      JSON.stringify(validResult({ status: "ok", reason: null, candidates: [candidate] })),
    );
    expect(result.candidates[0]?.pairKind).toBe("radialReturn");
  });

  it("radialReturn は device verification 済み handoff を受け入れ、不正形を拒否する", async () => {
    const mapsUrls = [
      "https://www.google.com/maps/dir/?api=1&origin=35.000000,139.000000&destination=35.100000,139.100000&travelmode=driving",
      "https://www.google.com/maps/dir/?api=1&origin=35.100000,139.100000&destination=35.400000,139.400000&travelmode=driving",
      "https://www.google.com/maps/dir/?api=1&origin=35.400000,139.400000&destination=35.000000,139.000000&travelmode=driving",
    ];
    const urlSha256 = await Promise.all(
      mapsUrls.map((mapsUrl) => hexDigest(new TextEncoder().encode(mapsUrl).buffer as ArrayBuffer)),
    );
    const enabledHandoff = {
      enabled: true,
      legUrls: ["surface_access", "loop_transfer", "surface_return"].map((role, index) => ({
        role,
        mapsUrl: mapsUrls[index],
        urlSha256: urlSha256[index],
      })),
      disabledReason: null,
    };
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    candidate.handoff = enabledHandoff;
    await expect(
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [candidate] })),
      ),
    ).resolves.toMatchObject({ candidates: [{ handoff: { enabled: true } }] });

    for (const mapsUrl of [
      "javascript:alert(1)",
      "https://evil.example/maps/dir/?api=1&origin=35.000000,139.000000&destination=35.100000,139.100000&travelmode=driving",
      "https://www.google.com/maps/dir/?api=1&origin=35.000000,139.000000",
    ]) {
      const invalid = JSON.parse(radialCandidate) as Record<string, unknown>;
      const invalidHandoff = structuredClone(enabledHandoff) as Record<string, unknown>;
      const invalidLeg = (invalidHandoff.legUrls as Record<string, unknown>[])[0];
      if (invalidLeg !== undefined) {
        invalidLeg.mapsUrl = mapsUrl;
      }
      invalid.handoff = invalidHandoff;
      await expect(
        parseSearchResult(
          JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
        ),
      ).rejects.toThrowError(/URL/);
    }

    for (const mutate of [
      (handoff: Record<string, unknown>) => {
        handoff.disabledReason = "device_verification_pending";
      },
      (handoff: Record<string, unknown>) => {
        handoff.legUrls = (handoff.legUrls as unknown[]).slice(0, 1);
      },
      (handoff: Record<string, unknown>) => {
        const leg = (handoff.legUrls as Record<string, unknown>[])[0];
        if (leg !== undefined) {
          leg.urlSha256 = "0".repeat(64);
        }
      },
    ]) {
      const invalid = JSON.parse(radialCandidate) as Record<string, unknown>;
      const invalidHandoff = structuredClone(enabledHandoff) as Record<string, unknown>;
      mutate(invalidHandoff);
      invalid.handoff = invalidHandoff;
      await expect(
        parseSearchResult(
          JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
        ),
      ).rejects.toThrowError(/device verification|URL|SHA-256/);
    }

    const invalidDisabled = JSON.parse(radialCandidate) as Record<string, unknown>;
    (invalidDisabled.handoff as Record<string, unknown>).disabledReason = "other";
    await expect(
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalidDisabled] })),
      ),
    ).rejects.toThrowError(/device verification/);
  });

  it("legacyRing にも Google Maps の scheme・host・path・パラメータ制限を適用する", async () => {
    const validUrl =
      "https://www.google.com/maps/dir/?api=1&origin=35.100000,139.100000&destination=35.100000,139.100000&travelmode=driving";
    await expect(
      parseSearchResult(
        JSON.stringify(
          validResult({
            status: "ok",
            reason: null,
            candidates: [legacyCandidateWithUrl(validUrl)],
          }),
        ),
      ),
    ).resolves.toMatchObject({ candidates: [{ pairKind: "legacyRing" }] });

    for (const mapsUrl of [
      "javascript:alert(1)",
      "https://evil.example/maps/dir/?api=1&origin=35.100000,139.100000&destination=35.100000,139.100000&travelmode=driving",
      "https://www.google.com/maps/dir/?api=1&origin=35.100000,139.100000&destination=35.100000,139.100000",
    ]) {
      await expect(
        parseSearchResult(
          JSON.stringify(
            validResult({
              status: "ok",
              reason: null,
              candidates: [legacyCandidateWithUrl(mapsUrl)],
            }),
          ),
        ),
      ).rejects.toThrowError(/URL/);
    }
  });

  it("surface leg は許可fieldと距離・時間の0同値条件を厳密に検証する", async () => {
    const zeroLegs = JSON.parse(radialCandidate) as Record<string, unknown>;
    for (const leg of zeroLegs.estimatedLegs as Record<string, unknown>[]) {
      leg.distanceMeters = 0;
      leg.durationSeconds = 0;
    }
    zeroLegs.duration = {
      accessSeconds: 0,
      shutokoSeconds: 1440,
      returnSeconds: 0,
      baseSeconds: 1440,
      bufferSeconds: 300,
      planSeconds: 1740,
    };
    zeroLegs.distanceMeters = zeroLegs.shutokoDistanceMeters;
    await expect(
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [zeroLegs] })),
      ),
    ).resolves.toMatchObject({ candidates: [{ pairKind: "radialReturn" }] });

    for (const mutate of [
      (candidate: Record<string, unknown>) => {
        (candidate.estimatedLegs as Record<string, unknown>[])[0].startEdgeIndex = 0;
      },
      (candidate: Record<string, unknown>) => {
        (candidate.estimatedLegs as Record<string, unknown>[])[0].durationSeconds = 0;
      },
    ]) {
      const invalid = JSON.parse(radialCandidate) as Record<string, unknown>;
      mutate(invalid);
      await expect(
        parseSearchResult(
          JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
        ),
      ).rejects.toThrowError(/estimatedLegs/);
    }
  });

  it("topologyOnly Candidate v2 を受け取り商品cohort外れを固定する", async () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    candidate.pairKind = "topologyOnly";
    candidate.eligibilityStatus = "topology_only";
    candidate.loopValidationStatus = "topology_only";
    candidate.reasons = ["TOPOLOGY_ONLY"];
    candidate.loop = {
      anchorNodeId: "fixture:node:merge",
      edgeIds: ["fixture:edge:lap:1", "fixture:edge:lap:2"],
      durationSeconds: 1200,
      distanceMeters: 20000,
      validated: false,
    };
    candidate.handoff = {
      origin: { lat: 35.1, lon: 139.1 },
      destination: { lat: 35.1, lon: 139.1 },
      waypoints: [],
      mapsUrl: "https://www.google.com/maps/dir/?api=1&origin=35.100000,139.100000&destination=35.100000,139.100000&travelmode=driving",
      verificationSetVersion: null,
    };
    delete candidate.anchor;
    delete candidate.routePlan;
    delete candidate.edgeRouteLegs;
    const result = await parseSearchResult(
      JSON.stringify(validResult({ status: "ok", reason: null, candidates: [candidate] })),
    );
    expect(result.candidates[0]?.pairKind).toBe("topologyOnly");

    for (const mutate of [
      (value: Record<string, unknown>) => {
        value.toll = { ...(value.toll as Record<string, unknown>), chargedSectionCount: 1 };
      },
      (value: Record<string, unknown>) => {
        value.reasons = ["TOPOLOGY_ONLY", "ONE_SECTION_TOLL"];
      },
      (value: Record<string, unknown>) => {
        value.eligibilityStatus = "verified_one_section_ahead";
      },
      (value: Record<string, unknown>) => {
        (value.handoff as Record<string, unknown>).mapsUrl = "javascript:alert(1)";
      },
    ]) {
      const invalid = { ...candidate };
      mutate(invalid);
      await expect(
        parseSearchResult(
          JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
        ),
      ).rejects.toThrowError(/topologyOnly|handoff|URL/);
    }
  });

  it("edgeRouteLegs の重複・欠落と legacy 残存フィールドを拒否する", async () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    for (const fragment of [duplicateLegs, missingLegs]) {
      const invalid = { ...candidate, edgeRouteLegs: JSON.parse(fragment).edgeRouteLegs };
      await expect(
        parseSearchResult(
          JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
        ),
      ).rejects.toThrowError(/edgeRouteLegs/);
    }
    const charged = {
      ...candidate,
      toll: { ...(candidate.toll as Record<string, unknown>), chargedSectionCount: 1 },
    };
    await expect(
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [charged] })),
      ),
    ).rejects.toThrowError(/chargedSectionCount/);
  });

  it("edgeRouteLegs の edgeIds スライスと hash 不一致を拒否する", async () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    const invalid = {
      ...candidate,
      routePlan: {
        ...(candidate.routePlan as Record<string, unknown>),
        resolvedRouteSegments: JSON.parse(hashMismatch).resolvedRouteSegments,
      },
    };
    await expect(
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
      ),
    ).rejects.toThrowError(/hash/);
  });

  it("radialReturn の base candidate 必須 field を欠落だけで拒否する", async () => {
    for (const field of ["entryId", "exitId", "edgeIds", "geometry", "toll"]) {
      const invalid = JSON.parse(radialCandidate) as Record<string, unknown>;
      delete invalid[field];
      await expect(
        parseSearchResult(
          JSON.stringify(validResult({ status: "ok", reason: null, candidates: [invalid] })),
        ),
      ).rejects.toThrowError(/field|entry|exit|geometry|toll|edgeIds/);
    }
  });

  it("未知の pairKind を拒否する", async () => {
    const candidate = { ...(JSON.parse(radialCandidate) as Record<string, unknown>), pairKind: "futurePair" };
    await expect(
      parseSearchResult(
        JSON.stringify(validResult({ status: "ok", reason: null, candidates: [candidate] })),
      ),
    ).rejects.toThrowError(/pairKind/);
  });
});

/** 料金 v3 の証拠がそろった、RadialCandidate ベースの toll。 */
function pricedToll(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    billingPairId: "fixture:radial",
    amountYen: 790,
    pricingAt: "2026-09-10T00:00:00Z",
    effectiveFrom: "2022-03-31T15:00:00Z",
    effectiveTo: "2026-09-30T15:00:00Z",
    billingDistanceMeters: 19400,
    tollSource: "official_distance_rule",
    assignmentId: "assignment:2:meguro-tengenji",
    ruleId: "shutoko-etc-ordinary-2022-04",
    evidenceId: "evidence:2025-04:p04:2-meguro-tengenji",
    distanceEvidenceId: "evidence:2025-04:p04:2-meguro-tengenji",
    fareLabel: "普通車ETC基本料金（割引適用前）",
    vehicleClass: "ordinary",
    paymentMethod: "etc",
    fareBasis: "base_toll_excluding_discounts",
    discountsExcluded: true,
    ...overrides,
  };
}

/** radial-valid.json を、Fare 検証したい toll と tariffStatus に差し替えた候補。 */
function radialWithToll(toll: Record<string, unknown>, tariffStatus = "priced"): Record<string, unknown> {
  const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
  candidate.toll = toll;
  candidate.tariffStatus = tariffStatus;
  return candidate;
}

async function parseWithCandidate(candidate: Record<string, unknown>): Promise<unknown> {
  return parseSearchResult(
    JSON.stringify(validResult({ status: "ok", reason: null, candidates: [candidate] })),
  );
}

describe("料金 v3 の実行時検証", () => {
  it("製品スコープと証拠がそろった priced 候補を受け入れる", async () => {
    const result = (await parseWithCandidate(radialWithToll(pricedToll()))) as SearchResult;
    expect(result.candidates[0]?.toll.amountYen).toBe(790);
    expect(result.candidates[0]?.toll.fareLabel).toBe("普通車ETC基本料金（割引適用前）");
  });

  it("未確定（unpriced）は製品スコープだけを持ち、金額も証拠も持たない", async () => {
    const unpriced = {
      billingPairId: "fixture:radial",
      amountYen: null,
      pricingAt: "2026-09-10T00:00:00Z",
      effectiveFrom: null,
      effectiveTo: null,
      billingDistanceMeters: null,
      fareLabel: "普通車ETC基本料金（割引適用前）",
      vehicleClass: "ordinary",
      paymentMethod: "etc",
      fareBasis: "base_toll_excluding_discounts",
      discountsExcluded: true,
    };
    const result = (await parseWithCandidate(radialWithToll(unpriced, "unpriced"))) as SearchResult;
    expect(result.candidates[0]?.toll.amountYen).toBeNull();
  });

  it("製品スコープが違えば RESULT_CONTRACT_MISMATCH で捨てる", async () => {
    for (const overrides of [
      { fareLabel: "普通車ETC基本料金" },
      { vehicleClass: "truck" },
      { paymentMethod: "cash" },
      { fareBasis: "base_toll" },
      { discountsExcluded: false },
    ]) {
      await expect(parseWithCandidate(radialWithToll(pricedToll(overrides)))).rejects.toThrowError(
        /製品スコープ/,
      );
    }
  });

  it("priced でも証拠が欠けた候補を捨てる", async () => {
    for (const field of [
      "assignmentId",
      "ruleId",
      "evidenceId",
      "distanceEvidenceId",
      "billingDistanceMeters",
      "effectiveFrom",
    ]) {
      const toll = pricedToll();
      delete toll[field];
      await expect(parseWithCandidate(radialWithToll(toll))).rejects.toThrowError(/証拠/);
    }
    await expect(
      parseWithCandidate(radialWithToll(pricedToll({ tollSource: "table" }))),
    ).rejects.toThrowError(/証拠/);
  });

  it("提示した金額が適用期間の外にあれば捨てる", async () => {
    // 2026-10 期の金額を 2026-09 の時点で提示している状態。
    await expect(
      parseWithCandidate(
        radialWithToll(
          pricedToll({
            effectiveFrom: "2026-09-30T15:00:00Z",
            effectiveTo: null,
            ruleId: "shutoko-etc-ordinary-2026-10",
            evidenceId: "evidence:2026-10:p04:2-meguro-tengenji",
            distanceEvidenceId: "evidence:2026-10:p04:2-meguro-tengenji",
            amountYen: 860,
          }),
        ),
      ),
    ).rejects.toThrowError(/適用期間/);
    // 終了時刻ちょうどの提示も半開区間の外。期間の外にあることを理由に落とす。
    await expect(
      parseWithCandidate(
        radialWithToll(
          pricedToll({
            pricingAt: "2026-09-30T15:00:00Z",
            effectiveTo: "2026-09-30T15:00:00Z",
          }),
        ),
      ),
    ).rejects.toThrowError(/適用期間/);
  });

  it("未確定の候補に金額や証拠が残っていれば捨てる", async () => {
    const unpricedBase = {
        billingPairId: "fixture:radial",
        amountYen: null,
        pricingAt: "2026-09-10T00:00:00Z",
        effectiveFrom: null,
        effectiveTo: null,
        billingDistanceMeters: null,
        fareLabel: "普通車ETC基本料金（割引適用前）",
        vehicleClass: "ordinary",
        paymentMethod: "etc",
        fareBasis: "base_toll_excluding_discounts",
      discountsExcluded: true,
    };
    // 状態が未確定なのに金額が入っていれば、状態との不一致として先に落とす。
    await expect(
      parseWithCandidate(radialWithToll({ ...unpricedBase, amountYen: 790 }, "unpriced")),
    ).rejects.toThrowError(/tariffStatus/);
    // 期間・規則 ID・出所など、状態と関係なく残っていれば落とす。
    for (const overrides of [
      { effectiveFrom: "2022-03-31T15:00:00Z" },
      { ruleId: "shutoko-etc-ordinary-2022-04" },
      { tollSource: "official_distance_rule" },
    ]) {
      await expect(
        parseWithCandidate(radialWithToll({ ...unpricedBase, ...overrides }, "unpriced")),
      ).rejects.toThrowError(/確定/);
    }
  });

  it("料金 v3 の証拠が無い旧 release の候補は後方互換で受け入れる", async () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    const result = (await parseWithCandidate(candidate)) as SearchResult;
    expect(result.candidates[0]?.toll.amountYen).toBeNull();
  });

  it("legacyRing 候補の料金も検査する", async () => {
    const mapsUrl =
      "https://www.google.com/maps/dir/?api=1&origin=35.000000,139.000000&destination=35.100000,139.100000&travelmode=driving";
    const legacy = legacyCandidateWithUrl(mapsUrl);
    legacy.toll = pricedToll({ chargedSectionCount: 1 });
    legacy.tariffStatus = "priced";
    const result = (await parseWithCandidate(legacy)) as SearchResult;
    expect(result.candidates[0]?.toll.fareLabel).toBe("普通車ETC基本料金（割引適用前）");

    // 製品スコープを外した legacyRing は evidence 検査で落ちる。
    const invalid = legacyCandidateWithUrl(mapsUrl);
    invalid.toll = pricedToll({ chargedSectionCount: 1, fareLabel: "割引適用後の料金" });
    invalid.tariffStatus = "priced";
    await expect(parseWithCandidate(invalid)).rejects.toThrowError(/製品スコープ/);
  });

  it("legacyRing 候補は tariffStatus を持たなくても通す（旧 engine 互換）", async () => {
    const legacy = legacyCandidateWithUrl(
      "https://www.google.com/maps/dir/?api=1&origin=35.000000,139.000000&destination=35.100000,139.100000&travelmode=driving",
    );
    delete legacy.tariffStatus;
    await expect(parseWithCandidate(legacy)).resolves.toMatchObject({ candidates: [{}] });
  });

  it("radialReturn / topologyOnly は tariffStatus が無ければ捨てる", async () => {
    const candidate = JSON.parse(radialCandidate) as Record<string, unknown>;
    delete candidate.tariffStatus;
    await expect(parseWithCandidate(candidate)).rejects.toThrowError(/tariffStatus/);
  });
});
