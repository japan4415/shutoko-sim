// 製品 UI の配線本体。描画ロジックの純粋部分は src/ui/model.ts、地図は src/map/、
// 外部 I/O は src/geocode.ts / src/geolocation.ts に置く。
// - 探索押下時に pricingAt を 1 回取得し、requestId を採番して Worker へ search を送る
// - in-flight の requestId 以外の応答は無視し、再押下時は旧を無効化してから新を送る
// - 10 秒で setTimeout → worker.terminate() → TIMEOUT 文言 → 次回検索時に Worker を再生成する
// - 候補カードと地図は同じ候補 ID に紐付け、条件変更で選択と出発リンクを失効させる
import {
  PRESET_KANDABASHI,
  RELEASE_ID,
  SEARCH_TIMEOUT_MS,
  VEHICLE_PROFILE,
  coordinateLabel,
  errorMessage,
  formatRank,
  geocodeErrorMessage,
  geolocationErrorMessage,
  recommendedLabel,
  statusMessage,
  timeBreakdownText,
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
  inputErrors: mustGet<HTMLOListElement>("input-errors"),
  status: mustGet<HTMLParagraphElement>("status"),
  map: mustGet<HTMLDivElement>("map"),
  mapFallback: mustGet<HTMLParagraphElement>("map-fallback"),
  mapRetry: mustGet<HTMLButtonElement>("map-retry-btn"),
  recovery: mustGet<HTMLDivElement>("recovery-actions"),
  results: mustGet<HTMLDivElement>("results"),
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

// --- Worker 管理 ---

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

function clearRecovery(): void {
  el.recovery.hidden = true;
  el.recovery.replaceChildren();
}

/** 再読み込みでしか復帰できない成果物不整合・通信失敗の案内（docs/requirements.md:43）。 */
function showReloadRecovery(code: string): void {
  if (code !== "ARTIFACT_MISMATCH" && code !== "FETCH_FAILED" && code !== "WASM_ERROR") {
    return;
  }
  const p = document.createElement("p");
  p.textContent = "成果物を読み込めませんでした。部分データでは探索しません。";
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "再読み込み";
  button.addEventListener("click", () => {
    window.location.reload();
  });
  el.recovery.replaceChildren(p, wrapActions(button));
  el.recovery.hidden = false;
}

/** 候補なし・時間枠不一致からの復帰（時間を広げる案内。自動変更しない）。 */
function showTimeWindowRecovery(text: string): void {
  const p = document.createElement("p");
  p.textContent = text;
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "時間の上限を広げる";
  button.addEventListener("click", () => {
    const current = Number(el.maxMinutes.value);
    const next = Math.min(240, Math.max(current + 30, 30));
    el.maxMinutes.value = String(next);
    clearRecovery();
    if (hasSearched) {
      setStatus(`最大時間を ${String(next)} 分に広げました。再検索してください。`);
    }
    el.maxMinutes.focus();
  });
  el.recovery.replaceChildren(p, wrapActions(button));
  el.recovery.hidden = false;
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
  el.recovery.replaceChildren(p, wrapActions(button));
  el.recovery.hidden = false;
}

// --- 出発地点 ---

function readOriginFromFields(): LatLng {
  return { lat: Number(el.lat.value), lon: Number(el.lon.value) };
}

/**
 * 出発地点を確定する。座標欄・地図マーカー・要約表示を同期し、旧結果を失効させる。
 * sourceLabel は確定手段（住所・現在地・座標）を利用者へ示す。
 * 確定手段が変わったら、古い住所候補リストは破棄する（誤って旧候補へ巻き戻さない）。
 */
function setOrigin(next: LatLng, sourceLabel: string): void {
  origin = next;
  el.lat.value = String(next.lat);
  el.lon.value = String(next.lon);
  el.originSummary.textContent = `出発地点: ${coordinateLabel(next.lat, next.lon)}（${sourceLabel}）`;
  if (!sourceLabel.startsWith("住所:")) {
    // 進行中の住所検索を無効化し、候補リストを閉じる。
    addressRequestSeq += 1;
    el.addressCandidatesField.hidden = true;
    el.addressCandidates.replaceChildren();
  }
  mapView?.setOrigin(next);
  invalidateResults();
}

function clearOrigin(): void {
  origin = null;
  el.originSummary.textContent = "出発地点が未確定です。";
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
  el.geolocate.disabled = true;
  el.geolocate.textContent = "取得中…";
  try {
    const position = await getCurrentPosition();
    setOrigin(position, "現在地");
    el.addressFeedback.textContent = "";
    clearRecovery();
    setStatus("現在地を出発地点に設定しました。");
  } catch (cause) {
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

// --- 探索 ---

function setSearching(searching: boolean): void {
  el.search.disabled = searching;
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

  clearRecovery();
  requestCounter += 1;
  const requestId = `request-${String(requestCounter)}`;
  const msg: UiSearchMessage = {
    type: "search",
    requestId,
    releaseId: RELEASE_ID,
    pricingAt: new Date().toISOString(), // 押下時に 1 回だけ取得
    origin,
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
    // 候補なしは時間条件が原因のことが多いため、時間を広げる導線を出す。
    if (result.status === "no_candidates" || result.status === "truncated") {
      showTimeWindowRecovery(
        "指定条件に収まる周回候補が見つかりませんでした。時間の範囲を広げると見つかる可能性があります。",
      );
    }
    return;
  }

  clearRecovery();
  // 料金確定状況は集合全体で判定する。1 件でも未算出なら順位を出さない。
  const allPriced = candidates.every((candidate) => candidate.toll.amountYen !== null);
  const models = candidates.map((candidate, i) => {
    const model = toCardModel(candidate, i + 1);
    model.rankLabel = formatRank(allPriced, i + 1);
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
  durationNote.textContent = "一般道での帰着までを含み、休憩は含みません";
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
  charging.textContent = `課金対象: ${model.route} の1区間`;
  card.appendChild(charging);

  const distance = document.createElement("p");
  distance.className = "distance";
  distance.textContent = `実走行距離: ${String(model.distanceKm)} km`;
  card.appendChild(distance);

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

  const button = document.createElement("button");
  button.type = "button";
  button.className = "depart";
  button.textContent = "出発する（Google マップを開く）";
  button.addEventListener("click", (event) => {
    event.stopPropagation(); // カード選択と出発のクリックを分離する
    window.open(model.mapsUrl, "_blank", "noopener");
  });
  card.appendChild(button);

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
 * 入力エラーは消さない（design-review-002 N1）。消去は探索ボタン押下時の再検証だけに任せる。
 */
function invalidateResults(): void {
  el.results.replaceChildren();
  currentResult = null;
  selectedCandidateId = null;
  mapView?.renderCandidates([]);
  stopWorker();
  setSearching(false);
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
    // 座標欄を直接編集した場合は、その座標を出発地点として扱う。
    if (input === el.lat || input === el.lon) {
      const next = readOriginFromFields();
      if (Number.isFinite(next.lat) && Number.isFinite(next.lon)) {
        origin = next;
        el.originSummary.textContent = `出発地点: ${coordinateLabel(next.lat, next.lon)}（座標入力）`;
        mapView?.setOrigin(next);
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
