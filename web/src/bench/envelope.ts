// bench ページが出力する計測結果 JSON（envelope）の型と検証。
//
// 依存を持たない純粋モジュールにして、ブラウザ（計測ページ）と Node（Vitest /
// Playwright の smoke）の双方から同じ型・同じ検証を使えるようにする。
// 構造化された値を後段（集計・レポート）が読むため、検証は必須フィールドの網羅と
// 型の取り違え（特に転送量の transferSize / encodedBodySize / decodedBodySize）を
// 重点的に見る。

/** envelope のスキーマ版。構造を変えたら上げる。 */
export const BENCH_SCHEMA_VERSION = 1;

/** 試行のキャッシュ状態。cold = `?bench=<nonce>` 付与でブラウザキャッシュを迂回。 */
export type BenchCacheState = "cold" | "warm";

/** メモリ値の出所。`performance.memory` が取れない環境では null（不合格ではなく判定不能）。 */
export type BenchMemorySource = "performance.memory" | "manual" | null;

/** 性能目標（docs/delivery.md:37-40）。 */
export interface BenchTargets {
  /** 成果物取得済みの探索時間 p95（ms）。 */
  searchP95Ms: number;
  /** 初回ロード〜候補表示 p95（ms）。 */
  firstLoadP95Ms: number;
  /** 圧縮後の転送量（バイト）。 */
  transferBytesMax: number;
  /** 探索ピークメモリ（MiB）。 */
  memoryMiBMax: number;
}

/** 既定の目標値。 */
export const BENCH_TARGETS: BenchTargets = {
  searchP95Ms: 2000,
  firstLoadP95Ms: 8000,
  transferBytesMax: 10 * 1024 * 1024,
  memoryMiBMax: 128,
};

/**
 * Resource Timing の 1 エントリ。3 つのサイズは意味が異なるため必ず 3 つとも記録する。
 * - `transferSize`: レスポンスヘッダ + ボディの実転送バイト（キャッシュヒット時は 0）
 * - `encodedBodySize`: content-encoding 適用後（= 圧縮後）のボディ
 * - `decodedBodySize`: 伸長後のボディ（**転送量として扱ってはならない**）
 */
export interface BenchResourceEntry {
  name: string;
  transferSize: number;
  encodedBodySize: number;
  decodedBodySize: number;
  deliveryType: string | null;
  responseStatus: number | null;
  duration: number;
}

/** 計測ページ自身のナビゲーションタイミング。 */
export interface BenchNavigationTiming {
  startTime: number;
  responseEnd: number;
  loadEventEnd: number;
  domContentLoadedEventEnd: number;
  transferSize: number;
  encodedBodySize: number;
  decodedBodySize: number;
}

/** 計測ページ自身のロード（1 回だけ記録する）。 */
export interface BenchPageLoad {
  url: string;
  navigation: BenchNavigationTiming | null;
  resources: BenchResourceEntry[];
}

/** 端末・ネットワーク条件（手入力欄の値と UA から作る）。 */
export interface BenchDevice {
  ua: string;
  platform: string;
  deviceName: string | null;
  os: string | null;
  browser: string | null;
  network: string | null;
  note: string | null;
}

/** 1 試行の計測値。時刻はすべて `performance.timeOrigin + performance.now()` の epoch ms。 */
export interface BenchTrial {
  patternIndex: number;
  patternId: string;
  originId: string;
  origin: { lat: number; lon: number };
  minMinutes: number;
  maxMinutes: number;
  cache: BenchCacheState;
  /** 同一パターン・同一キャッシュ状態内の試行番号（1 始まり）。 */
  repeat: number;
  /** Worker 生成〜成果物取得・照合・WASM init 完了（`ready` 直前）。 */
  tTransferMs: number | null;
  /** `search` 呼び出し直前〜直後（Worker 内で計測）。 */
  tSearchMs: number | null;
  /** Worker 生成〜候補（または status 文言）の描画を二重 requestAnimationFrame で確定した後。 */
  tFirstCandidateMs: number | null;
  /** 10 秒上限に到達して Worker を terminate したか。 */
  timeout: boolean;
  resultStatus: string | null;
  reason: string | null;
  candidateCount: number | null;
  /** エラーで終わった試行の error.code（TIMEOUT は timeout フラグ側で表す）。 */
  errorCode: string | null;
  /** メインスレッドと Worker の `performance.memory` サンプルの最大（MiB）。 */
  memoryPeakMiB: number | null;
  memorySource: BenchMemorySource;
  /** Worker 内で収集した成果物の Resource Timing エントリ。 */
  resources: BenchResourceEntry[];
}

/** 計測結果の JSON envelope。 */
export interface BenchEnvelope {
  schemaVersion: number;
  createdAt: string;
  releaseId: string;
  device: BenchDevice;
  /** envelope 全体で採用したメモリ値の出所。 */
  memorySource: BenchMemorySource;
  /** 手入力されたピークメモリ（MiB）。未入力は null。 */
  memoryManualMiB: number | null;
  pageLoad: BenchPageLoad;
  targets: BenchTargets;
  trials: BenchTrial[];
}

/** buildEnvelope の入力。 */
export interface BuildEnvelopeInput {
  releaseId: string;
  createdAt: string;
  device: BenchDevice;
  memoryManualMiB: number | null;
  pageLoad: BenchPageLoad;
  targets?: BenchTargets;
  trials: BenchTrial[];
}

/** 手入力のピークメモリがあれば envelope のメモリ出所は manual、無ければ試行の出所に従う。 */
export function resolveMemorySource(
  trials: readonly BenchTrial[],
  memoryManualMiB: number | null,
): BenchMemorySource {
  if (memoryManualMiB !== null) {
    return "manual";
  }
  return trials.some((trial) => trial.memoryPeakMiB !== null) ? "performance.memory" : null;
}

/** envelope を組み立てる（純粋関数。副作用も DOM 依存も無い）。 */
export function buildEnvelope(input: BuildEnvelopeInput): BenchEnvelope {
  return {
    schemaVersion: BENCH_SCHEMA_VERSION,
    createdAt: input.createdAt,
    releaseId: input.releaseId,
    device: input.device,
    memorySource: resolveMemorySource(input.trials, input.memoryManualMiB),
    memoryManualMiB: input.memoryManualMiB,
    pageLoad: input.pageLoad,
    targets: input.targets ?? BENCH_TARGETS,
    trials: input.trials,
  };
}

/** 検証結果。`valid` が false のとき `errors` に全違反を列挙する。 */
export interface EnvelopeValidation {
  valid: boolean;
  errors: string[];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isNullableNumber(value: unknown): value is number | null {
  return value === null || isFiniteNumber(value);
}

function checkNullableString(
  errors: string[],
  path: string,
  value: unknown,
): void {
  if (value !== null && typeof value !== "string") {
    errors.push(`${path}: string または null である必要があります`);
  }
}

function checkResourceEntry(errors: string[], path: string, value: unknown): void {
  if (!isRecord(value)) {
    errors.push(`${path}: オブジェクトである必要があります`);
    return;
  }
  if (typeof value.name !== "string") {
    errors.push(`${path}.name: string である必要があります`);
  }
  for (const key of ["transferSize", "encodedBodySize", "decodedBodySize", "duration"] as const) {
    if (!isFiniteNumber(value[key])) {
      errors.push(`${path}.${key}: 有限の数値である必要があります`);
    }
  }
  checkNullableString(errors, `${path}.deliveryType`, value.deliveryType);
  if (!isNullableNumber(value.responseStatus)) {
    errors.push(`${path}.responseStatus: 有限の数値または null である必要があります`);
  }
}

function checkResourceEntries(errors: string[], path: string, value: unknown): void {
  if (!Array.isArray(value)) {
    errors.push(`${path}: 配列である必要があります`);
    return;
  }
  value.forEach((entry, index) => {
    checkResourceEntry(errors, `${path}[${String(index)}]`, entry);
  });
}

function checkTargets(errors: string[], path: string, value: unknown): void {
  if (!isRecord(value)) {
    errors.push(`${path}: オブジェクトである必要があります`);
    return;
  }
  for (const key of [
    "searchP95Ms",
    "firstLoadP95Ms",
    "transferBytesMax",
    "memoryMiBMax",
  ] as const) {
    if (!isFiniteNumber(value[key])) {
      errors.push(`${path}.${key}: 有限の数値である必要があります`);
    }
  }
}

function checkPageLoad(errors: string[], path: string, value: unknown): void {
  if (!isRecord(value)) {
    errors.push(`${path}: オブジェクトである必要があります`);
    return;
  }
  if (typeof value.url !== "string") {
    errors.push(`${path}.url: string である必要があります`);
  }
  if (value.navigation !== null) {
    if (!isRecord(value.navigation)) {
      errors.push(`${path}.navigation: オブジェクトまたは null である必要があります`);
    } else {
      for (const key of [
        "startTime",
        "responseEnd",
        "loadEventEnd",
        "domContentLoadedEventEnd",
        "transferSize",
        "encodedBodySize",
        "decodedBodySize",
      ] as const) {
        if (!isFiniteNumber(value.navigation[key])) {
          errors.push(`${path}.navigation.${key}: 有限の数値である必要があります`);
        }
      }
    }
  }
  checkResourceEntries(errors, `${path}.resources`, value.resources);
}

function checkDevice(errors: string[], path: string, value: unknown): void {
  if (!isRecord(value)) {
    errors.push(`${path}: オブジェクトである必要があります`);
    return;
  }
  if (typeof value.ua !== "string") {
    errors.push(`${path}.ua: string である必要があります`);
  }
  if (typeof value.platform !== "string") {
    errors.push(`${path}.platform: string である必要があります`);
  }
  for (const key of ["deviceName", "os", "browser", "network", "note"] as const) {
    checkNullableString(errors, `${path}.${key}`, value[key]);
  }
}

function checkTrial(errors: string[], path: string, value: unknown): void {
  if (!isRecord(value)) {
    errors.push(`${path}: オブジェクトである必要があります`);
    return;
  }
  for (const key of ["patternIndex", "minMinutes", "maxMinutes", "repeat"] as const) {
    if (!isFiniteNumber(value[key])) {
      errors.push(`${path}.${key}: 有限の数値である必要があります`);
    }
  }
  for (const key of ["patternId", "originId"] as const) {
    if (typeof value[key] !== "string") {
      errors.push(`${path}.${key}: string である必要があります`);
    }
  }
  if (!isRecord(value.origin)) {
    errors.push(`${path}.origin: オブジェクトである必要があります`);
  } else {
    for (const key of ["lat", "lon"] as const) {
      if (!isFiniteNumber(value.origin[key])) {
        errors.push(`${path}.origin.${key}: 有限の数値である必要があります`);
      }
    }
  }
  if (value.cache !== "cold" && value.cache !== "warm") {
    errors.push(`${path}.cache: "cold" または "warm" である必要があります`);
  }
  for (const key of [
    "tTransferMs",
    "tSearchMs",
    "tFirstCandidateMs",
    "memoryPeakMiB",
    "candidateCount",
  ] as const) {
    if (!isNullableNumber(value[key])) {
      errors.push(`${path}.${key}: 有限の数値または null である必要があります`);
    }
  }
  if (typeof value.timeout !== "boolean") {
    errors.push(`${path}.timeout: boolean である必要があります`);
  }
  for (const key of ["resultStatus", "reason", "errorCode"] as const) {
    checkNullableString(errors, `${path}.${key}`, value[key]);
  }
  if (
    value.memorySource !== "performance.memory" &&
    value.memorySource !== "manual" &&
    value.memorySource !== null
  ) {
    errors.push(
      `${path}.memorySource: "performance.memory" / "manual" / null のいずれかである必要があります`,
    );
  }
  checkResourceEntries(errors, `${path}.resources`, value.resources);
}

/**
 * 計測結果 envelope の構造検証（依存なしの手書き実装）。
 * JSON Schema ではなく、後段が読む必須フィールドと型だけを検査する。
 */
export function validateEnvelope(value: unknown): EnvelopeValidation {
  const errors: string[] = [];
  if (!isRecord(value)) {
    return { valid: false, errors: ["envelope: オブジェクトである必要があります"] };
  }
  if (value.schemaVersion !== BENCH_SCHEMA_VERSION) {
    errors.push(`schemaVersion: ${String(BENCH_SCHEMA_VERSION)} である必要があります`);
  }
  if (typeof value.createdAt !== "string") {
    errors.push("createdAt: string である必要があります");
  }
  if (typeof value.releaseId !== "string") {
    errors.push("releaseId: string である必要があります");
  }
  if (
    value.memorySource !== "performance.memory" &&
    value.memorySource !== "manual" &&
    value.memorySource !== null
  ) {
    errors.push('memorySource: "performance.memory" / "manual" / null のいずれかである必要があります');
  }
  if (!isNullableNumber(value.memoryManualMiB)) {
    errors.push("memoryManualMiB: 有限の数値または null である必要があります");
  }
  checkDevice(errors, "device", value.device);
  checkPageLoad(errors, "pageLoad", value.pageLoad);
  checkTargets(errors, "targets", value.targets);
  if (!Array.isArray(value.trials)) {
    errors.push("trials: 配列である必要があります");
  } else if (value.trials.length === 0) {
    errors.push("trials: 1 件以上の試行が必要です");
  } else {
    value.trials.forEach((trial, index) => {
      checkTrial(errors, `trials[${String(index)}]`, trial);
    });
  }
  return { valid: errors.length === 0, errors };
}

/** validateEnvelope の型ガード版。 */
export function isBenchEnvelope(value: unknown): value is BenchEnvelope {
  return validateEnvelope(value).valid;
}
