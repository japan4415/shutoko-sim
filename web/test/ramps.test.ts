import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import { hexDigest, type FetchLike, type FetchResponseLike } from "../src/worker/pipeline";
import {
  DIRECTION_NAMES,
  filterRamps,
  formatCountInfo,
  formatDirection,
  formatRoute,
  getRampEligibility,
  loadRampsDataset,
  ROUTE_NAMES,
  validateExplicitSearch,
  validateRampsArtifact,
  duplicateOperationMessage,
  type EndpointCapabilities,
  type RampItem,
} from "../src/ui/ramps";

const root = new URL("../../", import.meta.url);

async function loadFixtureRampsAndManifest() {
  const rampsJsonText = await readFile(new URL("fixtures/generated/ramps.json", root), "utf8");
  const manifestJsonText = await readFile(new URL("fixtures/generated/manifest.json", root), "utf8");
  const rawRamps = JSON.parse(rampsJsonText);
  const manifest = JSON.parse(manifestJsonText);
  return { rawRamps, manifest, rampsJsonText, manifestJsonText };
}

describe("ramps.ts: ランプ台帳の検証・正規化・契約チェック", () => {
  it("実 fixtures/generated/ramps.json と manifest.json が整合し全 399 件が検証を通る", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const capabilities = manifest.coverage.endpointCapabilities as EndpointCapabilities;

    const ramps = validateRampsArtifact(rawRamps, capabilities, manifest.releaseId);
    expect(ramps).toHaveLength(399);

    // 契約件数の一致
    // routable 197件（entry 99 / exit 98）
    // structural_no_loop 35件（entry 13 / exit 22）
    // unsupported 139件（entry 70 / exit 69）
    // closed 4件（entry 2 / exit 2）
    // boundary 24件（boundary_in 12 / boundary_out 12）
    let routableEntries = 0;
    let routableExits = 0;
    let structNoLoopEntries = 0;
    let structNoLoopExits = 0;
    let unsupportedEntries = 0;
    let unsupportedExits = 0;
    let closedEntries = 0;
    let closedExits = 0;
    let boundaryIn = 0;
    let boundaryOut = 0;

    for (const r of ramps) {
      if (r.kind === "general_entry") {
        if (r.status === "closed") closedEntries++;
        else if (r.routingCapability === "routable") routableEntries++;
        else if (r.routingCapability === "structural_no_loop") structNoLoopEntries++;
        else if (r.routingCapability === "unsupported") unsupportedEntries++;
      } else if (r.kind === "general_exit") {
        if (r.status === "closed") closedExits++;
        else if (r.routingCapability === "routable") routableExits++;
        else if (r.routingCapability === "structural_no_loop") structNoLoopExits++;
        else if (r.routingCapability === "unsupported") unsupportedExits++;
      } else if (r.kind === "boundary_in") {
        boundaryIn++;
      } else if (r.kind === "boundary_out") {
        boundaryOut++;
      }
    }

    expect(routableEntries).toBe(99);
    expect(routableExits).toBe(98);
    expect(structNoLoopEntries).toBe(13);
    expect(structNoLoopExits).toBe(22);
    expect(unsupportedEntries).toBe(70);
    expect(unsupportedExits).toBe(69);
    expect(closedEntries).toBe(2);
    expect(closedExits).toBe(2);
    expect(boundaryIn).toBe(12);
    expect(boundaryOut).toBe(12);
  });

  it("重複 ID がある場合は fail closed（PipelineError）になる", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const cloned = JSON.parse(JSON.stringify(rawRamps));
    cloned.ramps.push({ ...cloned.ramps[0] });

    expect(() => validateRampsArtifact(cloned, manifest.coverage.endpointCapabilities)).toThrowError(
      /重複 ID/,
    );
  });

  it("未知の kind, status, supportState, routingCapability は fail closed になる", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();

    const testUnknown = (patch: Record<string, unknown>, expectedMsg: RegExp) => {
      const cloned = JSON.parse(JSON.stringify(rawRamps));
      cloned.ramps[0] = { ...cloned.ramps[0], ...patch };
      expect(() => validateRampsArtifact(cloned, manifest.coverage.endpointCapabilities)).toThrowError(
        expectedMsg,
      );
    };

    testUnknown({ kind: "invalid_kind" }, /未知の kind/);
    testUnknown({ status: "invalid_status" }, /未知の status/);
    testUnknown({ supportState: "invalid_support" }, /未知の supportState/);
    testUnknown({ routingCapability: "invalid_cap" }, /未知の routingCapability/);
    testUnknown({ lat: "not_a_number" }, /有限数値ではありません/);
  });

  it("manifest の capability 件数や ID 集合と不整合がある場合は fail closed になる", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const badCapabilities: EndpointCapabilities = {
      ...manifest.coverage.endpointCapabilities,
      routableEntryCount: 999, // 不整合
    };

    expect(() => validateRampsArtifact(rawRamps, badCapabilities)).toThrowError(
      /routableEntryCount 不整合/,
    );

    const badIds: EndpointCapabilities = {
      ...manifest.coverage.endpointCapabilities,
      routableEntryRampIds: ["ramp:non-existent-id"],
    };
    expect(() => validateRampsArtifact(rawRamps, badIds)).toThrowError(/不一致/);
  });
});

describe("ramps.ts: loadRampsDataset の整合性と改ざん検知", () => {
  it("正しい manifest と ramps.json を読み込んで RampsDataset を構築できる", async () => {
    const { rawRamps, manifest, rampsJsonText, manifestJsonText } = await loadFixtureRampsAndManifest();
    const releaseId = manifest.releaseId;

    const rampsBytes = new TextEncoder().encode(rampsJsonText);
    const manifestBytes = new TextEncoder().encode(manifestJsonText);

    const files: Record<string, Uint8Array> = {
      [`/releases/${releaseId}/manifest.json`]: manifestBytes,
      [`/releases/${releaseId}/ramps.json`]: rampsBytes,
    };

    const fetchImpl: FetchLike = async (url: string): Promise<FetchResponseLike> => {
      const bytes = files[url];
      if (!bytes) {
        return {
          ok: false,
          status: 404,
          async text() {
            return "";
          },
          async arrayBuffer() {
            return new ArrayBuffer(0);
          },
        };
      }
      return {
        ok: true,
        status: 200,
        async text() {
          return new TextDecoder().decode(bytes);
        },
        async arrayBuffer() {
          return bytes.slice().buffer as ArrayBuffer;
        },
      };
    };

    const dataset = await loadRampsDataset(fetchImpl, releaseId);
    expect(dataset.ramps).toHaveLength(399);
    expect(dataset.rampMap.size).toBe(399);
    expect(dataset.capabilities.routableEntryCount).toBe(99);
    expect(dataset.capabilities.routableExitCount).toBe(98);
  });

  it("ramps.json のハッシュ改ざん時は ARTIFACT_MISMATCH で停止する", async () => {
    const { manifest, manifestJsonText } = await loadFixtureRampsAndManifest();
    const releaseId = manifest.releaseId;

    const files: Record<string, Uint8Array> = {
      [`/releases/${releaseId}/manifest.json`]: new TextEncoder().encode(manifestJsonText),
      [`/releases/${releaseId}/ramps.json`]: new TextEncoder().encode("tampered content"),
    };

    const fetchImpl: FetchLike = async (url: string): Promise<FetchResponseLike> => {
      const bytes = files[url];
      return {
        ok: true,
        status: 200,
        async text() {
          return new TextDecoder().decode(bytes);
        },
        async arrayBuffer() {
          return bytes.slice().buffer as ArrayBuffer;
        },
      };
    };

    await expect(loadRampsDataset(fetchImpl, releaseId)).rejects.toThrowError(
      /sha256\/byteLength が期待値と不一致/,
    );
  });
});

describe("ramps.ts: 入口・出口の役割分離と非対応理由（getRampEligibility）", () => {
  it("入口選択において、routable な entry のみが選択可能（99件）", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const ramps = validateRampsArtifact(rawRamps, manifest.coverage.endpointCapabilities);

    let selectableCount = 0;
    let wrongKindCount = 0;
    let structNoLoopCount = 0;
    let unsupportedCount = 0;
    let closedCount = 0;
    let boundaryCount = 0;

    for (const r of ramps) {
      const el = getRampEligibility(r, "entry");
      if (el.selectable) {
        selectableCount++;
        expect(el.statusLabel).toBe("選択可能");
        expect(el.reason).toBe("");
        expect(r.kind).toBe("general_entry");
        expect(r.routingCapability).toBe("routable");
      } else {
        if (el.category === "wrong_kind") {
          wrongKindCount++;
          expect(el.statusLabel).toBe("出口専用");
          expect(el.reason).toContain("出口専用ランプのため入口としては選択できません");
        } else if (el.category === "structural_no_loop") {
          structNoLoopCount++;
          expect(el.statusLabel).toBe("周回不可");
          expect(el.reason).toContain("NO_LOOP");
        } else if (el.category === "unsupported") {
          unsupportedCount++;
          expect(el.statusLabel).toBe("未対応");
          expect(el.reason).toContain("未対応:");
        } else if (el.category === "closed") {
          closedCount++;
          expect(el.statusLabel).toBe("閉鎖済み");
          expect(el.reason).toContain("閉鎖済み施設のため選択できません");
        } else if (el.category === "boundary") {
          boundaryCount++;
          expect(el.statusLabel).toBe("境界JCT");
          expect(el.reason).toContain("境界JCT");
        }
      }
    }

    expect(selectableCount).toBe(99);
    expect(wrongKindCount).toBe(191);
    expect(structNoLoopCount).toBe(13);
    expect(unsupportedCount).toBe(70);
    expect(closedCount).toBe(2);
    expect(boundaryCount).toBe(24);
  });

  it("出口選択において、routable な exit のみが選択可能（98件）", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const ramps = validateRampsArtifact(rawRamps, manifest.coverage.endpointCapabilities);

    let selectableCount = 0;
    let wrongKindCount = 0;
    let structNoLoopCount = 0;
    let unsupportedCount = 0;
    let closedCount = 0;
    let boundaryCount = 0;

    for (const r of ramps) {
      const el = getRampEligibility(r, "exit");
      if (el.selectable) {
        selectableCount++;
        expect(el.statusLabel).toBe("選択可能");
        expect(el.reason).toBe("");
        expect(r.kind).toBe("general_exit");
        expect(r.routingCapability).toBe("routable");
      } else {
        if (el.category === "wrong_kind") {
          wrongKindCount++;
          expect(el.statusLabel).toBe("入口専用");
          expect(el.reason).toContain("入口専用ランプのため出口としては選択できません");
        } else if (el.category === "structural_no_loop") {
          structNoLoopCount++;
          expect(el.statusLabel).toBe("周回不可");
          expect(el.reason).toContain("NO_LOOP");
        } else if (el.category === "unsupported") {
          unsupportedCount++;
          expect(el.statusLabel).toBe("未対応");
          expect(el.reason).toContain("未対応:");
        } else if (el.category === "closed") {
          closedCount++;
          expect(el.statusLabel).toBe("閉鎖済み");
          expect(el.reason).toContain("閉鎖済み施設のため選択できません");
        } else if (el.category === "boundary") {
          boundaryCount++;
          expect(el.statusLabel).toBe("境界JCT");
          expect(el.reason).toContain("境界JCT");
        }
      }
    }

    expect(selectableCount).toBe(98);
    expect(wrongKindCount).toBe(184);
    expect(structNoLoopCount).toBe(22);
    expect(unsupportedCount).toBe(69);
    expect(closedCount).toBe(2);
    expect(boundaryCount).toBe(24);
  });
});

describe("ramps.ts: 検索絞り込みと件数インフォメーション（filterRamps & formatCountInfo）", () => {
  it("空クエリで 399 件すべてを返し、選択可能件数が正確に集計される", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const ramps = validateRampsArtifact(rawRamps, manifest.coverage.endpointCapabilities);

    const entryFilter = filterRamps(ramps, "", "entry");
    expect(entryFilter.totalCount).toBe(399);
    expect(entryFilter.selectableCount).toBe(99);
    expect(entryFilter.matchedCount).toBe(399);
    expect(entryFilter.matchedSelectableCount).toBe(99);
    expect(entryFilter.items).toHaveLength(399);
    // ソート順で先頭 99 件は選択可能
    expect(entryFilter.items.slice(0, 99).every((item) => item.eligibility.selectable)).toBe(true);
    expect(formatCountInfo(entryFilter, false)).toBe("全 399 件（選択可能 99 件）");

    const exitFilter = filterRamps(ramps, "", "exit");
    expect(exitFilter.totalCount).toBe(399);
    expect(exitFilter.selectableCount).toBe(98);
    expect(exitFilter.matchedCount).toBe(399);
    expect(exitFilter.matchedSelectableCount).toBe(98);
    expect(exitFilter.items).toHaveLength(399);
    expect(exitFilter.items.slice(0, 98).every((item) => item.eligibility.selectable)).toBe(true);
    expect(formatCountInfo(exitFilter, false)).toBe("全 399 件（選択可能 98 件）");
  });

  it("施設名、路線コード・名前、方向名、ID での絞り込みが機能する", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const ramps = validateRampsArtifact(rawRamps, manifest.coverage.endpointCapabilities);

    // 施設名 "宝町" (C1内回り入口とC1外回り出口の2件)
    const takaracho = filterRamps(ramps, "宝町", "entry");
    expect(takaracho.matchedCount).toBe(2);
    expect(takaracho.matchedSelectableCount).toBe(1);
    expect(formatCountInfo(takaracho, true)).toBe("該当 2 件（うち選択可能 1 件）/ 全体 399 件（選択可能 99 件）");

    // 路線 "都心環状線" または "C1"
    const c1 = filterRamps(ramps, "都心環状線", "entry");
    expect(c1.matchedCount).toBe(33);

    // 方向 "内回り"
    const inner = filterRamps(ramps, "内回り", "entry");
    expect(inner.matchedCount).toBe(38);

    // ID 部分一致
    const idFilter = filterRamps(ramps, "ramp:k1-inbound", "entry");
    expect(idFilter.matchedCount).toBeGreaterThan(0);
  });

  it("該当なし（0件）のときは親切な復帰メッセージが返る", async () => {
    const { rawRamps, manifest } = await loadFixtureRampsAndManifest();
    const ramps = validateRampsArtifact(rawRamps, manifest.coverage.endpointCapabilities);

    const zeroResult = filterRamps(ramps, "存在しないランプ名xyz", "entry");
    expect(zeroResult.matchedCount).toBe(0);
    expect(zeroResult.items).toHaveLength(0);
    expect(formatCountInfo(zeroResult, true)).toContain("該当するランプが見つかりませんでした（0 件 / 全 399 件）");
  });
});

describe("ramps.ts: 明示検索バリデーションと重複操作検知", () => {
  it("未選択・片方のみ選択時のエラー文言を正確に返す", () => {
    expect(validateExplicitSearch(null, null).valid).toBe(false);
    expect(validateExplicitSearch(null, null).error).toContain("入口と出口が未選択です");

    expect(validateExplicitSearch(null, "ramp:c1-outer:takaracho-exit").valid).toBe(false);
    expect(validateExplicitSearch(null, "ramp:c1-outer:takaracho-exit").error).toContain("入口が未選択です");

    expect(validateExplicitSearch("ramp:c1-inner:takaracho-entry", null).valid).toBe(false);
    expect(validateExplicitSearch("ramp:c1-inner:takaracho-entry", null).error).toContain("出口が未選択です");

    expect(
      validateExplicitSearch("ramp:c1-inner:takaracho-entry", "ramp:c1-outer:takaracho-exit").valid,
    ).toBe(true);
  });

  it("同一条件の重複操作を検知する", () => {
    const condition = {
      entryRampId: "ramp:c1-inner:takaracho-entry",
      exitRampId: "ramp:c1-outer:takaracho-exit",
      minMinutes: 15,
      maxMinutes: 60,
      origin: { lat: 35.68, lon: 139.76 },
    };

    expect(duplicateOperationMessage(condition, null)).toBeNull();
    expect(duplicateOperationMessage(condition, { ...condition })).toContain(
      "同じ結果が既に表示されています",
    );
    expect(duplicateOperationMessage(condition, { ...condition, minMinutes: 20 })).toBeNull();
  });
});
