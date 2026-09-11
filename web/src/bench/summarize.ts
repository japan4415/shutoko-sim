// bench envelope の集計と目標合否判定。
//
// 依存を持たない純粋関数のみで構成し、ブラウザ（計測ページの表示）と Node
// （Vitest / レポート生成）の双方から同じ集計を使えるようにする。
//
// 転送量の扱い（scout-003 F11 / scout-004 F9・F15）:
// - `transferSize` はレスポンスヘッダ込みの実転送バイト。キャッシュヒットでは 0。
// - `encodedBodySize` は圧縮後のボディ長。
// - `decodedBodySize` は伸長後（graph.json では約 7.6 倍）であり、**転送量ではない**。
//   本モジュールは decodedBodySize を一切合算しない。
import type {
  BenchEnvelope,
  BenchMemorySource,
  BenchPageLoad,
  BenchResourceEntry,
  BenchTargets,
  BenchTrial,
} from "./envelope";

/**
 * nearest-rank 法によるパーセンタイル（線形補間ではない）。
 *
 * 昇順ソートした n 件に対し `rank = ceil(p * n)`（1 始まり）の値を返す。
 * 例: 20 件の p95 は rank 19 → `sorted[18]`。線形補間と違い、返る値は必ず
 * 実測値のいずれかになる（レポートで「実測に無い値」を報告しないため）。
 * 空配列では null、p が [0, 1] の外なら RangeError。
 */
export function percentile(values: readonly number[], p: number): number | null {
  if (values.length === 0) {
    return null;
  }
  if (!Number.isFinite(p) || p < 0 || p > 1) {
    throw new RangeError(`percentile: p は 0〜1 の有限値である必要があります: ${String(p)}`);
  }
  const sorted = [...values].sort((a, b) => a - b);
  if (p === 0) {
    return sorted[0] ?? null;
  }
  const rank = Math.ceil(p * sorted.length);
  const index = Math.min(Math.max(rank, 1), sorted.length) - 1;
  return sorted[index] ?? null;
}

/**
 * Resource Timing エントリ群の転送バイト数。
 *
 * `transferSize` の合計を返す。全エントリで 0 の場合（キャッシュヒット、または
 * cross-origin マスク）は `encodedBodySize` の合計を下限として返す。
 * `decodedBodySize` は伸長後サイズで転送量ではないため使わない。
 */
export function transferredBytes(entries: readonly BenchResourceEntry[]): number {
  const transfer = entries.reduce((sum, entry) => sum + Math.max(0, entry.transferSize), 0);
  if (transfer > 0) {
    return transfer;
  }
  return entries.reduce((sum, entry) => sum + Math.max(0, entry.encodedBodySize), 0);
}

/** 計測ページ自身のロードで発生した転送バイト数（navigation + ページ自身のリソース）。 */
export function pageLoadTransferBytes(pageLoad: BenchPageLoad): number {
  const navigation = pageLoad.navigation;
  const navigationBytes =
    navigation === null
      ? 0
      : navigation.transferSize > 0
        ? navigation.transferSize
        : navigation.encodedBodySize;
  return navigationBytes + transferredBytes(pageLoad.resources);
}

/**
 * cold 試行の「初回ロード〜候補表示」時間。
 *
 * 計測ページのナビゲーションは試行より前に 1 度だけ起きるため、試行単位の
 * `tFirstCandidateMs`（Worker 生成〜候補描画）に、ページ自身のロードのうち
 * **`responseEnd` 〜 `loadEventEnd`** の区間を加算した値を初回ロード時間として扱う。
 * `navigation` が取れない環境ではページ側の加算を 0 とする。
 * `tFirstCandidateMs` が null（結果が得られなかった試行）では null を返す。
 */
export function firstLoadMs(trial: BenchTrial, pageLoad: BenchPageLoad): number | null {
  if (trial.tFirstCandidateMs === null) {
    return null;
  }
  const navigation = pageLoad.navigation;
  const pagePortion =
    navigation === null ? 0 : Math.max(0, navigation.loadEventEnd - navigation.responseEnd);
  return pagePortion + trial.tFirstCandidateMs;
}

/** 1 統計量の要約（null は該当値が 1 件も無いことを表す）。 */
export interface StatSummary {
  count: number;
  p50: number | null;
  p95: number | null;
  max: number | null;
}

function summarize(values: readonly number[]): StatSummary {
  return {
    count: values.length,
    p50: percentile(values, 0.5),
    p95: percentile(values, 0.95),
    max: values.length === 0 ? null : Math.max(...values),
  };
}

/** 条件別（全体 / cold / warm / パターン別）の集計結果。 */
export interface GroupAggregate {
  key: string;
  label: string;
  trialCount: number;
  timeoutCount: number;
  /** 10 秒上限到達数 / 試行数。試行 0 件なら 0。 */
  timeoutRate: number;
  tSearch: StatSummary;
  tFirstCandidate: StatSummary;
  /** `firstLoadMs` の統計。cold の初回ロード判定に使う。 */
  firstLoad: StatSummary;
  /** cold 試行の転送量合計（成果物 + pageLoad）の最大。cold 試行が無ければ null。 */
  coldTransferBytes: number | null;
  /** cold 試行の圧縮後ボディ合計の最大（transferSize が取れない場合の下限）。 */
  coldEncodedBodyBytes: number | null;
  /** 試行のピークメモリ最大（MiB）。1 件も取れなければ null。 */
  memoryPeakMiB: number | null;
}

/** envelope 全体の集計。 */
export interface BenchAggregate {
  trialCount: number;
  timeoutCount: number;
  timeoutRate: number;
  memorySource: BenchMemorySource;
  overall: GroupAggregate;
  cold: GroupAggregate;
  warm: GroupAggregate;
  byPattern: GroupAggregate[];
}

function numbers(values: readonly (number | null)[]): number[] {
  return values.filter((value): value is number => value !== null && Number.isFinite(value));
}

/**
 * envelope のメモリ出所を試行から導く。
 *
 * 最大ピークを出した試行の出所を代表とする（代理計測の envelope は
 * `"cdp-performance-metrics"`、ブラウザ単体の envelope は `"performance.memory"`）。
 * ピークが 1 件も無ければ null（= 判定不能）。
 */
function derivedMemorySource(trials: readonly BenchTrial[]): BenchMemorySource {
  let best: BenchTrial | null = null;
  for (const trial of trials) {
    if (trial.memoryPeakMiB === null || trial.memorySource === null) {
      continue;
    }
    if (best === null || trial.memoryPeakMiB > (best.memoryPeakMiB ?? 0)) {
      best = trial;
    }
  }
  return best === null ? null : best.memorySource;
}

function buildGroup(
  key: string,
  label: string,
  trials: readonly BenchTrial[],
  pageLoad: BenchPageLoad,
): GroupAggregate {
  const timeoutCount = trials.filter((trial) => trial.timeout).length;
  const coldTrials = trials.filter((trial) => trial.cache === "cold");
  const coldTotals = coldTrials.map((trial) => transferredBytes(trial.resources));
  const coldPageBytes = pageLoadTransferBytes(pageLoad);
  const coldEncodedTotals = coldTrials.map((trial) =>
    trial.resources.reduce((sum, entry) => sum + Math.max(0, entry.encodedBodySize), 0),
  );
  const memoryPeaks = numbers(trials.map((trial) => trial.memoryPeakMiB));
  return {
    key,
    label,
    trialCount: trials.length,
    timeoutCount,
    timeoutRate: trials.length === 0 ? 0 : timeoutCount / trials.length,
    tSearch: summarize(numbers(trials.map((trial) => trial.tSearchMs))),
    tFirstCandidate: summarize(numbers(trials.map((trial) => trial.tFirstCandidateMs))),
    firstLoad: summarize(numbers(trials.map((trial) => firstLoadMs(trial, pageLoad)))),
    coldTransferBytes:
      coldTotals.length === 0 ? null : Math.max(...coldTotals) + coldPageBytes,
    coldEncodedBodyBytes:
      coldEncodedTotals.length === 0 ? null : Math.max(...coldEncodedTotals) + coldPageBytes,
    memoryPeakMiB: memoryPeaks.length === 0 ? null : Math.max(...memoryPeaks),
  };
}

/**
 * envelope（複数可）を条件別に集計する。
 *
 * メモリは「手入力があればそれを正とする」: 手入力があれば `memorySource` は
 * `"manual"` で `memoryPeakMiB` は手入力値、無ければ試行から取れた最大値、
 * どの試行でも取れなければ null（= 判定不能）。
 */
export function aggregate(input: BenchEnvelope | readonly BenchEnvelope[]): BenchAggregate {
  const envelopes = Array.isArray(input) ? input : [input as BenchEnvelope];
  const trials = envelopes.flatMap((envelope) => envelope.trials);
  const pageLoad = envelopes[0]?.pageLoad ?? { url: "", navigation: null, resources: [] };
  const manual = envelopes.map((envelope) => envelope.memoryManualMiB).find((value) => value !== null) ?? null;

  const cold = buildGroup("cold", "cold（キャッシュなし）", trials.filter((t) => t.cache === "cold"), pageLoad);
  const warm = buildGroup("warm", "warm（キャッシュあり）", trials.filter((t) => t.cache === "warm"), pageLoad);
  const overall = buildGroup("overall", "全体", trials, pageLoad);

  const patternKeys = [...new Set(trials.map((trial) => trial.patternIndex))].sort((a, b) => a - b);
  const byPattern = patternKeys.map((patternIndex) => {
    const groupTrials = trials.filter((trial) => trial.patternIndex === patternIndex);
    const first = groupTrials[0];
    return buildGroup(
      `pattern-${String(patternIndex)}`,
      first === undefined ? String(patternIndex) : `${String(patternIndex)}: ${first.patternId}`,
      groupTrials,
      pageLoad,
    );
  });

  if (manual !== null) {
    for (const group of [overall, cold, warm, ...byPattern]) {
      group.memoryPeakMiB = manual;
    }
  }

  return {
    trialCount: trials.length,
    timeoutCount: trials.filter((trial) => trial.timeout).length,
    timeoutRate: trials.length === 0 ? 0 : trials.filter((trial) => trial.timeout).length / trials.length,
    memorySource: manual !== null ? "manual" : derivedMemorySource(trials),
    overall,
    cold,
    warm,
    byPattern,
  };
}

/** 目標 1 件の判定結果。 */
export interface JudgeVerdict {
  key: "searchP95" | "firstLoadP95" | "transfer" | "memory";
  label: string;
  /** 目標値（ms / バイト / MiB）。 */
  target: number;
  /** 実測値。null は「測れなかった」ことを表し、判定は unknown。 */
  actual: number | null;
  verdict: "pass" | "fail" | "unknown";
}

function verdictOf(actual: number | null, target: number): "pass" | "fail" | "unknown" {
  if (actual === null) {
    return "unknown";
  }
  return actual <= target ? "pass" : "fail";
}

/**
 * 集計結果を目標（docs/delivery.md:37-40）と突き合わせる。
 *
 * - 探索 p95: 全試行の `tSearchMs` p95
 * - 初回ロード p95: cold 試行の `firstLoadMs` p95
 * - 転送量: cold 試行の転送量合計（成果物 + pageLoad）の最大
 * - メモリ: 集計の `memoryPeakMiB`（手入力があれば手入力値）
 *
 * メモリが null（`performance.memory` も手入力も無い環境）なら `unknown`。
 */
export function judge(agg: BenchAggregate, targets: BenchTargets): JudgeVerdict[] {
  return [
    {
      key: "searchP95",
      label: "成果物取得済み探索 p95",
      target: targets.searchP95Ms,
      actual: agg.overall.tSearch.p95,
      verdict: verdictOf(agg.overall.tSearch.p95, targets.searchP95Ms),
    },
    {
      key: "firstLoadP95",
      label: "初回ロード〜候補表示 p95（cold）",
      target: targets.firstLoadP95Ms,
      actual: agg.cold.firstLoad.p95,
      verdict: verdictOf(agg.cold.firstLoad.p95, targets.firstLoadP95Ms),
    },
    {
      key: "transfer",
      label: "cold 転送量 最大（成果物 + pageLoad）",
      target: targets.transferBytesMax,
      actual: agg.cold.coldTransferBytes,
      verdict: verdictOf(agg.cold.coldTransferBytes, targets.transferBytesMax),
    },
    {
      key: "memory",
      label: "探索ピークメモリ",
      target: targets.memoryMiBMax,
      actual: agg.overall.memoryPeakMiB,
      verdict: verdictOf(agg.overall.memoryPeakMiB, targets.memoryMiBMax),
    },
  ];
}
