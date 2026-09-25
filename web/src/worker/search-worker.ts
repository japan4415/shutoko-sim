// 探索用 Web Worker 本体。パイプラインのロジックは pipeline.ts の純粋関数に置き、
// ここはメッセージ配線のみを行う。
// 10 秒タイムアウトと terminate は UI 側（第 2 段）の責務のためここでは持たない。
//
// 計測フック（`msg.bench`）は bench ページ専用。通常 UI は `bench` を付けないため、
// 取得順・照合・エラーの挙動は一切変わらない。
import {
  buildResultResponse,
  buildSearchRequest,
  parseSearchResult,
  parseWasmError,
  PipelineError,
  ReleaseStore,
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

import { DEFAULT_RELEASE_ID } from "./artifact-hashes";

export { DEFAULT_RELEASE_ID };

/** 計測中の試行の状態。bench Worker では 1 メッセージ = 1 試行。 */
interface BenchState {
  marks: BenchMarks;
  memory: BenchMemorySample | null;
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

export interface SearchWorkerHandlerOptions {
  post: (message: WorkerResponse) => void;
  store?: ReleaseStore;
  isBench?: boolean;
}

export function createSearchWorkerHandler(options: SearchWorkerHandlerOptions) {
  const { post, isBench = false } = options;
  let benchState: BenchState | null = null;
  let readySent = false;

  const store =
    options.store ??
    new ReleaseStore({
      onLoadStart: () => {
        if (benchState !== null) {
          benchState.marks.loadStartEpochMs = epochNow();
        }
      },
      onLoadEnd: () => {
        if (benchState !== null) {
          benchState.marks.loadEndEpochMs = epochNow();
        }
      },
    });

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
    let state: LoadedRelease | null = null;
    try {
      state = await store.acquire(releaseId, msg.bench?.cacheBust);
      if (!readySent) {
        readySent = true;
        post({ type: "ready", releaseId });
      }
      const requestJson = JSON.stringify(buildSearchRequest(msg));
      if (benchState !== null) {
        benchState.memory = { beforeMiB: sampleHeapMiB(), afterMiB: null };
        benchState.marks.searchStartEpochMs = epochNow();
      }
      const resultJson = state.searchPrepared(requestJson);
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
      const result = await parseSearchResult(resultJson);
      post(buildResultResponse(msg.requestId, result, bench));
    } catch (err) {
      if (err instanceof PipelineError) {
        post({ type: "error", requestId: msg.requestId, code: err.code, message: err.message });
        return;
      }
      const { code, message } = parseWasmError(err);
      post({ type: "error", requestId: msg.requestId, code, message });
    } finally {
      if (state !== null) {
        state.release();
      }
      benchState = null;
    }
  }

  function boot(): void {
    if (!isBench) {
      store
        .acquire(DEFAULT_RELEASE_ID)
        .then((state) => {
          state.release();
          if (!readySent) {
            readySent = true;
            post({ type: "ready", releaseId: DEFAULT_RELEASE_ID });
          }
        })
        .catch((err: unknown) => {
          const { code, message } =
            err instanceof PipelineError
              ? { code: err.code, message: err.message }
              : parseWasmError(err);
          post({ type: "error", requestId: "", code, message });
        });
    }
  }

  return {
    handleSearch,
    boot,
    store,
  };
}

const isWorkerScope =
  typeof self !== "undefined" &&
  typeof (self as unknown as { postMessage?: unknown }).postMessage === "function";

if (isWorkerScope) {
  const ctx = self as unknown as {
    name?: string;
    postMessage(message: WorkerResponse): void;
    onmessage: ((event: MessageEvent) => void) | null;
  };
  const isBenchWorker = ctx.name === BENCH_WORKER_NAME;
  const handler = createSearchWorkerHandler({
    post: (msg) => ctx.postMessage(msg),
    isBench: isBenchWorker,
  });
  ctx.onmessage = (event: MessageEvent) => {
    const msg = event.data as UiSearchMessage | undefined;
    if (msg !== undefined && msg.type === "search") {
      void handler.handleSearch(msg);
    }
  };
  handler.boot();
}
