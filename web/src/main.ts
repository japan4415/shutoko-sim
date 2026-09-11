// 最小 UI の配線本体。描画ロジックの純粋部分は src/ui/model.ts に置く。
// - 探索押下時に pricingAt を 1 回取得し、requestId を採番して Worker へ search を送る
// - in-flight の requestId 以外の応答は無視し、再押下時は旧を無効化してから新を送る
// - 10 秒で setTimeout → worker.terminate() → TIMEOUT 文言 → 次回検索時に Worker を再生成し、
//   ready を待ってから送信する
import {
  PRESET_KANDABASHI,
  RELEASE_ID,
  SEARCH_TIMEOUT_MS,
  VEHICLE_PROFILE,
  errorMessage,
  statusMessage,
  toCardModel,
  validateInputs,
} from "./ui/model";
import type { UiSearchMessage, WorkerResponse } from "./worker/types";

const el = {
  preset: mustGet<HTMLSelectElement>("origin-preset"),
  lat: mustGet<HTMLInputElement>("lat"),
  lon: mustGet<HTMLInputElement>("lon"),
  minMinutes: mustGet<HTMLInputElement>("min-minutes"),
  maxMinutes: mustGet<HTMLInputElement>("max-minutes"),
  search: mustGet<HTMLButtonElement>("search-btn"),
  inputErrors: mustGet<HTMLOListElement>("input-errors"),
  status: mustGet<HTMLParagraphElement>("status"),
  results: mustGet<HTMLDivElement>("results"),
};

function mustGet<T extends Element>(id: string): T {
  const node = document.getElementById(id);
  if (node === null) {
    throw new Error(`missing element: ${id}`);
  }
  return node as unknown as T;
}

let worker: Worker | null = null;
let workerReady = false;
// ブート初期化が失敗済みなら true。ready は来ないので送信を保留せず即再試行させる。
let bootFailed = false;
// 一度でも探索を開始したか。遅れて届いた ready で結果・探索中の文言を上書きしないためのフラグ。
let hasSearched = false;
let inflightRequestId: string | null = null;
let pending: UiSearchMessage | null = null;
let timeoutId: ReturnType<typeof setTimeout> | null = null;
let requestCounter = 0;

function createWorker(): Worker {
  const next = new Worker(new URL("./worker/search-worker.ts", import.meta.url), {
    type: "module",
  });
  workerReady = false;
  bootFailed = false;
  next.onmessage = (event: MessageEvent<WorkerResponse>) => handleWorkerMessage(event.data);
  worker = next;
  return next;
}

function clearTimer(): void {
  if (timeoutId !== null) {
    clearTimeout(timeoutId);
    timeoutId = null;
  }
}

function stopWorker(): void {
  clearTimer();
  if (worker !== null) {
    worker.terminate();
    worker = null;
  }
  workerReady = false;
  inflightRequestId = null;
}

function sendMessage(msg: UiSearchMessage): void {
  if (worker === null) {
    worker = createWorker();
  }
  // ready（または初期化失敗）を待ってから送信するため保持する。
  pending = msg;
  if (workerReady || bootFailed) {
    flushPending();
  }
}

function flushPending(): void {
  if (pending !== null && worker !== null && (workerReady || bootFailed)) {
    const msg = pending;
    pending = null;
    worker.postMessage(msg);
  }
}

function handleWorkerMessage(msg: WorkerResponse): void {
  switch (msg.type) {
    case "ready": {
      workerReady = true;
      bootFailed = false;
      // 探索開始後に届いた ready は「探索中…」や結果の文言を上書きしない。
      if (!hasSearched) {
        setStatus("準備完了");
      }
      flushPending();
      return;
    }
    case "result": {
      if (msg.requestId !== inflightRequestId) {
        return; // 古い requestId の応答は無視
      }
      clearTimer();
      inflightRequestId = null;
      renderResult(msg.result);
      return;
    }
    case "error": {
      if (msg.requestId === "") {
        // ブート時の初期化失敗。以後の送信は保留せず再試行させる。
        bootFailed = true;
        setStatus(errorMessage(msg.code));
        flushPending();
        return;
      }
      if (msg.requestId !== inflightRequestId) {
        return; // 古い requestId の応答は無視
      }
      clearTimer();
      inflightRequestId = null;
      el.results.replaceChildren();
      setStatus(errorMessage(msg.code));
      return;
    }
  }
}

function setStatus(text: string): void {
  el.status.textContent = text;
}

function renderResult(result: import("./worker/types").SearchResult): void {
  const min = Number(el.minMinutes.value);
  const max = Number(el.maxMinutes.value);
  setStatus(statusMessage(result, min, max));
  const fragment = document.createDocumentFragment();
  for (const candidate of result.candidates.slice(0, 3)) {
    fragment.appendChild(renderCard(toCardModel(candidate)));
  }
  el.results.replaceChildren(fragment);
}

function renderCard(model: import("./ui/model").CardModel): HTMLElement {
  const card = document.createElement("article");
  card.className = "card";
  card.dataset.mapsUrl = model.mapsUrl;

  const items: string[] = [
    `総所要時間: 約${String(model.planMinutes)}分（一般道での帰着まで含み、休憩は含まない）`,
    model.toll,
    `経路: ${model.route}`,
    `通過路線: ${model.roadNames.join(" / ")}`,
    model.chargedSection,
  ];
  for (const text of items) {
    const p = document.createElement("p");
    p.textContent = text;
    card.appendChild(p);
  }
  if (model.warnings.length > 0) {
    const ul = document.createElement("ul");
    for (const warning of model.warnings) {
      const li = document.createElement("li");
      li.textContent = warning;
      ul.appendChild(li);
    }
    card.appendChild(ul);
  }

  const button = document.createElement("button");
  button.textContent = "出発する（Google マップを開く）";
  button.addEventListener("click", () => {
    window.open(model.mapsUrl, "_blank", "noopener");
  });
  card.appendChild(button);
  return card;
}

function invalidateResults(options?: { silent?: boolean }): void {
  // 条件変更時は前回の候補と出発リンクを消す（docs/requirements.md:30）。
  el.results.replaceChildren();
  stopWorker();
  if (options?.silent !== true) {
    setStatus("条件が変更されました。探索ボタンで再検索してください。");
  }
}

function readOrigin(): { lat: number; lon: number } {
  return { lat: Number(el.lat.value), lon: Number(el.lon.value) };
}

function startSearch(): void {
  const errors = validateInputs(
    el.lat.value,
    el.lon.value,
    el.minMinutes.value,
    el.maxMinutes.value,
  );
  const listItems = errors.map((text) => {
    const li = document.createElement("li");
    li.textContent = text;
    return li;
  });
  el.inputErrors.replaceChildren(...listItems);
  if (errors.length > 0) {
    return; // 入力不備では検索しない
  }

  requestCounter += 1;
  const requestId = `request-${String(requestCounter)}`;
  const msg: UiSearchMessage = {
    type: "search",
    requestId,
    releaseId: RELEASE_ID,
    pricingAt: new Date().toISOString(), // 押下時に 1 回だけ取得
    origin: readOrigin(),
    minMinutes: Number(el.minMinutes.value),
    maxMinutes: Number(el.maxMinutes.value),
    vehicleProfile: VEHICLE_PROFILE,
  };

  // 二重送信防止: 旧 in-flight を無効化してから新 requestId で送る。
  clearTimer();
  inflightRequestId = requestId;
  hasSearched = true;
  setStatus("探索中…");
  sendMessage(msg);

  timeoutId = setTimeout(() => {
    if (inflightRequestId !== requestId) {
      return;
    }
    stopWorker();
    pending = null;
    el.results.replaceChildren();
    setStatus(errorMessage("TIMEOUT"));
  }, SEARCH_TIMEOUT_MS);
}

function applyPreset(options?: { silent?: boolean }): void {
  if (el.preset.value === "kandabashi") {
    el.lat.value = String(PRESET_KANDABASHI.lat);
    el.lon.value = String(PRESET_KANDABASHI.lon);
  }
  invalidateResults({ silent: options?.silent === true });
}

// --- 初期化 ---
el.preset.addEventListener("change", () => {
  applyPreset();
});
for (const input of [el.lat, el.lon, el.minMinutes, el.maxMinutes]) {
  input.addEventListener("change", () => {
    invalidateResults();
  });
}
el.search.addEventListener("click", startSearch);

el.preset.value = "kandabashi";
// ブート時はまだ検索していないため「条件が変更されました…」を出さない（design-review-001 F1）。
applyPreset({ silent: true });
setStatus("成果物を読み込み中…");
createWorker(); // ブート: ready が来たらステータスへ反映される
