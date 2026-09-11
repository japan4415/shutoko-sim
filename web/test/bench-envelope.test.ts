// bench envelope の検証（validateEnvelope）と組み立て（buildEnvelope）のユニットテスト。
import { describe, expect, it } from "vitest";
import {
  BENCH_SCHEMA_VERSION,
  BENCH_TARGETS,
  buildEnvelope,
  isBenchEnvelope,
  resolveMemorySource,
  validateEnvelope,
  type BenchEnvelope,
  type BenchTrial,
} from "../src/bench/envelope";

function trial(overrides: Partial<BenchTrial> = {}): BenchTrial {
  return {
    patternIndex: 0,
    patternId: "kandabashi-mid",
    originId: "kandabashi",
    origin: { lat: 35.689673, lon: 139.764425 },
    minMinutes: 15,
    maxMinutes: 60,
    cache: "cold",
    repeat: 1,
    tTransferMs: 120,
    tSearchMs: 300,
    tFirstCandidateMs: 420,
    timeout: false,
    resultStatus: "ok",
    reason: null,
    candidateCount: 2,
    errorCode: null,
    memoryPeakMiB: 42.5,
    memorySource: "performance.memory",
    resources: [
      {
        name: "/releases/c1-real-v1/graph.json?bench=abc",
        transferSize: 300_000,
        encodedBodySize: 250_000,
        decodedBodySize: 2_907_908,
        deliveryType: null,
        responseStatus: 200,
        duration: 80,
      },
    ],
    ...overrides,
  };
}

function envelope(overrides: Partial<BenchEnvelope> = {}): BenchEnvelope {
  return buildEnvelope({
    releaseId: "c1-real-v1",
    createdAt: "2026-09-11T00:00:00.000Z",
    device: {
      ua: "test-ua",
      platform: "test-platform",
      deviceName: null,
      os: null,
      browser: null,
      network: null,
      note: null,
    },
    memoryManualMiB: null,
    pageLoad: {
      url: "http://localhost:8787/bench.html",
      navigation: {
        startTime: 0,
        responseEnd: 20,
        loadEventEnd: 60,
        domContentLoadedEventEnd: 40,
        transferSize: 800,
        encodedBodySize: 700,
        decodedBodySize: 1500,
      },
      resources: [],
    },
    trials: [trial()],
    ...overrides,
  });
}

describe("validateEnvelope", () => {
  it("既定の目標値は docs/delivery.md の 4 項目", () => {
    expect(BENCH_TARGETS).toEqual({
      searchP95Ms: 2000,
      firstLoadP95Ms: 8000,
      transferBytesMax: 10 * 1024 * 1024,
      memoryMiBMax: 128,
    });
  });

  it("組み立てた envelope は検証を通る（JSON 往復後も同じ）", () => {
    const env = envelope();
    expect(validateEnvelope(env)).toEqual({ valid: true, errors: [] });
    expect(validateEnvelope(JSON.parse(JSON.stringify(env)))).toEqual({ valid: true, errors: [] });
    expect(isBenchEnvelope(env)).toBe(true);
    expect(env.schemaVersion).toBe(BENCH_SCHEMA_VERSION);
  });

  it("オブジェクトでなければ invalid", () => {
    expect(validateEnvelope(null).valid).toBe(false);
    expect(validateEnvelope([]).valid).toBe(false);
    expect(validateEnvelope("envelope").valid).toBe(false);
  });

  it("schemaVersion 不一致は invalid", () => {
    const result = validateEnvelope({ ...envelope(), schemaVersion: 2 });
    expect(result.valid).toBe(false);
    expect(result.errors.join(" ")).toContain("schemaVersion");
  });

  it("trials が空・非配列なら invalid", () => {
    expect(validateEnvelope({ ...envelope(), trials: [] }).valid).toBe(false);
    expect(validateEnvelope({ ...envelope(), trials: "none" }).valid).toBe(false);
  });

  it("試行の cache は cold / warm のみ", () => {
    const bad = { ...envelope(), trials: [trial({ cache: "lukewarm" as unknown as "cold" })] };
    const result = validateEnvelope(bad);
    expect(result.valid).toBe(false);
    expect(result.errors.join(" ")).toContain("cache");
  });

  it("転送量 3 値と duration が欠けた resource は invalid", () => {
    const broken = trial();
    const resource = { ...broken.resources[0] } as Record<string, unknown>;
    delete resource.transferSize;
    delete resource.duration;
    const result = validateEnvelope({ ...envelope(), trials: [{ ...broken, resources: [resource] }] });
    expect(result.valid).toBe(false);
    expect(result.errors.join(" ")).toContain("transferSize");
    expect(result.errors.join(" ")).toContain("duration");
  });

  it("計測不能を表す null は許容する", () => {
    const env = envelope({
      trials: [
        trial({
          tTransferMs: null,
          tSearchMs: null,
          tFirstCandidateMs: null,
          memoryPeakMiB: null,
          memorySource: null,
          resultStatus: null,
          candidateCount: null,
        }),
      ],
      memoryManualMiB: null,
    });
    expect(env.memorySource).toBeNull();
    expect(validateEnvelope(env).valid).toBe(true);
  });

  it("構築後の envelope に対する resolveMemorySource は manual / performance.memory / null の 3 通り", () => {
    expect(envelope({ memoryManualMiB: 96 }).memorySource).toBe("manual");
    expect(envelope().memorySource).toBe("performance.memory");
    expect(
      envelope({ trials: [trial({ memoryPeakMiB: null, memorySource: null })] }).memorySource,
    ).toBeNull();
  });

  it("memorySource は 3 値のみ", () => {
    const result = validateEnvelope({ ...envelope(), memorySource: "instruments" });
    expect(result.valid).toBe(false);
    expect(result.errors.join(" ")).toContain("memorySource");
  });

  it("device / pageLoad / targets の型違反を検出する", () => {
    const bad = {
      ...envelope(),
      device: { ua: 1, platform: null, deviceName: null, os: null, browser: null, network: null, note: null },
      pageLoad: { url: 1, navigation: null, resources: [{}] },
      targets: { searchP95Ms: "2000" },
    };
    const result = validateEnvelope(bad);
    expect(result.valid).toBe(false);
    expect(result.errors.length).toBeGreaterThanOrEqual(5);
  });

  // 以下は #13 第 2 段の代理計測（Playwright ランナー）向けの後方互換な追加分。
  it("cdp-performance-metrics を memorySource として許容する（代理計測）", () => {
    const env = envelope({
      memorySource: "cdp-performance-metrics",
      trials: [trial({ memoryPeakMiB: 128.25, memorySource: "cdp-performance-metrics" })],
    });
    expect(validateEnvelope(env).errors).toEqual([]);
    expect(validateEnvelope(JSON.parse(JSON.stringify(env)) as unknown).valid).toBe(true);
  });

  it("memorySource の未知の値は依然として拒否する", () => {
    const result = validateEnvelope({ ...envelope(), memorySource: "instruments" });
    expect(result.valid).toBe(false);
    expect(result.errors.join(" ")).toContain("cdp-performance-metrics");
  });

  it("device.cpuThrottle は任意で、あるときだけ型を見る", () => {
    const withThrottle = {
      ...envelope(),
      device: {
        ua: "ua",
        platform: "p",
        deviceName: "Pixel 5 emulation",
        os: "desktop macOS",
        browser: "Chromium 141.0.0.0",
        network: "fast4g (CDP)",
        cpuThrottle: 4,
        note: null,
      },
    };
    expect(validateEnvelope(withThrottle).errors).toEqual([]);
    expect(
      validateEnvelope({ ...withThrottle, device: { ...withThrottle.device, cpuThrottle: "4" } }).valid,
    ).toBe(false);
    // 未設定（実機の envelope）はそのまま通る。
    expect(validateEnvelope(envelope()).errors).toEqual([]);
  });
});

describe("resolveMemorySource / buildEnvelope", () => {
  it("手入力があれば manual（試行に値があっても手入力が正）", () => {
    expect(resolveMemorySource([trial()], 96)).toBe("manual");
  });

  it("手入力が無く試行にピークがあれば performance.memory", () => {
    expect(resolveMemorySource([trial()], null)).toBe("performance.memory");
  });

  it("どちらも無ければ null（判定不能）", () => {
    expect(resolveMemorySource([trial({ memoryPeakMiB: null, memorySource: null })], null)).toBeNull();
    expect(resolveMemorySource([], null)).toBeNull();
  });

  it("buildEnvelope は手入力を memoryManualMiB に載せ、目標値の既定を埋める", () => {
    const env = buildEnvelope({
      releaseId: "c1-real-v1",
      createdAt: "2026-09-11T00:00:00.000Z",
      device: {
        ua: "ua",
        platform: "p",
        deviceName: "Pixel 5",
        os: "Android 14",
        browser: "Chrome 130",
        network: "Slow 4G",
        note: null,
      },
      memoryManualMiB: 96,
      pageLoad: { url: "http://localhost:8787/bench.html", navigation: null, resources: [] },
      trials: [trial()],
    });
    expect(env.memoryManualMiB).toBe(96);
    expect(env.memorySource).toBe("manual");
    expect(env.targets).toEqual(BENCH_TARGETS);
    expect(validateEnvelope(env).valid).toBe(true);
  });
});
