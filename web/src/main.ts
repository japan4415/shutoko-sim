// 製品 UI の配線本体。描画ロジックの純粋部分は src/ui/model.ts、地図は src/map/、
// 外部 I/O は src/geocode.ts / src/geolocation.ts に置く。
// - 探索押下時に pricingAt を 1 回取得し、requestId を採番して Worker へ search を送る
// - in-flight の requestId 以外の応答は無視し、再押下時は旧を無効化してから新を送る
// - 10 秒で setTimeout → worker.terminate() → TIMEOUT 文言 → 次回検索時に Worker を再生成する
// - 候補カードと地図は同じ候補 ID に紐付け、条件変更で選択と出発リンクを失効させる
import {
  MAX_PRODUCT_MINUTES,
  PRESET_KANDABASHI,
  RELEASE_ID,
  SEARCH_TIMEOUT_MS,
  SUPPORTED_AREA_TEXT,
  VEHICLE_PROFILE,
  canRankByPrice,
  classifyNoCandidates,
  coordinateLabel,
  errorMessage,
  formatRank,
  geocodeErrorMessage,
  geolocationErrorMessage,
  lowerMinClickOutcome,
  recommendedLabel,
  statusMessage,
  timeBreakdownText,
  timeWindowActions,
  toCardModel,
  validateAddressQuery,
  validateInputFields,
} from "./ui/model";
import type { CardModel } from "./ui/model";
import { GeocodeError, geocode } from "./geocode";
import type { GeocodeCandidate } from "./geocode";
import { getCurrentPosition } from "./geolocation";
import { createMapView } from "./map/map-view";
import type { MapView } from "./map/map-view";
import "leaflet/dist/leaflet.css";
import type { Candidate, LatLng, SearchResult, UiSearchMessage, WorkerResponse } from "./worker/types";
import {
  filterRamps,
  formatCountInfo,
  formatDirection,
  formatRoute,
  loadRampsDataset,
  validateExplicitSearch,
  duplicateOperationMessage,
  type ExplicitSearchCondition,
  type RampItem,
  type RampsDataset,
} from "./ui/ramps";

const el = {
  addressQuery: mustGet<HTMLInputElement>("address-query"),
  addressSearch: mustGet<HTMLButtonElement>("address-search-btn"),
  addressFeedback: mustGet<HTMLDivElement>("address-feedback"),
  addressCandidatesField: mustGet<HTMLFieldSetElement>("address-candidates-field"),
  addressCandidates: mustGet<HTMLDivElement>("address-candidates"),
  geolocate: mustGet<HTMLButtonElement>("geolocate-btn"),
  preset: mustGet<HTMLSelectElement>("origin-preset"),
  lat: mustGet<HTMLInputElement>("lat"),
  lon: mustGet<HTMLInputElement>("lon"),
  originSummary: mustGet<HTMLParagraphElement>("origin-summary"),
  minMinutes: mustGet<HTMLInputElement>("min-minutes"),
  maxMinutes: mustGet<HTMLInputElement>("max-minutes"),
  search: mustGet<HTMLButtonElement>("search-btn"),
  cancel: mustGet<HTMLButtonElement>("cancel-btn"),
  duplicateWarning: mustGet<HTMLDivElement>("duplicate-warning"),
  mapPick: mustGet<HTMLButtonElement>("map-pick-btn"),
  mapPickPanel: mustGet<HTMLDivElement>("map-pick-panel"),
  mapPickStatus: mustGet<HTMLParagraphElement>("map-pick-status"),
  mapPickConfirm: mustGet<HTMLButtonElement>("map-pick-confirm"),
  mapPickCancel: mustGet<HTMLButtonElement>("map-pick-cancel"),
  inputErrors: mustGet<HTMLOListElement>("input-errors"),
  status: mustGet<HTMLParagraphElement>("status"),
  map: mustGet<HTMLDivElement>("map"),
  mapFallback: mustGet<HTMLParagraphElement>("map-fallback"),
  mapRetry: mustGet<HTMLButtonElement>("map-retry-btn"),
  recovery: mustGet<HTMLDivElement>("recovery-actions"),
  results: mustGet<HTMLDivElement>("results"),
  explicitOdSection: mustGet<HTMLElement>("explicit-od-section"),
  explicitOriginCurrent: mustGet<HTMLSpanElement>("explicit-origin-current"),
  rampsLoadingStatus: mustGet<HTMLDivElement>("ramps-loading-status"),
  rampsErrorPanel: mustGet<HTMLDivElement>("ramps-error-panel"),
  rampsErrorMessage: mustGet<HTMLParagraphElement>("ramps-error-message"),
  rampsRetryBtn: mustGet<HTMLButtonElement>("ramps-retry-btn"),
  rampPickersContainer: mustGet<HTMLDivElement>("ramp-pickers-container"),
  entryRampSearch: mustGet<HTMLInputElement>("entry-ramp-search"),
  entryClearSearchBtn: mustGet<HTMLButtonElement>("entry-clear-search-btn"),
  entryCountInfo: mustGet<HTMLParagraphElement>("entry-count-info"),
  entrySelectedBadge: mustGet<HTMLDivElement>("entry-selected-badge"),
  entrySelectedName: mustGet<HTMLSpanElement>("entry-selected-name"),
  entryDeselectBtn: mustGet<HTMLButtonElement>("entry-deselect-btn"),
  entryZeroMessage: mustGet<HTMLDivElement>("entry-zero-message"),
  entryResetFilterBtn: mustGet<HTMLButtonElement>("entry-reset-filter-btn"),
  entryRampList: mustGet<HTMLDivElement>("entry-ramp-list"),
  exitRampSearch: mustGet<HTMLInputElement>("exit-ramp-search"),
  exitClearSearchBtn: mustGet<HTMLButtonElement>("exit-clear-search-btn"),
  exitCountInfo: mustGet<HTMLParagraphElement>("exit-count-info"),
  exitSelectedBadge: mustGet<HTMLDivElement>("exit-selected-badge"),
  exitSelectedName: mustGet<HTMLSpanElement>("exit-selected-name"),
  exitDeselectBtn: mustGet<HTMLButtonElement>("exit-deselect-btn"),
  exitZeroMessage: mustGet<HTMLDivElement>("exit-zero-message"),
  exitResetFilterBtn: mustGet<HTMLButtonElement>("exit-reset-filter-btn"),
  exitRampList: mustGet<HTMLDivElement>("exit-ramp-list"),
  explicitSelectionStatus: mustGet<HTMLDivElement>("explicit-selection-status"),
};

function mustGet<T extends Element>(id: string): T {
  const node = document.getElementById(id);
  if (node === null) {
    throw new Error(`missing element: ${id}`);
  }
  return node as unknown as T;
}

/** 入力欄を DOM 順に並べたもの。エラー表示・aria 付与・フォーカス移動の走査順。 */
const INPUTS = [el.lat, el.lon, el.minMinutes, el.maxMinutes];

/** エラー文言の li に振る id の接頭辞。欄の aria-describedby から参照する。 */
const ERROR_ID_PREFIX = "input-error-";

/**
 * 欄に静的に紐づく説明文の id。index.html が付けた aria-describedby を起動時に控え、
 * エラー id と空白区切りで併記する（例: 時間欄の #time-note）。
 */
const STATIC_DESCRIBEDBY = new Map<HTMLInputElement, string[]>(
  INPUTS.map((input) => [
    input,
    (input.getAttribute("aria-describedby") ?? "").split(" ").filter((id) => id !== ""),
  ]),
);

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

// 確定済み出発地点。探索条件に使う唯一の座標源。
let origin: LatLng | null = null;
// 現在表示中の探索結果（地図とカードの選択状態を同期するため保持）。
let currentResult: SearchResult | null = null;
let selectedCandidateId: string | null = null;
// 地図がタイル取得に失敗したか。失敗中は出発を無効化する（docs/requirements.md:44）。
let mapFailed = false;
let mapView: MapView | null = null;
// 位置情報が拒否られたか。一旦拒否されたら許可要求を繰り返さない。
let geolocationDenied = false;
// 進行中の現在地取得を識別する世代番号。地図・住所・座標などで出発地点が
// 後から確定したら加算し、遅れて成功した古い取得結果を捨てる（住所検索の
// addressRequestSeq とは独立のカウンタ）。
let geolocationRequestSeq = 0;
// 地図タップで出発地点を選ぶモード。誤タップで確定しないよう pending を経由する。
let pickMode = false;
// 地図タップで置いた候補地点（確定前）。住所は逆ジオコーディングしない。
let pendingPick: LatLng | null = null;
// 復帰パネルの由来。'result' は探索結果の候補ゼロに基づく導線で、条件変更で失効させる。
// 'error' は結果取得前の成果物不一致・通信失敗の再読み込み案内で、条件を変えても残す。
let recoveryOrigin: "result" | "error" | null = null;

// --- ランプ明示指定モードの状態 ---
type SearchMode = "coord" | "explicit";
let searchMode: SearchMode = "coord";
let rampsDataset: RampsDataset | null = null;
let selectedEntryRampId: string | null = null;
let selectedExitRampId: string | null = null;
let entryFilterQuery = "";
let exitFilterQuery = "";
let lastSuccessCondition: ExplicitSearchCondition | null = null;
let isSearching = false;
let rampsLoading = false;
let rampsLoadGeneration = 0;
let rampsLoadAbort: AbortController | null = null;
let entryRenderFrame: number | null = null;
let exitRenderFrame: number | null = null;

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
        setStatus("準備完了。出発地点と時間を指定して検索してください。");
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
      setSearching(false);
      if (searchMode === "explicit" && origin !== null) {
        lastSuccessCondition = {
          entryRampId: selectedEntryRampId,
          exitRampId: selectedExitRampId,
          minMinutes: Number(el.minMinutes.value),
          maxMinutes: Number(el.maxMinutes.value),
          origin,
        };
      }
      renderResult(msg.result);
      return;
    }
    case "error": {
      if (msg.requestId === "") {
        // ブート時の初期化失敗。以後の送信は保留せず再試行させる。
        bootFailed = true;
        setStatus(errorMessage(msg.code));
        showReloadRecovery(msg.code);
        flushPending();
        return;
      }
      if (msg.requestId !== inflightRequestId) {
        return; // 古い requestId の応答は無視
      }
      clearTimer();
      inflightRequestId = null;
      setSearching(false);
      clearResults();
      setStatus(errorMessage(msg.code));
      showReloadRecovery(msg.code);
      return;
    }
  }
}

// --- ステータス・復帰導線 ---

function setStatus(text: string): void {
  el.status.textContent = text;
}

/**
 * 復帰導線を可視化して、その主操作へフォーカスを移す。
 * 復帰パネルは地図の下にありモバイルで画面外になりやすいため、
 * 表示時にスクロールして操作位置を見えるようにする（design F5）。
 */
function revealRecovery(focusTarget: HTMLElement | null): void {
  el.recovery.hidden = false;
  if (focusTarget !== null) {
    focusTarget.focus({ preventScroll: true });
  }
  el.recovery.scrollIntoView({ block: "nearest" });
}

function clearRecovery(): void {
  el.recovery.hidden = true;
  el.recovery.replaceChildren();
  recoveryOrigin = null;
}

/**
 * 条件変更で失効させるべき復帰導線だけを消す。
 * `result`（探索結果の候補ゼロに基づく導線）は前回結果が無効になった時点で消すが、
 * `error`（結果取得前の成果物不一致・通信失敗）の再読み込み案内は、時間や出発地点を
 * 変えても有効なままなので消さない（review R3-01 / docs/requirements.md:43,52）。
 */
function clearResultRecovery(): void {
  if (recoveryOrigin === "result") {
    clearRecovery();
  }
}

/**
 * 再読み込みでしか復帰できない成果物不整合・通信失敗・探索結果契約不一致の案内
 * （docs/requirements.md:43）。部分データでは探索しない。
 * RESULT_CONTRACT_MISMATCH は「成果物（エンジン成果物）が読めない」のではなく、探索結果 JSON が
 * 実行中エンジンの形式と合わない失敗なので、失敗種別を正しく名指しする（design D3-2）。
 */
function showReloadRecovery(code: string): void {
  if (
    code !== "ARTIFACT_MISMATCH" &&
    code !== "FETCH_FAILED" &&
    code !== "WASM_ERROR" &&
    code !== "RESULT_CONTRACT_MISMATCH"
  ) {
    return;
  }
  const p = document.createElement("p");
  p.textContent =
    code === "RESULT_CONTRACT_MISMATCH"
      ? "探索結果の形式が実行中のエンジンと一致しません。再読み込みしてください。部分データでは探索しません。"
      : "成果物を読み込めませんでした。部分データでは探索しません。";
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "再読み込み";
  button.addEventListener("click", () => {
    window.location.reload();
  });
  recoveryOrigin = "error";
  el.recovery.replaceChildren(p, wrapActions(button));
  revealRecovery(button);
}

/**
 * 時間枠不一致からの復帰。実際に値が変わる操作（最小時間を下げる / 上限を広げる）だけを並べる。
 * 上限が製品上限 240 分のときは「上限を広げる」を出さない（値が変わらないため）。
 * 時間を変えても解決を証明できないとき（打切り等）は、値の変更を成功として告げず出発地点の
 * 見直しを案内する（review R2-01）。
 * 原因に対応する操作（最小時間を下げる）を先頭・フォーカス対象にし、#status の主導線と
 * 一致させる（design D3-1）。上限拡大は後段に残す。
 */
function showTimeWindowRecovery(result: SearchResult, minMinutes: number, maxMinutes: number): void {
  const actions = timeWindowActions(result, minMinutes, maxMinutes);
  if (actions.lowerMinMinutes === null && actions.widenMaxMinutes === null) {
    showAreaRecovery(
      "指定時間枠に収まる周回候補が見つかりませんでした。時間枠を広げても見つかるとは限らないため、出発地点や条件を見直してください。",
    );
    return;
  }
  const buttons: HTMLButtonElement[] = [];
  const hints: string[] = [];
  if (actions.lowerMinMinutes !== null) {
    // 下限起因なら最小時間の引き下げが原因に対応する主操作。
    buttons.push(createLowerMinButton(actions.lowerMinMinutes));
    hints.push(`最小時間を ${String(actions.lowerMinMinutes)} 分に下げる`);
  }
  if (actions.widenMaxMinutes !== null) {
    const widen = createWidenMaxButton();
    if (buttons.length > 0) {
      // 主操作が別にある場合は補助に下げる（先頭だけを強調する）。
      widen.className = "secondary";
    }
    buttons.push(widen);
    hints.push("時間の上限を広げる");
  }
  const p = document.createElement("p");
  const explicitContext =
    searchMode === "explicit"
      ? "選択した入口・出口は端点単体では利用可能ですが、出発地点からのアクセス・帰着概算を含めると TIME_WINDOW になりました。"
      : "";
  p.textContent = `${explicitContext}指定時間枠に収まる周回候補が見つかりませんでした。${hints.join("か、")}と見つかる可能性があります。出発地点または時間条件を見直してください。`;
  recoveryOrigin = "result";
  el.recovery.replaceChildren(p, wrapActions(...buttons));
  revealRecovery(buttons[0] ?? null);
}

/** 最小時間を下げるボタン。値を実際に変えてから再検索を促す（結果の保証はしない）。 */
function createLowerMinButton(nextMinMinutes: number): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = `最小時間を ${String(nextMinMinutes)} 分に下げる`;
  button.addEventListener("click", () => {
    // 描画後に手入力で現在値が変わっていても、下げられないときは値を変えず成功も告げない。
    const outcome = lowerMinClickOutcome(Number(el.minMinutes.value), nextMinMinutes);
    clearRecovery();
    if (outcome.nextValue !== null) {
      el.minMinutes.value = String(outcome.nextValue);
    }
    setStatus(outcome.message);
    el.minMinutes.focus();
  });
  return button;
}

/** 最大時間を広げるボタン（従来の導線。製品上限 240 分に達しているときは作らない）。 */
function createWidenMaxButton(): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "時間の上限を広げる";
  button.addEventListener("click", () => {
    const current = Number(el.maxMinutes.value);
    const next = Math.min(MAX_PRODUCT_MINUTES, Math.max(current + 30, 30));
    clearRecovery();
    if (next !== current) {
      el.maxMinutes.value = String(next);
      setStatus(`最大時間を ${String(next)} 分に広げました。再検索してください。`);
    } else {
      // 値が変わらない操作を成功として告げない（上限 240 分では広げられない）。
      setStatus(`最大時間はすでに上限（${String(MAX_PRODUCT_MINUTES)} 分）です。出発地点や条件を見直してください。`);
    }
    el.maxMinutes.focus();
  });
  return button;
}

/**
 * 候補ゼロが時間枠では説明できないとき（探索打切り・引き継ぎ除外など）の復帰導線。
 * 時間枠を広げれば解決すると偽らず、条件変更・再試行の最小限の導線を出す（review R2-F3）。
 * 同じ入力での再検索はリリース成果物に対して決定論的に同じ結果（同一の打切り）を返すため、
 * 「もう一度検索」を第一候補にせず、出発地点を変える操作を先頭・フォーカス対象にする
 * （review R3-03）。SEARCH_LIMIT の打切りの意味は status 文言に残す。
 */
function showRetryRecovery(message: string): void {
  const p = document.createElement("p");
  p.textContent = `${message}出発地点や条件を変えるか、もう一度検索してください。`;
  const pick = document.createElement("button");
  pick.type = "button";
  pick.textContent = "地図で出発地点を指定";
  pick.addEventListener("click", () => {
    clearRecovery();
    enterPickMode();
  });
  const retry = document.createElement("button");
  retry.type = "button";
  retry.className = "secondary";
  retry.textContent = "もう一度検索";
  retry.addEventListener("click", () => {
    clearRecovery();
    startSearch();
  });
  recoveryOrigin = "result";
  el.recovery.replaceChildren(p, wrapActions(pick, wrapSecondaryAddressButton(), retry));
  revealRecovery(pick);
}

function wrapActions(...buttons: HTMLButtonElement[]): HTMLElement {
  const div = document.createElement("div");
  div.className = "recovery-actions";
  div.append(...buttons);
  return div;
}

function showAddressFallback(message: string): void {
  const p = document.createElement("p");
  p.textContent = message;
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "住所検索を使う";
  button.addEventListener("click", () => {
    el.addressQuery.focus();
  });
  recoveryOrigin = "error";
  el.recovery.replaceChildren(p, wrapActions(button));
  revealRecovery(button);
}

/**
 * 出発地点が対応範囲外・到達不能なときの復帰導線（docs/requirements.md:39）。
 * 対応範囲を明示し、検証済みの有効な出発地点（神田橋）をワンタップで設定できるようにする。
 * 近い高速道路へ直線接続するような誤った代替は出さない。
 */
function showAreaRecovery(message: string): void {
  const p = document.createElement("p");
  p.textContent = message;
  const usePreset = document.createElement("button");
  usePreset.type = "button";
  usePreset.textContent = "神田橋を出発地点にする";
  usePreset.addEventListener("click", () => {
    setOrigin({ lat: PRESET_KANDABASHI.lat, lon: PRESET_KANDABASHI.lon }, "対応範囲内の地点");
    el.preset.value = "kandabashi";
    clearRecovery();
    setStatus("出発地点を神田橋に設定しました。探索ボタンで再検索してください。");
  });
  const pick = document.createElement("button");
  pick.type = "button";
  pick.className = "secondary";
  pick.textContent = "地図で出発地点を指定";
  pick.addEventListener("click", () => {
    clearRecovery();
    enterPickMode();
  });
  const search = document.createElement("button");
  search.type = "button";
  search.className = "secondary";
  search.textContent = "住所を検索し直す";
  search.addEventListener("click", () => {
    el.addressQuery.focus();
  });
  recoveryOrigin = "result";
  el.recovery.replaceChildren(p, wrapActions(usePreset, pick, search));
  revealRecovery(usePreset);
}

/**
 * 指定枠では周回できない地点の復帰導線。時間を広げても届かないため
 * 「時間枠を広げる」は出さず、出発地点そのものの見直しを案内する。
 */
function showUnreachableRecovery(message: string): void {
  const p = document.createElement("p");
  p.textContent = message;
  const pick = document.createElement("button");
  pick.type = "button";
  pick.textContent = "地図で出発地点を指定";
  pick.addEventListener("click", () => {
    clearRecovery();
    enterPickMode();
  });
  const usePreset = document.createElement("button");
  usePreset.type = "button";
  usePreset.className = "secondary";
  usePreset.textContent = "神田橋を出発地点にする";
  usePreset.addEventListener("click", () => {
    setOrigin({ lat: PRESET_KANDABASHI.lat, lon: PRESET_KANDABASHI.lon }, "対応範囲内の地点");
    el.preset.value = "kandabashi";
    clearRecovery();
    setStatus("出発地点を神田橋に設定しました。探索ボタンで再検索してください。");
  });
  recoveryOrigin = "result";
  el.recovery.replaceChildren(
    p,
    wrapActions(pick, usePreset, wrapSecondaryAddressButton()),
  );
  revealRecovery(pick);
}

/** 住所検索へ戻る補助ボタン（復帰導線の共通部品）。 */
function wrapSecondaryAddressButton(): HTMLButtonElement {
  const search = document.createElement("button");
  search.type = "button";
  search.className = "secondary";
  search.textContent = "住所を検索し直す";
  search.addEventListener("click", () => {
    el.addressQuery.focus();
  });
  return search;
}

// --- 出発地点 ---

function readOriginFromFields(): LatLng {
  return { lat: Number(el.lat.value), lon: Number(el.lon.value) };
}

/**
 * 出発地点を確定する。座標欄・地図マーカー・要約表示を同期し、旧結果を失効させる。
 * sourceLabel は確定手段（住所・現在地・座標）を利用者へ示す。
 * 確定手段が変わったら、古い住所候補リストは破棄する（誤って旧候補へ巻き戻さない）。
 * あわせて地図を確定地点へ追従させ（表示範囲外なら pan/zoom）、プリセット select を
 * 実出発地点と矛盾しないよう manual に同期する（プリセット経由なら維持する）。
 */
function setOrigin(next: LatLng, sourceLabel: string): void {
  origin = next;
  el.lat.value = String(next.lat);
  el.lon.value = String(next.lon);
  el.originSummary.textContent = `出発地点: ${coordinateLabel(next.lat, next.lon)}（${sourceLabel}）`;
  updateExplicitOriginCallout();
  if (!sourceLabel.startsWith("プリセット")) {
    el.preset.value = "manual";
  }
  if (!sourceLabel.startsWith("住所:")) {
    // 進行中の住所検索を無効化し、候補リストを閉じる。
    addressRequestSeq += 1;
    el.addressCandidatesField.hidden = true;
    el.addressCandidates.replaceChildren();
  }
  if (!sourceLabel.startsWith("現在地")) {
    // 現在地以外で出発地点が確定したら、進行中の現在地取得を無効化する。
    // 遅れて成功した取得結果が確定済みの地点を上書きしないようにする（SEC-01）。
    geolocationRequestSeq += 1;
  }
  mapView?.setOrigin(next);
  mapView?.focusOrigin(next);
  invalidateResults();
}

function clearOrigin(): void {
  origin = null;
  el.originSummary.textContent = "出発地点が未確定です。";
  updateExplicitOriginCallout();
  // 出発地点を未確定に戻したら、進行中の現在地取得も無効化する。
  geolocationRequestSeq += 1;
}

function updateExplicitOriginCallout(): void {
  el.explicitOriginCurrent.textContent =
    origin === null
      ? "現在の出発地点: 未確定です。下の出発地点パネルで指定してください。"
      : `現在の出発地点: ${el.originSummary.textContent?.replace(/^出発地点:\s*/, "") ?? coordinateLabel(origin.lat, origin.lon)}`;
}

// --- 地図タップによる出発地点指定 ---

/**
 * 地図タップでの出発地点指定を開始する。ドラッグ/ズームと競合しないよう
 * 専用モード中だけ地図クリックを拾い、タップ結果は pending として表示する。
 */
function enterPickMode(): void {
  pickMode = true;
  pendingPick = null;
  mapView?.setPendingOrigin(null);
  mapView?.setPickMode(true);
  el.mapPick.setAttribute("aria-pressed", "true");
  el.mapPickPanel.hidden = false;
  el.mapPickConfirm.disabled = true;
  el.mapPickStatus.textContent =
    "地図をタップすると候補地点を表示します。確定前にどんどん選び直せます。";
  // disabled の確定ボタンへ focus() してもフォーカスは移らない（design F2）。
  // 状態文（地図の直下）へフォーカスし、パネルを可視位置へスクロールする。
  el.mapPickPanel.scrollIntoView({ block: "nearest" });
  el.mapPickStatus.focus({ preventScroll: true });
}

/**
 * モードを終了し、pending の候補地点を消す（出発地点は変更しない）。
 * restoreFocus を渡すと、hidden になったパネルに残ったフォーカスを
 * 論理的な起点へ戻す（design F3。body 落ちの防止）。
 */
function exitPickMode(announce: string | null, restoreFocus: HTMLElement | null = null): void {
  pickMode = false;
  pendingPick = null;
  mapView?.setPendingOrigin(null);
  mapView?.setPickMode(false);
  el.mapPick.setAttribute("aria-pressed", "false");
  el.mapPickPanel.hidden = true;
  el.mapPickConfirm.disabled = true;
  el.mapPickStatus.textContent = "";
  restoreFocus?.focus({ preventScroll: true });
  if (announce !== null) {
    setStatus(announce);
  }
}

function handlePickOrigin(point: LatLng): void {
  if (!pickMode) {
    return;
  }
  pendingPick = point;
  mapView?.setPendingOrigin(point);
  el.mapPickConfirm.disabled = false;
  // 逆ジオコーディングは対象外なので座標ラベルだけを示す。
  el.mapPickStatus.textContent = `候補地点: ${coordinateLabel(point.lat, point.lon)}（住所は取得していません）。「この地点を出発地点にする」で確定します。`;
  // タップ後に確定/取消が視界に入るよう、パネルを可視位置へスクロールする（design F1）。
  el.mapPickPanel.scrollIntoView({ block: "nearest" });
}

function confirmPick(): void {
  if (pendingPick === null) {
    return;
  }
  const point = pendingPick;
  exitPickMode(null);
  setOrigin(point, "地図で指定（住所は未取得）");
  clearRecovery();
  setStatus(
    `出発地点を地図の座標（${coordinateLabel(point.lat, point.lon)}）に設定しました。探索ボタンで再検索してください。`,
  );
  // 確定後は出発地点の要約（新しい起点）へフォーカスを戻す。
  el.originSummary.focus({ preventScroll: true });
}

// --- 住所検索 ---

let addressRequestSeq = 0;

async function handleAddressSearch(): Promise<void> {
  const error = validateAddressQuery(el.addressQuery.value);
  el.addressFeedback.textContent = error ?? "";
  if (error !== null) {
    el.addressCandidatesField.hidden = true;
    el.addressCandidates.replaceChildren();
    return;
  }

  addrsearch: {
    setAddressBusy(true);
    const seq = (addressRequestSeq += 1);
    try {
      const candidates = await geocode(el.addressQuery.value);
      if (seq !== addressRequestSeq) {
        break addrsearch; // 古い検索の応答は破棄
      }
      renderAddressCandidates(candidates);
    } catch (cause) {
      if (seq !== addressRequestSeq) {
        break addrsearch;
      }
      const code = cause instanceof GeocodeError ? cause.code : "FETCH_FAILED";
      el.addressFeedback.textContent = geocodeErrorMessage(code);
      el.addressCandidatesField.hidden = true;
      el.addressCandidates.replaceChildren();
    } finally {
      if (seq === addressRequestSeq) {
        setAddressBusy(false);
      }
    }
  }
}

function setAddressBusy(busy: boolean): void {
  el.addressSearch.disabled = busy;
  el.addressSearch.textContent = busy ? "検索中…" : "住所を検索";
}

/** 同名住所は候補から選ばせる。最初の候補へ無断で決定しない（docs/requirements.md:20）。 */
function renderAddressCandidates(candidates: GeocodeCandidate[]): void {
  el.addressCandidates.replaceChildren();
  if (candidates.length === 0) {
    el.addressFeedback.textContent =
      "住所が見つかりませんでした。表記を変えて再検索してください。時間の入力は保持されています。";
    el.addressCandidatesField.hidden = true;
    return;
  }
  el.addressFeedback.textContent = "";
  const name = `address-candidate-${String(addressRequestSeq)}`;
  const fragment = document.createDocumentFragment();
  candidates.forEach((candidate, i) => {
    const label = document.createElement("label");
    label.className = "address-option";
    const input = document.createElement("input");
    input.type = "radio";
    input.name = name;
    input.value = String(i);
    input.addEventListener("change", () => {
      setOrigin({ lat: candidate.lat, lon: candidate.lon }, `住所: ${candidate.label}`);
      el.addressFeedback.textContent = "";
    });
    const span = document.createElement("span");
    span.textContent = candidate.label;
    label.append(input, span);
    fragment.appendChild(label);
  });
  el.addressCandidates.appendChild(fragment);
  el.addressCandidatesField.hidden = false;
}

/** 現在地を一度だけ取得する。拒否・失敗時は住所検索へ誘導する（再要求しない）。 */
async function handleGeolocate(): Promise<void> {
  // 取得開始時に世代を採番し、完了時に自分が最新かを確認する。取得中に
  // 地図・住所・座標で出発地点が確定したら、遅れた結果は無視する（SEC-01）。
  const seq = (geolocationRequestSeq += 1);
  el.geolocate.disabled = true;
  el.geolocate.textContent = "取得中…";
  try {
    const position = await getCurrentPosition();
    if (seq !== geolocationRequestSeq) {
      return; // 後から確定した出発地点を現在地で上書きしない
    }
    setOrigin(position, "現在地");
    el.addressFeedback.textContent = "";
    clearRecovery();
    setStatus("現在地を出発地点に設定しました。");
  } catch (cause) {
    if (seq !== geolocationRequestSeq) {
      return; // 古い取得の失敗も無視する
    }
    const code = typeof cause === "object" && cause !== null && "code" in cause
      ? Number((cause as { code: unknown }).code)
      : 0;
    const message = geolocationErrorMessage(code);
    el.addressFeedback.textContent = message;
    showAddressFallback(message);
    // 権限拒否（code 1）は許可要求を繰り返さない（docs/requirements.md:20,37）。
    if (code === 1) {
      geolocationDenied = true;
      el.geolocate.disabled = true;
      el.geolocate.textContent = "現在地は利用できません";
      return;
    }
  } finally {
    if (!geolocationDenied) {
      el.geolocate.disabled = false;
      el.geolocate.textContent = "現在地を使う";
    }
  }
}

// --- 入力エラー ---

/**
 * 入力エラーの表示と各欄への aria 付与。
 * - 各 li に id を振り、欄の aria-describedby は**その欄に係る**エラーの id だけを指す
 * - エラーが無くなった欄からは属性を外すため、呼び出しごとに全欄を走査する
 *
 * @returns 最初のエラー欄（DOM 順）。エラーが無ければ null。
 */
function applyInputErrors(fields: import("./ui/model").InputFieldErrors): HTMLInputElement | null {
  const entries: { input: HTMLInputElement; message: string | null }[] = [
    { input: el.lat, message: fields.lat },
    { input: el.lon, message: fields.lon },
    { input: el.minMinutes, message: fields.minMinutes },
    { input: el.maxMinutes, message: fields.maxMinutes },
    { input: el.minMinutes, message: fields.range },
    { input: el.maxMinutes, message: fields.range },
  ];

  // 同一文言は 1 つの li を共有する（範囲条件は最小・最大のどちらからも参照される）。
  const idByMessage = new Map<string, string>();
  for (const { message } of entries) {
    if (message !== null && !idByMessage.has(message)) {
      idByMessage.set(message, `${ERROR_ID_PREFIX}${String(idByMessage.size + 1)}`);
    }
  }
  el.inputErrors.replaceChildren(
    ...[...idByMessage].map(([message, id]) => {
      const li = document.createElement("li");
      li.id = id;
      li.textContent = message;
      return li;
    }),
  );

  const describedBy = new Map<HTMLInputElement, string[]>();
  for (const { input, message } of entries) {
    if (message === null) {
      continue;
    }
    const id = idByMessage.get(message);
    if (id !== undefined) {
      describedBy.set(input, [...(describedBy.get(input) ?? []), id]);
    }
  }

  for (const input of INPUTS) {
    const errorIds = describedBy.get(input) ?? [];
    const describedIds = [...(STATIC_DESCRIBEDBY.get(input) ?? []), ...errorIds];
    if (errorIds.length > 0) {
      input.setAttribute("aria-invalid", "true");
    } else {
      input.removeAttribute("aria-invalid");
    }
    if (describedIds.length > 0) {
      input.setAttribute("aria-describedby", describedIds.join(" "));
    } else {
      input.removeAttribute("aria-describedby");
    }
  }

  return INPUTS.find((input) => describedBy.has(input)) ?? null;
}

// --- ランプ選択 UI ロジック ---

function updateSearchButtonState(): void {
  if (isSearching) {
    el.search.disabled = true;
    return;
  }
  if (searchMode === "explicit") {
    el.search.disabled =
      rampsLoading ||
      rampsDataset === null ||
      selectedEntryRampId === null ||
      selectedExitRampId === null;
  } else {
    el.search.disabled = false;
  }
}

function updateExplicitSelectionStatus(): void {
  if (selectedEntryRampId === null && selectedExitRampId === null) {
    el.explicitSelectionStatus.textContent = "入口と出口が未選択です。両方のランプを選択してください。";
  } else if (selectedEntryRampId === null) {
    el.explicitSelectionStatus.textContent = "入口が未選択です。有効な入口ランプを選択してください。";
  } else if (selectedExitRampId === null) {
    el.explicitSelectionStatus.textContent = "出口が未選択です。有効な出口ランプを選択してください。";
  } else {
    el.explicitSelectionStatus.textContent = "入口・出口が指定されています。「ルートを探す」を押して探索を開始できます。";
  }
  updateSearchButtonState();
}

function renderRampList(role: "entry" | "exit"): void {
  if (rampsDataset === null) {
    return;
  }
  const query = role === "entry" ? entryFilterQuery : exitFilterQuery;
  const listContainer = role === "entry" ? el.entryRampList : el.exitRampList;
  const countInfo = role === "entry" ? el.entryCountInfo : el.exitCountInfo;
  const zeroMessage = role === "entry" ? el.entryZeroMessage : el.exitZeroMessage;
  const clearBtn = role === "entry" ? el.entryClearSearchBtn : el.exitClearSearchBtn;
  const selectedId = role === "entry" ? selectedEntryRampId : selectedExitRampId;

  const result = filterRamps(rampsDataset.ramps, query, role);
  countInfo.textContent = formatCountInfo(result, query.trim().length > 0);
  clearBtn.hidden = query.trim().length === 0;

  if (result.matchedCount === 0) {
    zeroMessage.hidden = false;
    listContainer.hidden = true;
    listContainer.replaceChildren();
    return;
  }

  zeroMessage.hidden = true;
  listContainer.hidden = false;

  const fragment = document.createDocumentFragment();
  for (const { ramp, eligibility } of result.items) {
    const itemLabel = document.createElement("label");
    itemLabel.className = `ramp-item ${eligibility.selectable ? "ramp-item--routable" : "ramp-item--disabled"}`;
    itemLabel.htmlFor = `${role}-${ramp.id}`;

    const radio = document.createElement("input");
    radio.type = "radio";
    radio.id = `${role}-${ramp.id}`;
    radio.name = `${role}-ramp-selection`;
    radio.value = ramp.id;
    radio.disabled = !eligibility.selectable;
    if (!eligibility.selectable) {
      radio.setAttribute("aria-disabled", "true");
    }
    if (ramp.id === selectedId) {
      radio.checked = true;
    }

    radio.addEventListener("change", () => {
      if (radio.checked) {
        if (role === "entry") {
          setEntryRamp(ramp);
        } else {
          setExitRamp(ramp);
        }
      }
    });

    const content = document.createElement("div");
    content.className = "ramp-item-content";

    const header = document.createElement("div");
    header.className = "ramp-item-header";

    const routeBadge = document.createElement("span");
    routeBadge.className = "ramp-route-badge";
    routeBadge.textContent = ramp.route;

    const dirBadge = document.createElement("span");
    dirBadge.className = "ramp-dir-badge";
    dirBadge.textContent = formatDirection(ramp.direction);

    const nameSpan = document.createElement("span");
    nameSpan.className = "ramp-name";
    nameSpan.textContent = ramp.name;

    const statusBadge = document.createElement("span");
    statusBadge.className = `ramp-status-badge ramp-status-badge--${eligibility.category}`;
    statusBadge.textContent = eligibility.statusLabel;

    header.append(routeBadge, dirBadge, nameSpan, statusBadge);

    const idDiv = document.createElement("div");
    idDiv.className = "ramp-id";
    idDiv.textContent = ramp.id;

    content.append(header, idDiv);

    if (!eligibility.selectable && eligibility.reason) {
      const reasonP = document.createElement("p");
      reasonP.className = "ramp-disabled-reason";
      reasonP.textContent = eligibility.reason;
      content.append(reasonP);
    }

    itemLabel.append(radio, content);
    fragment.append(itemLabel);
  }

  listContainer.replaceChildren(fragment);
}

/** 入力イベントを1フレームに集約し、連続入力中の399件DOM再生成を抑える。 */
function scheduleRampListRender(role: "entry" | "exit"): void {
  const previous = role === "entry" ? entryRenderFrame : exitRenderFrame;
  if (previous !== null) {
    cancelAnimationFrame(previous);
  }
  const frame = requestAnimationFrame(() => {
    if (role === "entry") entryRenderFrame = null;
    else exitRenderFrame = null;
    renderRampList(role);
  });
  if (role === "entry") entryRenderFrame = frame;
  else exitRenderFrame = frame;
}

function setEntryRamp(ramp: RampItem): void {
  selectedEntryRampId = ramp.id;
  el.entrySelectedName.textContent = `${formatRoute(ramp.route)} ${formatDirection(ramp.direction)} ${ramp.name} (${ramp.id})`;
  el.entrySelectedBadge.hidden = false;
  invalidateResults();
  updateExplicitSelectionStatus();
}

function clearEntryRamp(): void {
  selectedEntryRampId = null;
  el.entrySelectedBadge.hidden = true;
  el.entrySelectedName.textContent = "";
  const checked = el.entryRampList.querySelector<HTMLInputElement>('input[type="radio"]:checked');
  if (checked) checked.checked = false;
  invalidateResults();
  updateExplicitSelectionStatus();
}

function setExitRamp(ramp: RampItem): void {
  selectedExitRampId = ramp.id;
  el.exitSelectedName.textContent = `${formatRoute(ramp.route)} ${formatDirection(ramp.direction)} ${ramp.name} (${ramp.id})`;
  el.exitSelectedBadge.hidden = false;
  invalidateResults();
  updateExplicitSelectionStatus();
}

function clearExitRamp(): void {
  selectedExitRampId = null;
  el.exitSelectedBadge.hidden = true;
  el.exitSelectedName.textContent = "";
  const checked = el.exitRampList.querySelector<HTMLInputElement>('input[type="radio"]:checked');
  if (checked) checked.checked = false;
  invalidateResults();
  updateExplicitSelectionStatus();
}

async function initRamps(): Promise<void> {
  const generation = (rampsLoadGeneration += 1);
  rampsLoadAbort?.abort();
  const abort = new AbortController();
  rampsLoadAbort = abort;
  rampsLoading = true;
  rampsDataset = null;
  el.rampsLoadingStatus.hidden = false;
  el.rampsLoadingStatus.textContent = "ランプ台帳を読み込み中…";
  el.rampsErrorPanel.hidden = true;
  el.rampPickersContainer.hidden = true;
  el.rampsRetryBtn.disabled = true;
  el.entryRampSearch.disabled = true;
  el.exitRampSearch.disabled = true;
  el.entryCountInfo.textContent = "ランプ台帳を読み込み中です。";
  el.exitCountInfo.textContent = "ランプ台帳を読み込み中です。";
  updateSearchButtonState();
  try {
    const dataset = await loadRampsDataset(window.fetch.bind(window), RELEASE_ID, abort.signal);
    if (generation !== rampsLoadGeneration) {
      return;
    }
    rampsDataset = dataset;
    el.rampsLoadingStatus.textContent = `正規ランプ台帳 ${String(rampsDataset.ramps.length)} 件を検証完了（選択可能: 入口 ${String(rampsDataset.capabilities.routableEntryCount)} / 出口 ${String(rampsDataset.capabilities.routableExitCount)}）`;
    el.rampPickersContainer.hidden = false;
    renderRampList("entry");
    renderRampList("exit");
    updateExplicitSelectionStatus();
  } catch (err) {
    if (generation !== rampsLoadGeneration) {
      return;
    }
    el.rampsLoadingStatus.hidden = true;
    el.rampsErrorPanel.hidden = false;
    el.rampPickersContainer.hidden = true;
    el.entryCountInfo.textContent = "台帳を読み込めないため件数を表示できません。";
    el.exitCountInfo.textContent = "台帳を読み込めないため件数を表示できません。";
    const msg = err instanceof Error ? err.message : String(err);
    el.rampsErrorMessage.textContent = `ランプ台帳の読み込みまたは検証に失敗しました（${msg}）。再読み込みをお試しください。`;
  } finally {
    if (generation === rampsLoadGeneration) {
      rampsLoading = false;
      rampsLoadAbort = null;
      el.rampsRetryBtn.disabled = false;
      el.entryRampSearch.disabled = false;
      el.exitRampSearch.disabled = false;
      updateSearchButtonState();
    }
  }
}

function cancelRampsLoad(): void {
  if (!rampsLoading) {
    return;
  }
  rampsLoadGeneration += 1;
  rampsLoadAbort?.abort();
  rampsLoadAbort = null;
  rampsLoading = false;
  el.rampsLoadingStatus.textContent = "ランプ台帳の読み込みを中断しました。ランプ指定モードで再開します。";
  el.rampsRetryBtn.disabled = false;
  el.entryRampSearch.disabled = false;
  el.exitRampSearch.disabled = false;
  updateSearchButtonState();
}

// --- 探索 ---

function setSearching(searching: boolean): void {
  isSearching = searching;
  updateSearchButtonState();
  el.cancel.hidden = !searching;
}

function startSearch(): void {
  if (origin === null) {
    setStatus("出発地点を確定してください（住所検索・現在地・座標のいずれか）。");
    el.addressQuery.focus();
    return;
  }
  const fields = validateInputFields(
    String(origin.lat),
    String(origin.lon),
    el.minMinutes.value,
    el.maxMinutes.value,
  );
  const firstError = applyInputErrors(fields);
  if (firstError !== null) {
    setStatus("入力に誤りがあります");
    firstError.focus();
    return; // 入力不備では検索しない
  }

  if (searchMode === "explicit") {
    const val = validateExplicitSearch(selectedEntryRampId, selectedExitRampId, rampsDataset ?? undefined);
    if (!val.valid) {
      setStatus(val.error ?? "入口・出口を選択してください。");
      if (!selectedEntryRampId) el.entryRampSearch.focus();
      else if (!selectedExitRampId) el.exitRampSearch.focus();
      return;
    }
    const currentCondition: ExplicitSearchCondition = {
      entryRampId: selectedEntryRampId,
      exitRampId: selectedExitRampId,
      minMinutes: Number(el.minMinutes.value),
      maxMinutes: Number(el.maxMinutes.value),
      origin,
    };
    const dup = duplicateOperationMessage(currentCondition, lastSuccessCondition);
    if (dup !== null) {
      el.duplicateWarning.textContent = dup;
      el.duplicateWarning.hidden = false;
      setStatus(dup);
      return;
    }
    el.duplicateWarning.hidden = true;
    el.duplicateWarning.textContent = "";
  }

  clearRecovery();
  requestCounter += 1;
  const requestId = `request-${String(requestCounter)}`;
  const msg: UiSearchMessage = {
    type: "search",
    requestId,
    releaseId: RELEASE_ID,
    pricingAt: new Date().toISOString(), // 押下時に 1 回だけ取得
    origin,
    ...(searchMode === "explicit"
      ? {
          entryRampId: selectedEntryRampId!,
          exitRampId: selectedExitRampId!,
        }
      : {}),
    minMinutes: Number(el.minMinutes.value),
    maxMinutes: Number(el.maxMinutes.value),
    vehicleProfile: VEHICLE_PROFILE,
  };

  // 二重送信防止: 旧 in-flight を無効化してから新 requestId で送る。
  clearTimer();
  inflightRequestId = requestId;
  hasSearched = true;
  setSearching(true);
  setStatus("探索中…");
  sendMessage(msg);

  timeoutId = setTimeout(() => {
    if (inflightRequestId !== requestId) {
      return;
    }
    stopWorker();
    pending = null;
    setSearching(false);
    clearResults();
    setStatus(errorMessage("TIMEOUT"));
  }, SEARCH_TIMEOUT_MS);
}

function cancelSearch(): void {
  if (inflightRequestId === null) {
    return;
  }
  stopWorker();
  pending = null;
  setSearching(false);
  setStatus("探索をキャンセルしました。条件を変更して再検索できます。");
}

// --- 結果描画 ---

function clearResults(): void {
  el.results.replaceChildren();
  currentResult = null;
  selectedCandidateId = null;
  mapView?.renderCandidates([]);
}

function renderResult(result: SearchResult): void {
  currentResult = result;
  const min = Number(el.minMinutes.value);
  const max = Number(el.maxMinutes.value);
  setStatus(statusMessage(result, min, max));

  const candidates = result.candidates.slice(0, 3);
  if (candidates.length === 0) {
    el.results.replaceChildren();
    mapView?.renderCandidates([]);
    // 候補ゼロの原因で復帰導線を分ける。届かない地点に「時間枠を広げる」を出すと誤りになる。
    switch (classifyNoCandidates(result)) {
      case "unreachable":
        // 数値根拠（最短計画分・最寄り入口距離）を本文と復帰導線の両方で示す。
        showUnreachableRecovery(
          searchMode === "explicit"
            ? `選択した入口・出口は端点単体では利用可能ですが、現在の出発地点から接続できません（NO_CONNECTION）。この地点では周回できる候補を作れないため、出発地点を見直してください。`
            : `この地点では周回できる候補を作れません（${String(MAX_PRODUCT_MINUTES)} 分が上限です）。時間を広げても届かないため、出発地点そのものを見直してください。`,
        );
        return;
      case "unsupported_area":
        // 出発地点が対応範囲外・接続不能なときは、対応範囲を示して有効な地点を案内する。
        showAreaRecovery(
          searchMode === "explicit"
            ? `選択した入口・出口は端点単体では利用可能ですが、現在の出発地点から接続できません（NO_CONNECTION）。出発地点を見直してください。${SUPPORTED_AREA_TEXT}`
            : SUPPORTED_AREA_TEXT,
        );
        return;
      case "time_window":
        // 指定枠が原因の場合は、実際に値が変わる操作（最小を下げる / 上限を広げる）だけを出す。
        showTimeWindowRecovery(result, min, max);
        return;
      case "retry":
        // 時間枠では説明できない候補ゼロ（SEARCH_LIMIT / NO_HANDOFF 等）。行き止まりにしない。
        showRetryRecovery(statusMessage(result, min, max));
        return;
    }
  }

  clearRecovery();
  // 料金確定状況は集合全体で判定する。1 件でも未算出なら順位を出さない。
  const canRank = canRankByPrice(result.rankingMode, candidates);
  const models = candidates.map((candidate, i) => {
    const model = toCardModel(candidate, i + 1);
    model.rankLabel = formatRank(canRank, i + 1);
    return model;
  });

  const fragment = document.createDocumentFragment();
  models.forEach((model, i) => {
    const candidate = candidates[i];
    if (candidate !== undefined) {
      fragment.appendChild(renderCard(model, candidate));
    }
  });
  el.results.replaceChildren(fragment);

  mapView?.renderCandidates(candidates);
  // 先頭候補を初期選択して地図と同期する（支援技術へ選択状態も伝える）。
  const first = models[0];
  if (first !== undefined) {
    selectCard(first.id, false);
    mapView?.fitToCandidates();
    // 結果が地図の下に隠れないよう、候補カード先頭へスクロールする。
    el.results.firstElementChild?.scrollIntoView({ block: "nearest" });
  }
}

function routeLine(lineStyle: string, lineStyleText: string): HTMLSpanElement {
  const line = document.createElement("span");
  line.className = `route-line route-line--${lineStyle}`;
  line.setAttribute("aria-hidden", "true");
  line.title = lineStyleText;
  return line;
}

function renderRouteOrder(model: CardModel): HTMLElement | null {
  if (model.routeLegs.length === 0 && model.routeOverview.length === 0) {
    return null;
  }
  const section = document.createElement("section");
  section.className = "route-order";
  const heading = document.createElement("h3");
  heading.textContent = model.routeLegs.length > 0 ? "首都高の経路順序" : "経路の順序（商品対象外）";
  section.appendChild(heading);
  if (model.pathSummary !== null) {
    const summary = document.createElement("p");
    summary.className = "route-summary";
    summary.textContent = model.pathSummary;
    section.appendChild(summary);
  }
  const list = document.createElement("ol");
  list.className = "route-order-list";
  if (model.routeLegs.length > 0) {
    for (const leg of model.routeLegs) {
      const item = document.createElement("li");
      item.dataset.routeRole = leg.role;
      const number = document.createElement("span");
      number.className = "route-step-number";
      number.textContent = String(leg.number);
      const copy = document.createElement("span");
      copy.className = "route-step-copy";
      const label = document.createElement("strong");
      label.textContent = leg.label;
      const style = document.createElement("span");
      style.className = "route-step-style";
      style.textContent = leg.lineStyleText;
      copy.append(label, style);
      item.append(number, routeLine(leg.lineStyle, leg.lineStyleText), copy);
      list.appendChild(item);
    }
  } else {
    for (const step of model.routeOverview) {
      const item = document.createElement("li");
      const number = document.createElement("span");
      number.className = "route-step-number";
      number.textContent = String(step.number);
      const copy = document.createElement("span");
      copy.className = "route-step-copy";
      const label = document.createElement("strong");
      label.textContent = step.label;
      const style = document.createElement("span");
      style.className = "route-step-style";
      style.textContent = step.lineStyleText;
      copy.append(label, style);
      if (step.detail !== "") {
        const detail = document.createElement("span");
        detail.className = "route-step-detail";
        detail.textContent = step.detail;
        copy.appendChild(detail);
      }
      item.append(number, routeLine(step.lineStyle, step.lineStyleText), copy);
      list.appendChild(item);
    }
  }
  section.appendChild(list);
  return section;
}

function renderEstimatedLegs(model: CardModel): HTMLElement | null {
  if (model.estimatedLegs.length === 0) {
    return null;
  }
  const section = document.createElement("section");
  section.className = "estimated-legs";
  const heading = document.createElement("h3");
  heading.textContent = "一般道の概算区間";
  const note = document.createElement("p");
  note.textContent = "一般道の推定区間は地図の線に含めていません。";
  const list = document.createElement("ul");
  for (const leg of model.estimatedLegs) {
    const item = document.createElement("li");
    const label = document.createElement("span");
    label.className = "estimated-leg-label";
    label.textContent = leg.label;
    const value = document.createElement("span");
    value.className = "estimated-leg-value";
    value.textContent = `推定 ${String(leg.distanceKm)} km / 約${String(leg.durationMinutes)}分`;
    item.dataset.estimatedRole = leg.role;
    item.append(label, value);
    list.appendChild(item);
  }
  section.append(heading, note, list);
  return section;
}

function renderCard(model: CardModel, candidate: Candidate): HTMLElement {
  const card = document.createElement("article");
  card.className = "card";
  card.dataset.mapsUrl = model.mapsUrl;
  card.dataset.candidateId = model.id;
  card.setAttribute("role", "listitem");
  card.setAttribute("aria-current", "false");
  card.tabIndex = 0;

  const head = document.createElement("div");
  head.className = "card-head";
  const index = document.createElement("span");
  index.className = "candidate-index";
  index.textContent = String(model.index);
  index.setAttribute("aria-label", `候補 ${String(model.index)}`);
  head.appendChild(index);
  const recommended = recommendedLabel(candidate);
  if (recommended !== null) {
    head.appendChild(badge("recommended", recommended));
  }
  if (model.rankLabel !== null) {
    head.appendChild(badge("rank", model.rankLabel));
  }
  card.appendChild(head);

  // 比較の主指標は余裕込みの計画時間。総推定時間はその内訳として補助に落とす
  // （docs/requirements.md:24「総推定時間と余裕込みの計画時間」）。
  const plan = document.createElement("p");
  plan.className = "plan-time";
  plan.textContent = `計画時間: 約${String(model.planMinutes)}分`;
  card.appendChild(plan);

  const duration = document.createElement("p");
  duration.className = "duration";
  duration.textContent = `総所要時間: 約${String(model.baseMinutes)}分`;
  card.appendChild(duration);

  const durationNote = document.createElement("p");
  durationNote.className = "duration-note";
  durationNote.textContent = "アクセス・帰着を含み（概算）、休憩は含みません";
  card.appendChild(durationNote);

  const breakdown = document.createElement("p");
  breakdown.className = "breakdown";
  breakdown.textContent = timeBreakdownText(model);
  card.appendChild(breakdown);

  // 料金は比較の副次指標。金額が確定しているときだけ順位を併記する。
  const toll = document.createElement("p");
  toll.className = "toll";
  toll.textContent =
    model.rankLabel === null ? model.toll : `${model.toll}（${model.rankLabel}）`;
  card.appendChild(toll);

  // 入口・出口は「課金対象」と重複するため 1 行に統合する（restraint）。
  const charging = document.createElement("p");
  charging.className = "charging";
  charging.textContent = model.chargedSection;
  card.appendChild(charging);

  const distance = document.createElement("p");
  distance.className = "distance distance--total";
  distance.textContent = `総距離: ${String(model.distanceKm)} km`;
  card.appendChild(distance);

  const shutokoDistance = document.createElement("p");
  shutokoDistance.className = "distance distance--shutoko";
  shutokoDistance.textContent = `首都高距離: ${String(model.shutokoDistanceKm)} km`;
  card.appendChild(shutokoDistance);

  if (model.estimatedLegs.length > 0) {
    const distanceNote = document.createElement("p");
    distanceNote.className = "distance-note";
    distanceNote.textContent = "総距離には一般道の推定距離を含みます。";
    card.appendChild(distanceNote);
  }

  const accessDist = document.createElement("p");
  accessDist.className = "access-distance";
  accessDist.textContent = `入口まで（直線）: ${String(Number((candidate.snappedOrigin.distanceMeters / 1000).toFixed(1)))} km`;
  card.appendChild(accessDist);

  const routeOrder = renderRouteOrder(model);
  if (routeOrder !== null) {
    card.appendChild(routeOrder);
  }
  const estimatedLegs = renderEstimatedLegs(model);
  if (estimatedLegs !== null) {
    card.appendChild(estimatedLegs);
  }

  // 円当たり効率は金額が算出できたときだけ示す（docs/requirements.md:24）。
  if (model.timePerYen !== null) {
    const efficiency = document.createElement("p");
    efficiency.className = "efficiency";
    efficiency.textContent = `1区間の料金で首都高を ${model.timePerYen}`;
    card.appendChild(efficiency);
  }

  // 通過路線は比較の主眼ではないため折りたたむ（情報過多の抑制）。
  if (model.roadNames.length > 0) {
    const details = document.createElement("details");
    details.className = "roads";
    const summary = document.createElement("summary");
    summary.textContent = `通過路線（${String(model.roadNames.length)}）`;
    const list = document.createElement("ul");
    for (const name of model.roadNames) {
      const li = document.createElement("li");
      li.textContent = name;
      list.appendChild(li);
    }
    details.append(summary, list);
    card.appendChild(details);
  }

  if (model.reasons.length > 0) {
    const ul = document.createElement("ul");
    ul.className = "reasons";
    for (const reason of model.reasons) {
      const li = document.createElement("li");
      li.textContent = reason;
      ul.appendChild(li);
    }
    card.appendChild(ul);
  }

  if (model.warnings.length > 0) {
    const ul = document.createElement("ul");
    ul.className = "warnings";
    for (const warning of model.warnings) {
      const li = document.createElement("li");
      li.textContent = warning;
      ul.appendChild(li);
    }
    card.appendChild(ul);
  }

  if (model.mapsHandoffNotice !== null) {
    const notice = document.createElement("p");
    notice.className = "maps-handoff-notice";
    notice.textContent = model.mapsHandoffNotice;
    card.appendChild(notice);
  }

  if (model.mapsUrl !== "") {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "depart";
    button.textContent = "出発する（Google マップを開く）";
    button.addEventListener("click", (event) => {
      event.stopPropagation(); // カード選択と出発のクリックを分離する
      window.open(model.mapsUrl, "_blank", "noopener");
    });
    card.appendChild(button);
  }

  card.addEventListener("click", () => {
    selectCard(model.id, true);
  });
  card.addEventListener("keydown", (event) => {
    // カード自身にフォーカスがある時だけ選択操作として扱う。
    // 子の出発ボタンからの Enter/Space を奪うと Maps 遷移が動かなくなる。
    if (event.target !== card) {
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      selectCard(model.id, true);
    }
  });
  return card;
}

function badge(className: string, text: string): HTMLSpanElement {
  const span = document.createElement("span");
  span.className = `badge ${className}`;
  span.textContent = text;
  return span;
}

/** カード選択と地図強調を同期する。focusCard はキーボード選択時にフォーカスも移す。 */
function selectCard(id: string, focusCard: boolean): void {
  selectedCandidateId = id;
  for (const card of el.results.querySelectorAll<HTMLElement>(".card")) {
    const selected = card.dataset.candidateId === id;
    // aria-selected は listitem で無効なため aria-current を使う（要件: 選択状態の通知）。
    card.setAttribute("aria-current", selected ? "true" : "false");
    if (selected && focusCard) {
      card.focus();
    }
  }
  mapView?.selectCandidate(id);
  // 出発リンクは地図確認ができるまで無効化する（docs/requirements.md:44）。
  updateDepartButtons();
}

function updateDepartButtons(): void {
  const disabled = mapFailed;
  for (const button of el.results.querySelectorAll<HTMLButtonElement>(".depart")) {
    button.disabled = disabled;
  }
}

/**
 * 条件変更時に前回の候補・選択・出発リンクを無効化する。
 * 前回の探索結果に基づく復帰導線も失効させる（最小時間を手入力した後に古い「下げる」
 * ボタンが残り、無変更や引上げを成功として告げるのを防ぐ。review R3-01）。
 * 結果取得前の成果物不一致・通信失敗の再読み込み案内は条件を変えても有効なため残す。
 * 入力エラーは消さない（design-review-002 N1）。消去は探索ボタン押下時の再検証だけに任せる。
 * 探索実行中（in-flight）の場合のみ stopWorker() で中断し、アイドル時は Worker と PreparedGraph を維持する。
 */
function invalidateResults(): void {
  el.results.replaceChildren();
  currentResult = null;
  selectedCandidateId = null;
  mapView?.renderCandidates([]);
  clearResultRecovery();
  lastSuccessCondition = null;
  el.duplicateWarning.hidden = true;
  el.duplicateWarning.textContent = "";
  if (inflightRequestId !== null) {
    stopWorker();
  }
  setSearching(false);
  updateSearchButtonState();
  if (hasSearched) {
    setStatus("条件が変更されました。探索ボタンで再検索してください。");
  }
}

// --- 地図 ---

function initMap(): void {
  // 再試行リスナーは地図生成の成否に関わらず登録する（生成失敗時も復帰手段を残す）。
  el.mapRetry.addEventListener("click", retryMap);
  try {
    mapView = createMapView(el.map);
  } catch {
    // 同期例外はタイル単位の失敗を経ないため、即座に復帰不能として扱う。
    markMapFailed(true);
    return;
  }
  mapView.onTileError(() => markMapFailed(false));
  // タイルが実際に読めたら失敗状態を解除する（自然回復を反映）。
  mapView.onTileLoad(() => clearMapFailed());
  // 地図タップは pending として受け取り、確定ボタンで出発地点にする。
  mapView.onPickOrigin((point) => handlePickOrigin(point));
  if (origin !== null) {
    mapView.setOrigin(origin);
  }
}

/**
 * Leaflet のタイル失敗は個別タイルで発火する。連続失敗で復帰不能とみなし、
 * 地図確認ができるまで出発を無効化する（docs/requirements.md:44）。
 */
let tileErrorCount = 0;
function markMapFailed(immediate: boolean): void {
  tileErrorCount += 1;
  if (!immediate && tileErrorCount < 4) {
    return;
  }
  mapFailed = true;
  el.mapFallback.hidden = false;
  updateDepartButtons();
}

function clearMapFailed(): void {
  tileErrorCount = 0;
  if (!mapFailed) {
    return;
  }
  mapFailed = false;
  el.mapFallback.hidden = true;
  updateDepartButtons();
}

function retryMap(): void {
  // 楽観的に解除しない。タイルが実際に読めた時点（tileload）で解除する。
  tileErrorCount = 0;
  if (mapView !== null) {
    mapView.renderCandidates(currentResult?.candidates.slice(0, 3) ?? []);
    if (selectedCandidateId !== null) {
      mapView.selectCandidate(selectedCandidateId);
    }
    if (currentResult !== null) {
      mapView.fitToCandidates();
    }
    // タイルレイヤーを作り直して再取得させる（失敗タイルのキャッシュに依らない）。
    mapView.reloadTiles();
  }
  setStatus("地図を再読み込みしています。");
}

// --- 初期化 ---

el.addressSearch.addEventListener("click", () => {
  void handleAddressSearch();
});
el.addressQuery.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    void handleAddressSearch();
  }
});
el.geolocate.addEventListener("click", () => {
  void handleGeolocate();
});
el.preset.addEventListener("change", () => {
  applyPreset();
});
for (const input of [el.lat, el.lon, el.minMinutes, el.maxMinutes]) {
  input.addEventListener("change", () => {
    // 座標欄を直接編集した場合は、その座標を出発地点として扱う
    // （キーボード利用者にとって地図タップと等価な入力手段）。
    if (input === el.lat || input === el.lon) {
      const next = readOriginFromFields();
      if (Number.isFinite(next.lat) && Number.isFinite(next.lon)) {
        setOrigin(next, "座標入力");
        return;
      }
    }
    invalidateResults();
  });
}
for (const chip of document.querySelectorAll<HTMLButtonElement>(".presets .chip")) {
  chip.addEventListener("click", () => {
    el.minMinutes.value = chip.dataset.min ?? el.minMinutes.value;
    el.maxMinutes.value = chip.dataset.max ?? el.maxMinutes.value;
    invalidateResults();
  });
}
el.search.addEventListener("click", startSearch);
el.cancel.addEventListener("click", cancelSearch);
el.mapPick.addEventListener("click", () => {
  if (pickMode) {
    exitPickMode("地図タップでの指定をやめました。", el.mapPick);
    return;
  }
  clearRecovery();
  enterPickMode();
});
el.mapPickConfirm.addEventListener("click", confirmPick);
el.mapPickCancel.addEventListener("click", () => {
  exitPickMode("地図タップでの指定をキャンセルしました。", el.mapPick);
});
// Esc でモードを抜ける（誤タップを確定させない）。
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && pickMode) {
    exitPickMode("地図タップでの指定をキャンセルしました。", el.mapPick);
  }
});

// 検索モード切り替え
const searchModeRadios = document.querySelectorAll<HTMLInputElement>('input[name="search-mode"]');
for (const radio of searchModeRadios) {
  radio.addEventListener("change", () => {
    if (radio.checked) {
      searchMode = radio.value as SearchMode;
      el.explicitOdSection.hidden = searchMode !== "explicit";
      if (searchMode === "explicit" && rampsDataset === null && !rampsLoading) {
        void initRamps();
      } else if (searchMode === "coord") {
        cancelRampsLoad();
      }
      invalidateResults();
      updateExplicitSelectionStatus();
    }
  });
}

// 入口・出口ランプ検索・解除操作
el.entryRampSearch.addEventListener("input", () => {
  entryFilterQuery = el.entryRampSearch.value;
  scheduleRampListRender("entry");
});
el.entryClearSearchBtn.addEventListener("click", () => {
  el.entryRampSearch.value = "";
  entryFilterQuery = "";
  renderRampList("entry");
  el.entryRampSearch.focus();
});
el.entryResetFilterBtn.addEventListener("click", () => {
  el.entryRampSearch.value = "";
  entryFilterQuery = "";
  renderRampList("entry");
  el.entryRampSearch.focus();
});
el.entryDeselectBtn.addEventListener("click", () => {
  clearEntryRamp();
});

el.exitRampSearch.addEventListener("input", () => {
  exitFilterQuery = el.exitRampSearch.value;
  scheduleRampListRender("exit");
});
el.exitClearSearchBtn.addEventListener("click", () => {
  el.exitRampSearch.value = "";
  exitFilterQuery = "";
  renderRampList("exit");
  el.exitRampSearch.focus();
});
el.exitResetFilterBtn.addEventListener("click", () => {
  el.exitRampSearch.value = "";
  exitFilterQuery = "";
  renderRampList("exit");
  el.exitRampSearch.focus();
});
el.exitDeselectBtn.addEventListener("click", () => {
  clearExitRamp();
});

el.rampsRetryBtn.addEventListener("click", () => {
  void initRamps();
});

el.preset.value = "kandabashi";
setSearching(false);
setStatus("成果物を読み込み中…");
initMap();
// 地図生成後にプリセットを適用し、出発地マーカーと要約を同期させる。
applyPreset();
createWorker(); // ブート: ready が来たらステータスへ反映される

function applyPreset(): void {
  if (el.preset.value === "kandabashi") {
    el.lat.value = String(PRESET_KANDABASHI.lat);
    el.lon.value = String(PRESET_KANDABASHI.lon);
    setOrigin({ lat: PRESET_KANDABASHI.lat, lon: PRESET_KANDABASHI.lon }, "プリセット: 神田橋");
    return;
  }
  clearOrigin();
  invalidateResults();
}
