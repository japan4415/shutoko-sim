// 探索用 Web Worker 本体。パイプラインのロジックは pipeline.ts の純粋関数に置き、
// ここはメッセージ配線のみを行う。
// 10 秒タイムアウトと terminate は UI 側（第 2 段）の責務のためここでは持たない。
//
// 計測フック（`msg.bench`）は bench ページ専用。通常 UI は `bench` を付けないため、
// 取得順・照合・エラーの挙動は一切変わらない。
import { KNOWN_RELEASES } from "./artifact-hashes";
import {
  buildResultResponse,
  buildSearchRequest,
  loadRelease,
  parseSearchResult,
  parseWasmError,
  PipelineError,
  type LoadedRelease,
} from "./pipeline";
import {
  BENCH_WORKER_NAME,
  type BenchMarks,
  type BenchMemorySample,
  type BenchPayload,
  type UiSearchMessage,
  type WorkerResponse,
} from "./types";

const DEFAULT_RELEASE_ID = "c1-real-v1";

const ctx = self as unknown as {
  name?: string;
  postMessage(message: WorkerResponse): void;
  onmessage: ((event: MessageEvent) => void) | null;
};

/**
 * bench ページが `new Worker(url, { name: "bench" })` で生成した Worker か。
 *
 * bench Worker は起動時の先読みを行わず、最初の `search` メッセージで初めて
 * 成果物を取得する。先読みが先に走ると `?bench=<nonce>` 付きの取得が
 * 先読み済みリリースへ相乗りしてしまい、cold 計測が成立しないため。
 * 通常 UI が生成する Worker の `name` は空なので、この分岐は通常経路に影響しない。
 */
const isBenchWorker = ctx.name === BENCH_WORKER_NAME;

let loaded: { releaseId: string; state: LoadedRelease } | null = null;
let loadPromise: Promise<{ releaseId: string; state: LoadedRelease }> | null = null;
// 進行中ロードの対象 releaseId。loadPromise を安全に再利用するために保持する。
let loadingReleaseId: string | null = null;
let readySent = false;

/** 計測中の試行の状態。bench Worker では 1 メッセージ = 1 試行。 */
interface BenchState {
  marks: BenchMarks;
  memory: BenchMemorySample | null;
}

let benchState: BenchState | null = null;

function post(message: WorkerResponse): void {
  ctx.postMessage(message);
}

/** epoch ms（`performance.timeOrigin + performance.now()`）。スレッドをまたいで比較できる。 */
function epochNow(): number {
  return performance.timeOrigin + performance.now();
}

/**
 * `performance.memory`（Chromium のみ）から JS ヒープ使用量を MiB で返す。
 * 未実装・例外・非有限値のときは null（計測不能として記録する）。
 */
function sampleHeapMiB(): number | null {
  try {
    const perf = performance as Performance & { memory?: { usedJSHeapSize?: number } };
    const used = perf.memory?.usedJSHeapSize;
    if (typeof used !== "number" || !Number.isFinite(used)) {
      return null;
    }
    return used / (1024 * 1024);
  } catch {
    return null;
  }
}

/**
 * この試行で取得した成果物（manifest / engine / graph / wasm / glue）の
 * Resource Timing エントリを返す。Worker 内の fetch はメイン document の
 * Resource Timing に出ないため、Worker 側で収集して応答に載せる。
 */
function collectResources(releaseId: string): BenchPayload["resources"] {
  const prefix = `/releases/${encodeURIComponent(releaseId)}/`;
  return performance
    .getEntriesByType("resource")
    .filter((entry) => entry.name.includes(prefix))
    .map((entry) => {
      const timing = entry as PerformanceResourceTiming & {
        deliveryType?: unknown;
        responseStatus?: unknown;
      };
      return {
        name: timing.name,
        transferSize: timing.transferSize,
        encodedBodySize: timing.encodedBodySize,
        decodedBodySize: timing.decodedBodySize,
        deliveryType: typeof timing.deliveryType === "string" ? timing.deliveryType : null,
        responseStatus: typeof timing.responseStatus === "number" ? timing.responseStatus : null,
        duration: timing.duration,
      };
    });
}

function ensureLoaded(
  releaseId: string,
  cacheBust?: string,
): Promise<{ releaseId: string; state: LoadedRelease }> {
  if (loaded !== null && loaded.releaseId === releaseId) {
    return Promise.resolve(loaded);
  }
  if (!KNOWN_RELEASES.includes(releaseId)) {
    return Promise.reject(
      new PipelineError("ARTIFACT_MISMATCH", `未知の releaseId です: ${releaseId}`),
    );
  }
  // 進行中のロードがあれば共有する（同一成果物の二重取得と WASM 二重 init を防ぐ）。
  if (loadPromise !== null && loadingReleaseId === releaseId) {
    return loadPromise;
  }
  loadingReleaseId = releaseId;
  // 取得系 fetch は共通の AbortController でまとめられるよう signal を渡す。
  const controller = new AbortController();
  if (benchState !== null) {
    benchState.marks.loadStartEpochMs = epochNow();
  }
  loadPromise = loadRelease(fetch, releaseId, undefined, controller.signal, { cacheBust })
    .then((state) => {
      loaded = { releaseId, state };
      loadingReleaseId = null;
      if (benchState !== null) {
        benchState.marks.loadEndEpochMs = epochNow();
      }
      return loaded;
    })
    .catch((err: unknown) => {
      loadPromise = null; // 失敗時は再取得できるようにする
      loadingReleaseId = null;
      throw err;
    });
  return loadPromise;
}

async function handleSearch(msg: UiSearchMessage): Promise<void> {
  const releaseId = msg.releaseId ?? DEFAULT_RELEASE_ID;
  const measuring = msg.bench !== undefined;
  if (measuring) {
    benchState = {
      marks: {
        loadStartEpochMs: null,
        loadEndEpochMs: null,
        searchStartEpochMs: null,
        searchEndEpochMs: null,
      },
      memory: null,
    };
  }
  try {
    const { state } = await ensureLoaded(releaseId, msg.bench?.cacheBust);
    if (!readySent) {
      readySent = true;
      post({ type: "ready", releaseId });
    }
    const requestJson = JSON.stringify(buildSearchRequest(msg));
    if (benchState !== null) {
      benchState.memory = { beforeMiB: sampleHeapMiB(), afterMiB: null };
      benchState.marks.searchStartEpochMs = epochNow();
    }
    const resultJson = state.search(state.graphJson, requestJson, "{}");
    if (benchState !== null) {
      benchState.marks.searchEndEpochMs = epochNow();
      if (benchState.memory !== null) {
        benchState.memory.afterMiB = sampleHeapMiB();
      }
    }
    const bench: BenchPayload | undefined =
      benchState === null
        ? undefined
        : {
            marks: benchState.marks,
            resources: collectResources(releaseId),
            memory: benchState.memory,
          };
    post(buildResultResponse(msg.requestId, parseSearchResult(resultJson), bench));
  } catch (err) {
    if (err instanceof PipelineError) {
      post({ type: "error", requestId: msg.requestId, code: err.code, message: err.message });
      return;
    }
    const { code, message } = parseWasmError(err);
    post({ type: "error", requestId: msg.requestId, code, message });
  } finally {
    benchState = null;
  }
}

ctx.onmessage = (event: MessageEvent) => {
  const msg = event.data as UiSearchMessage | undefined;
  if (msg !== undefined && msg.type === "search") {
    void handleSearch(msg);
  }
};

// 起動時に既定リリースを先読みし、初期化完了で ready を送る。
// bench Worker は先読みせず、`search` メッセージ駆動で取得する（cold 計測のため）。
const bootReleaseId = DEFAULT_RELEASE_ID;
if (!isBenchWorker) {
  ensureLoaded(bootReleaseId)
    .then(() => {
      if (!readySent) {
        readySent = true;
        post({ type: "ready", releaseId: bootReleaseId });
      }
    })
    .catch((err: unknown) => {
      const { code, message } =
        err instanceof PipelineError ? { code: err.code, message: err.message } : parseWasmError(err);
      post({ type: "error", requestId: "", code, message });
    });
}
