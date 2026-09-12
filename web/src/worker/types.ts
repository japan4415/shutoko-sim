// UI ↔ Web Worker のメッセージ契約（docs/interfaces.md「ブラウザの探索境界」）。
import type { LatLng, SearchResult, SearchRequest } from "../../../crates/routing-wasm/types/index.d";
import type { BenchResourceEntry } from "../bench/envelope";

export type {
  Candidate,
  Duration,
  GeoJsonLineString,
  Handoff,
  LatLng,
  Loop,
  RampInfo,
  SearchResult,
  SearchRequest,
  SnappedOrigin,
  Toll,
} from "../../../crates/routing-wasm/types/index.d";

/**
 * 計測ページ（bench.html）だけが付ける計測フック。通常 UI は付けない。
 * `bench` が無いときの Worker の挙動（取得順・照合・エラー）は一切変わらない。
 */
export interface BenchRequestHook {
  /**
   * 成果物 URL に付けるキャッシュ回避クエリ（`?bench=<nonce>`）。
   * 配信側ルータはクエリを落とすため同一成果物が返り、cold 計測だけができる。
   */
  cacheBust?: string;
}

/** bench Worker（`new Worker(url, { name: "bench" })`）の名前。 */
export const BENCH_WORKER_NAME = "bench";

/** Worker 内で記録した epoch ms（`performance.timeOrigin + performance.now()`）。 */
export interface BenchMarks {
  /** 成果物取得（loadRelease）開始直前。 */
  loadStartEpochMs: number | null;
  /** 成果物取得・照合・WASM init 完了直後（`ready` 送信直前）。 */
  loadEndEpochMs: number | null;
  /** `search` 呼び出し直前。 */
  searchStartEpochMs: number | null;
  /** `search` 呼び出し直後。 */
  searchEndEpochMs: number | null;
}

/** Worker 内の `performance.memory` サンプル（MiB）。取得できない環境では null。 */
export interface BenchMemorySample {
  beforeMiB: number | null;
  afterMiB: number | null;
}

/** `result` 応答に載せる計測値。`msg.bench` があるときだけ埋まる。 */
export interface BenchPayload {
  marks: BenchMarks;
  /** 成果物 URL（manifest / engine / graph / wasm / glue）の Resource Timing エントリ。 */
  resources: BenchResourceEntry[];
  memory: BenchMemorySample | null;
}

/** UI → Worker の検索依頼エンベロープ。`type` は SearchRequest JSON には含めない。 */
export interface UiSearchMessage {
  type: "search";
  requestId: string;
  releaseId: string;
  pricingAt: string;
  origin?: LatLng;
  originNodeId?: string;
  minMinutes: number;
  maxMinutes: number;
  vehicleProfile: string;
  /** 計測ページ専用の計測フック（通常 UI は未指定）。 */
  bench?: BenchRequestHook;
}

/** Worker → UI の応答。 */
export type WorkerResponse =
  | { type: "ready"; releaseId: string }
  | { type: "result"; requestId: string; result: SearchResult; bench?: BenchPayload }
  | { type: "error"; requestId: string; code: string; message?: string };
