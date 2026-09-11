// 探索用 Web Worker 本体。パイプラインのロジックは pipeline.ts の純粋関数に置き、
// ここはメッセージ配線のみを行う。
// 10 秒タイムアウトと terminate は UI 側（第 2 段）の責務のためここでは持たない。
import { ARTIFACT_HASHES } from "./artifact-hashes";
import {
  buildSearchRequest,
  loadRelease,
  parseSearchResult,
  parseWasmError,
  PipelineError,
  type LoadedRelease,
} from "./pipeline";
import type { UiSearchMessage, WorkerResponse } from "./types";

const DEFAULT_RELEASE_ID = "c1-real-v1";

const ctx = self as unknown as {
  postMessage(message: WorkerResponse): void;
  onmessage: ((event: MessageEvent) => void) | null;
};

let loaded: { releaseId: string; state: LoadedRelease } | null = null;
let loadPromise: Promise<{ releaseId: string; state: LoadedRelease }> | null = null;
let readySent = false;

function post(message: WorkerResponse): void {
  ctx.postMessage(message);
}

function ensureLoaded(releaseId: string): Promise<{ releaseId: string; state: LoadedRelease }> {
  if (loaded !== null && loaded.releaseId === releaseId) {
    return Promise.resolve(loaded);
  }
  const hashes = ARTIFACT_HASHES[releaseId];
  if (hashes === undefined) {
    return Promise.reject(
      new PipelineError("ARTIFACT_MISMATCH", `未知の releaseId です: ${releaseId}`),
    );
  }
  // 取得系 fetch は共通の AbortController でまとめられるよう signal を渡す。
  const controller = new AbortController();
  loadPromise = loadRelease(fetch, releaseId, hashes, undefined, controller.signal)
    .then((state) => {
      loaded = { releaseId, state };
      return loaded;
    })
    .catch((err: unknown) => {
      loadPromise = null; // 失敗時は再取得できるようにする
      throw err;
    });
  return loadPromise;
}

async function handleSearch(msg: UiSearchMessage): Promise<void> {
  const releaseId = msg.releaseId ?? DEFAULT_RELEASE_ID;
  try {
    const { state } = await ensureLoaded(releaseId);
    if (!readySent) {
      readySent = true;
      post({ type: "ready", releaseId });
    }
    const requestJson = JSON.stringify(buildSearchRequest(msg));
    const resultJson = state.search(state.graphJson, requestJson, "{}");
    post({ type: "result", requestId: msg.requestId, result: parseSearchResult(resultJson) });
  } catch (err) {
    if (err instanceof PipelineError) {
      post({ type: "error", requestId: msg.requestId, code: err.code, message: err.message });
      return;
    }
    const { code, message } = parseWasmError(err);
    post({ type: "error", requestId: msg.requestId, code, message });
  }
}

ctx.onmessage = (event: MessageEvent) => {
  const msg = event.data as UiSearchMessage | undefined;
  if (msg !== undefined && msg.type === "search") {
    void handleSearch(msg);
  }
};

//起動時に既定リリースを先読みし、初期化完了で ready を送る。
const bootReleaseId = DEFAULT_RELEASE_ID;
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
