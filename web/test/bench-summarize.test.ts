// bench 集計（percentile / aggregate / judge）のユニットテスト。
// ブラウザ不要・決定論的に検証できるよう、定数の入力だけで境界値を突く。
import { describe, expect, it } from "vitest";
import {
  BENCH_TARGETS,
  buildEnvelope,
  type BenchEnvelope,
  type BenchPageLoad,
  type BenchResourceEntry,
  type BenchTrial,
} from "../src/bench/envelope";
import {
  aggregate,
  firstLoadMs,
  judge,
  pageLoadTransferBytes,
  percentile,
  transferredBytes,
} from "../src/bench/summarize";

const PAGE_LOAD: BenchPageLoad = {
  url: "http://localhost:8787/bench.html",
  navigation: {
    startTime: 0,
    responseEnd: 20,
    // responseEnd〜loadEventEnd = 40 ms を初回ロード判定に加算する。
    loadEventEnd: 60,
    domContentLoadedEventEnd: 40,
    transferSize: 1000,
    encodedBodySize: 900,
    decodedBodySize: 4000,
  },
  resources: [
    {
      name: "http://localhost:8787/assets/bench.js",
      transferSize: 500,
      encodedBodySize: 400,
      decodedBodySize: 1200,
      deliveryType: null,
      responseStatus: 200,
      duration: 5,
    },
  ],
};

function resource(transferSize: number, encodedBodySize: number): BenchResourceEntry {
  return {
    name: "/releases/c1-real-v1/graph.json",
    transferSize,
    encodedBodySize,
    decodedBodySize: encodedBodySize * 8,
    deliveryType: null,
    responseStatus: 200,
    duration: 10,
  };
}

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
    tTransferMs: 100,
    tSearchMs: 300,
    tFirstCandidateMs: 1000,
    timeout: false,
    resultStatus: "ok",
    reason: null,
    candidateCount: 2,
    errorCode: null,
    memoryPeakMiB: 40,
    memorySource: "performance.memory",
    resources: [resource(300_000, 250_000)],
    ...overrides,
  };
}

function envelope(trials: BenchTrial[], memoryManualMiB: number | null = null): BenchEnvelope {
  return buildEnvelope({
    releaseId: "c1-real-v1",
    createdAt: "2026-09-11T00:00:00.000Z",
    device: {
      ua: "ua",
      platform: "p",
      deviceName: null,
      os: null,
      browser: null,
      network: null,
      note: null,
    },
    memoryManualMiB,
    pageLoad: PAGE_LOAD,
    trials,
  });
}

describe("percentile（nearest-rank）", () => {
  it("空配列は null", () => {
    expect(percentile([], 0.5)).toBeNull();
    expect(percentile([], 0.95)).toBeNull();
  });

  it("1 件は p によらずその値", () => {
    expect(percentile([7], 0.5)).toBe(7);
    expect(percentile([7], 0.95)).toBe(7);
    expect(percentile([7], 1)).toBe(7);
  });

  it("線形補間せず実測値のいずれかを返す（1..4 の p50 は 2）", () => {
    // 線形補間なら 2.5 になる。nearest-rank は rank = ceil(0.5 * 4) = 2 → 2 番目の値。
    expect(percentile([1, 2, 3, 4], 0.5)).toBe(2);
    expect(percentile([1, 2, 3, 4], 0.95)).toBe(4);
    expect(percentile([1, 2, 3, 4], 0.25)).toBe(1);
    expect(percentile([1, 2, 3, 4], 1)).toBe(4);
    expect(percentile([1, 2, 3, 4], 0)).toBe(1);
  });

  it("20 件の p95 は rank 19（20 番目ではない）", () => {
    const values = Array.from({ length: 20 }, (_, index) => index + 1);
    expect(percentile(values, 0.5)).toBe(10);
    expect(percentile(values, 0.95)).toBe(19);
    expect(percentile(values, 1)).toBe(20);
  });

  it("入力順に依存せず昇順ソートして求める（入力を破壊しない）", () => {
    const values = [5, 1, 4, 2, 3];
    expect(percentile(values, 0.5)).toBe(3);
    expect(values).toEqual([5, 1, 4, 2, 3]);
  });

  it("p が範囲外なら RangeError", () => {
    expect(() => percentile([1], -0.1)).toThrow(RangeError);
    expect(() => percentile([1], 1.1)).toThrow(RangeError);
    expect(() => percentile([1], Number.NaN)).toThrow(RangeError);
  });
});

describe("転送量の合算", () => {
  it("transferSize の合計を使い、encodedBodySize / decodedBodySize と混ぜない", () => {
    expect(transferredBytes([resource(300_000, 250_000), resource(100_000, 90_000)])).toBe(400_000);
  });

  it("全エントリが transferSize 0（キャッシュヒット）なら encodedBodySize にフォールバック", () => {
    expect(transferredBytes([resource(0, 250_000), resource(0, 90_000)])).toBe(340_000);
  });

  it("pageLoad は navigation とページ自身のリソースを合算する", () => {
    // navigation 1000 + bench.js 500
    expect(pageLoadTransferBytes(PAGE_LOAD)).toBe(1500);
  });
});

describe("firstLoadMs", () => {
  it("pageLoad の responseEnd〜loadEventEnd を tFirstCandidate に加算する", () => {
    expect(firstLoadMs(trial({ tFirstCandidateMs: 1000 }), PAGE_LOAD)).toBe(1040);
  });

  it("tFirstCandidate が null なら null", () => {
    expect(firstLoadMs(trial({ tFirstCandidateMs: null }), PAGE_LOAD)).toBeNull();
  });

  it("navigation が取れない環境ではページ側の加算を 0 とする", () => {
    const pageLoad: BenchPageLoad = { url: "u", navigation: null, resources: [] };
    expect(firstLoadMs(trial({ tFirstCandidateMs: 1000 }), pageLoad)).toBe(1000);
  });
});

describe("aggregate", () => {
  it("cold / warm / 全体ごとに p50・p95 と 10 秒到達率を出す", () => {
    const trials = [
      trial({ cache: "cold", repeat: 1, tSearchMs: 100, tFirstCandidateMs: 1000 }),
      trial({ cache: "cold", repeat: 2, tSearchMs: 200, tFirstCandidateMs: 2000 }),
      trial({ cache: "warm", repeat: 1, tSearchMs: 300, tFirstCandidateMs: 3000 }),
      trial({
        cache: "warm",
        repeat: 2,
        tSearchMs: null,
        tFirstCandidateMs: null,
        timeout: true,
        resultStatus: null,
        candidateCount: null,
      }),
    ];
    const agg = aggregate(envelope(trials));
    expect(agg.trialCount).toBe(4);
    expect(agg.timeoutCount).toBe(1);
    expect(agg.timeoutRate).toBeCloseTo(0.25);
    expect(agg.cold.tSearch).toEqual({ count: 2, p50: 100, p95: 200, max: 200 });
    expect(agg.warm.tSearch).toEqual({ count: 1, p50: 300, p95: 300, max: 300 });
    // 全体は null を除いた 3 件で計算する: rank(0.5)=2 → 200、rank(0.95)=3 → 300。
    expect(agg.overall.tSearch).toEqual({ count: 3, p50: 200, p95: 300, max: 300 });
    // cold の初回ロードは responseEnd〜loadEventEnd(40) + tFirstCandidate。
    expect(agg.cold.firstLoad).toEqual({ count: 2, p50: 1040, p95: 2040, max: 2040 });
  });

  it("cold の転送量は「成果物 + pageLoad」の最大を採る", () => {
    const trials = [
      trial({ cache: "cold", repeat: 1, resources: [resource(300_000, 250_000)] }),
      trial({ cache: "cold", repeat: 2, resources: [resource(400_000, 300_000)] }),
      // warm はキャッシュヒット（transferSize 0）なので cold の最大に影響しない。
      trial({ cache: "warm", repeat: 1, resources: [resource(0, 250_000)] }),
    ];
    const agg = aggregate(envelope(trials));
    expect(agg.cold.coldTransferBytes).toBe(400_000 + 1500);
    expect(agg.cold.coldEncodedBodyBytes).toBe(300_000 + 1500);
    expect(agg.warm.coldTransferBytes).toBeNull();
  });

  it("メモリは手入力があればそれを正とする", () => {
    const trials = [trial({ memoryPeakMiB: 40 }), trial({ cache: "warm", memoryPeakMiB: 60 })];
    expect(aggregate(envelope(trials)).overall.memoryPeakMiB).toBe(60);
    const withManual = aggregate(envelope(trials, 96));
    expect(withManual.memorySource).toBe("manual");
    expect(withManual.overall.memoryPeakMiB).toBe(96);
    expect(withManual.cold.memoryPeakMiB).toBe(96);
  });

  it("メモリが 1 件も取れなければ null と performance.memory 以外の出所になる", () => {
    const agg = aggregate(
      envelope([trial({ memoryPeakMiB: null, memorySource: null })]),
    );
    expect(agg.overall.memoryPeakMiB).toBeNull();
    expect(agg.memorySource).toBeNull();
  });

  it("パターン別にまとめ、パターン index 昇順で返す", () => {
    const trials = [
      trial({ patternIndex: 1, patternId: "takaracho-mid" }),
      trial({ patternIndex: 0, patternId: "kandabashi-mid" }),
      trial({ patternIndex: 0, patternId: "kandabashi-mid", cache: "warm" }),
    ];
    const agg = aggregate(envelope(trials));
    expect(agg.byPattern.map((group) => group.key)).toEqual(["pattern-0", "pattern-1"]);
    expect(agg.byPattern[0]?.trialCount).toBe(2);
    expect(agg.byPattern[0]?.label).toBe("0: kandabashi-mid");
  });

  it("複数 envelope を受け取り試行を連結する", () => {
    const agg = aggregate([
      envelope([trial({ cache: "cold" })]),
      envelope([trial({ cache: "warm" })]),
    ]);
    expect(agg.trialCount).toBe(2);
    expect(agg.cold.trialCount).toBe(1);
    expect(agg.warm.trialCount).toBe(1);
  });
});

describe("judge", () => {
  it("目標内なら pass、超過なら fail", () => {
    const agg = aggregate(
      envelope([
        trial({ cache: "cold", tSearchMs: 1500, tFirstCandidateMs: 7000, memoryPeakMiB: 100 }),
      ]),
    );
    const verdicts = judge(agg, BENCH_TARGETS);
    expect(verdicts.map((v) => [v.key, v.verdict])).toEqual([
      ["searchP95", "pass"],
      ["firstLoadP95", "pass"],
      ["transfer", "pass"],
      ["memory", "pass"],
    ]);
    expect(verdicts[0]?.target).toBe(2000);
    expect(verdicts[1]?.actual).toBe(7040);
  });

  it("超過は fail（境界値は pass）", () => {
    const over = judge(
      aggregate(
        envelope([
          trial({ cache: "cold", tSearchMs: 2001, tFirstCandidateMs: 8000, memoryPeakMiB: 129 }),
        ]),
      ),
      BENCH_TARGETS,
    );
    expect(over.map((v) => v.verdict)).toEqual(["fail", "fail", "pass", "fail"]);

    const boundary = judge(
      aggregate(
        envelope([
          trial({
            cache: "cold",
            tSearchMs: 2000,
            tFirstCandidateMs: 8000 - 40,
            memoryPeakMiB: 128,
            resources: [resource(10 * 1024 * 1024 - 1500, 1)],
          }),
        ]),
      ),
      BENCH_TARGETS,
    );
    expect(boundary.map((v) => v.verdict)).toEqual(["pass", "pass", "pass", "pass"]);
  });

  it("メモリが取れない環境では memory だけ unknown", () => {
    const verdicts = judge(
      aggregate(envelope([trial({ memoryPeakMiB: null, memorySource: null })])),
      BENCH_TARGETS,
    );
    expect(verdicts.map((v) => v.verdict)).toEqual(["pass", "pass", "pass", "unknown"]);
    expect(verdicts[3]?.actual).toBeNull();
  });

  it("手入力メモリは判定に使われる（unknown にならない）", () => {
    const verdicts = judge(
      aggregate(envelope([trial({ memoryPeakMiB: null, memorySource: null })], 120)),
      BENCH_TARGETS,
    );
    expect(verdicts[3]).toMatchObject({ verdict: "pass", actual: 120 });
  });

  it("計測できない試行しか無ければ unknown（fail にしない）", () => {
    const trials = [
      trial({
        tSearchMs: null,
        tFirstCandidateMs: null,
        timeout: true,
        resultStatus: null,
        candidateCount: null,
        memoryPeakMiB: null,
        memorySource: null,
        resources: [],
      }),
    ];
    const verdicts = judge(aggregate(envelope(trials)), BENCH_TARGETS);
    // 転送量は「全エントリ transferSize 0」で encodedBodySize 0 → pageLoad 分のみ計上される。
    expect(verdicts.map((v) => v.key)).toEqual(["searchP95", "firstLoadP95", "transfer", "memory"]);
    expect(verdicts[0]?.verdict).toBe("unknown");
    expect(verdicts[1]?.verdict).toBe("unknown");
    expect(verdicts[3]?.verdict).toBe("unknown");
  });
});
