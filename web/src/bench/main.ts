// 計測ページ（bench.html）の駆動部。
//
// 各パターン（代表出発地点 × 時間条件）について cold N 回 → warm N 回を自動実行する。
// 1 試行ごとに新しい Worker を生成し、終了後に terminate する（ウォームアップの持ち越しを
// 排除するため）。cold 試行は `bench.cacheBust` に一意 nonce を渡して成果物 URL を変え、
// ブラウザ HTTP キャッシュ（`immutable`）を迂回する。warm 試行は nonce を渡さず、
// 事前に載せたキャッシュ（primeArtifactCache）に当てる。
//
// 時刻はすべて `performance.timeOrigin + performance.now()` の epoch ms で記録し、
// ページと Worker のマークを同じ土台で差分できるようにする（Worker の time origin は
// Worker 生成時刻なので、`performance.now()` の生値はページと比較できない）。
import {
  BENCH_TARGETS,
  buildEnvelope,
  validateEnvelope,
  type BenchDevice,
  type BenchEnvelope,
  type BenchPageLoad,
  type BenchResourceEntry,
  type BenchTrial,
} from "./envelope";
import { selectPatterns, type BenchPattern } from "./patterns";
import { aggregate, judge, type BenchAggregate, type JudgeVerdict } from "./summarize";
import {
  RELEASE_ID,
  SEARCH_TIMEOUT_MS,
  VEHICLE_PROFILE,
  errorMessage,
  statusMessage,
  toCardModel,
} from "../ui/model";
import {
  BENCH_WORKER_NAME,
  type BenchPayload,
  type UiSearchMessage,
  type WorkerResponse,
} from "../worker/types";
import type { SearchResult } from "../worker/types";

/** 1 試行で取得する成果物 URL（prime と resource 収集で同じ並びを使う）。 */
const ARTIFACT_PATHS = [
  "manifest.json",
  "engine.json",
  "graph.json",
  "shutoko_routing_bg.wasm",
  "shutoko_routing.js",
] as const;

/** cold / warm それぞれの既定試行回数。 */
const DEFAULT_REPEATS = 3;

declare global {
  interface Window {
    /** 最後に確定した envelope（Playwright が回収する）。 */
    __benchResult?: BenchEnvelope;
    /** 全試行が終わったか。 */
    __benchDone?: boolean;
    /** 計測が途中で失敗したときの理由（`__benchDone` は立たない）。 */
    __benchError?: string;
  }
}

const el = {
  start: mustGet<HTMLButtonElement>("start-btn"),
  status: mustGet<HTMLParagraphElement>("status"),
  progress: mustGet<HTMLParagraphElement>("progress"),
  verdicts: mustGet<HTMLDivElement>("verdicts"),
  summary: mustGet<HTMLDivElement>("summary"),
  json: mustGet<HTMLPreElement>("json"),
  download: mustGet<HTMLAnchorElement>("download"),
  uploadBtn: mustGet<HTMLButtonElement>("upload-btn"),
  uploadStatus: mustGet<HTMLParagraphElement>("upload-status"),
  validate: mustGet<HTMLParagraphElement>("validate"),
  results: mustGet<HTMLDivElement>("results"),
  deviceName: mustGet<HTMLInputElement>("device-name"),
  os: mustGet<HTMLInputElement>("device-os"),
  browser: mustGet<HTMLInputElement>("device-browser"),
  network: mustGet<HTMLInputElement>("device-network"),
  memory: mustGet<HTMLInputElement>("memory-manual"),
  note: mustGet<HTMLInputElement>("device-note"),
};

const MANUAL_INPUTS = [el.deviceName, el.os, el.browser, el.network, el.memory, el.note];

let trials: BenchTrial[] = [];
let running = false;
let uploading = false;
let objectUrl: string | null = null;

function mustGet<T extends Element>(id: string): T {
  const node = document.getElementById(id);
  if (node === null) {
    throw new Error(`missing element: ${id}`);
  }
  return node as unknown as T;
}

function epochNow(): number {
  return performance.timeOrigin + performance.now();
}

/** `performance.memory`（Chromium のみ）から JS ヒープ使用量を MiB で返す。 */
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

function toResourceEntry(entry: PerformanceEntry): BenchResourceEntry {
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
}

function doubleRaf(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        resolve();
      });
    });
  });
}

function newNonce(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `${String(Date.now())}-${String(Math.random()).slice(2, 10)}`;
  }
}

function textOrNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function readDevice(): BenchDevice {
  return {
    ua: navigator.userAgent,
    platform: navigator.platform,
    deviceName: textOrNull(el.deviceName.value),
    os: textOrNull(el.os.value),
    browser: textOrNull(el.browser.value),
    network: textOrNull(el.network.value),
    note: textOrNull(el.note.value),
  };
}

/** 手入力のピークメモリ（MiB）。未入力・不正値は null。 */
function readManualMemory(): number | null {
  const value = Number(el.memory.value);
  return el.memory.value.trim() === "" || !Number.isFinite(value) ? null : value;
}

function capturePageLoad(): BenchPageLoad {
  const navigation = performance.getEntriesByType("navigation")[0] as
    | PerformanceNavigationTiming
    | undefined;
  return {
    url: window.location.href,
    navigation:
      navigation === undefined
        ? null
        : {
            startTime: navigation.startTime,
            responseEnd: navigation.responseEnd,
            loadEventEnd: navigation.loadEventEnd,
            domContentLoadedEventEnd: navigation.domContentLoadedEventEnd,
            transferSize: navigation.transferSize,
            encodedBodySize: navigation.encodedBodySize,
            decodedBodySize: navigation.decodedBodySize,
          },
    // bench ページ自身のリソース（html / js / css）。成果物は Worker 側で収集する。
    resources: performance
      .getEntriesByType("resource")
      .filter((entry) => !entry.name.includes("/releases/"))
      .map(toResourceEntry),
  };
}

/**
 * warm 試行の前提を作る。warm は nonce を付けない URL を叩くため、その URL が
 * ブラウザ HTTP キャッシュ（`immutable`）に載っている必要がある。試行前に一度だけ
 * 5 成果物を取得してキャッシュへ載せる（試行の計測値には含めない）。
 */
async function primeArtifactCache(): Promise<void> {
  const base = `/releases/${encodeURIComponent(RELEASE_ID)}`;
  await Promise.all(
    ARTIFACT_PATHS.map(async (path) => {
      try {
        const response = await fetch(`${base}/${path}`);
        await response.arrayBuffer();
      } catch {
        // 事前取得に失敗しても試行は続ける（warm が cold 相当になるだけ）。
      }
    }),
  );
}

/** 候補カードを通常 UI と同じ構造で描く（描画コストを揃えるため）。 */
function renderCard(model: ReturnType<typeof toCardModel>): HTMLElement {
  const card = document.createElement("article");
  card.className = "card";
  card.dataset.mapsUrl = model.mapsUrl;
  const duration = document.createElement("p");
  duration.className = "duration";
  duration.textContent = `総所要時間: 約${String(model.planMinutes)}分`;
  card.appendChild(duration);
  const durationNote = document.createElement("p");
  durationNote.className = "duration-note";
  durationNote.textContent = "一般道での帰着までを含み、休憩は含みません";
  card.appendChild(durationNote);
  const items: string[] = [
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
  const button = document.createElement("button");
  button.textContent = "出発する（Google マップを開く）";
  card.appendChild(button);
  return card;
}

/**
 * 1 試行分の結果を DOM に描く。候補が無い試行でも status 文言を必ず描く
 * （初回候補表示の計測対象は「候補または status 文言の描画」）。
 */
function renderTrialResult(
  pattern: BenchPattern,
  result: SearchResult | null,
  errorCode: string | null,
  timedOut: boolean,
): void {
  if (timedOut) {
    el.results.replaceChildren();
    el.status.textContent = errorMessage("TIMEOUT");
    return;
  }
  if (errorCode !== null) {
    el.results.replaceChildren();
    el.status.textContent = errorMessage(errorCode);
    return;
  }
  if (result === null) {
    el.results.replaceChildren();
    el.status.textContent = "結果を取得できませんでした";
    return;
  }
  el.status.textContent = statusMessage(result, pattern.time.minMinutes, pattern.time.maxMinutes);
  const fragment = document.createDocumentFragment();
  for (const candidate of result.candidates.slice(0, 3)) {
    fragment.appendChild(renderCard(toCardModel(candidate)));
  }
  el.results.replaceChildren(fragment);
}

interface TrialOutcome {
  result: SearchResult | null;
  bench: BenchPayload | null;
  errorCode: string | null;
  timedOut: boolean;
  tTransferMs: number | null;
  mainMemoryBefore: number | null;
  mainMemoryAfter: number | null;
}

/** 1 試行を実行する（Worker 生成 → 応答待ち → 描画 → 二重 rAF → terminate）。 */
async function runTrial(
  pattern: BenchPattern,
  cache: "cold" | "warm",
  repeat: number,
  nonce: string,
): Promise<BenchTrial> {
  const mainMemoryBefore = sampleHeapMiB();
  const workerCreatedEpoch = epochNow();
  const worker = new Worker(new URL("../worker/search-worker.ts", import.meta.url), {
    type: "module",
    name: BENCH_WORKER_NAME,
  });

  let readyEpoch: number | null = null;
  let outcome: TrialOutcome | null = null;
  let settle!: () => void;
  const settled = new Promise<void>((resolve) => {
    settle = resolve;
  });
  worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
    const msg = event.data;
    if (msg.type === "ready") {
      readyEpoch = epochNow();
      return;
    }
    if (msg.type === "result") {
      outcome = {
        result: msg.result,
        bench: msg.bench ?? null,
        errorCode: null,
        timedOut: false,
        tTransferMs: readyEpoch === null ? null : readyEpoch - workerCreatedEpoch,
        mainMemoryBefore,
        mainMemoryAfter: null,
      };
      settle();
      return;
    }
    outcome = {
      result: null,
      bench: null,
      errorCode: msg.code,
      timedOut: false,
      tTransferMs: readyEpoch === null ? null : readyEpoch - workerCreatedEpoch,
      mainMemoryBefore,
      mainMemoryAfter: null,
    };
    settle();
  };
  worker.onerror = () => {
    outcome = {
      result: null,
      bench: null,
      errorCode: "WORKER_ERROR",
      timedOut: false,
      tTransferMs: readyEpoch === null ? null : readyEpoch - workerCreatedEpoch,
      mainMemoryBefore,
      mainMemoryAfter: null,
    };
    settle();
  };

  const timeoutId = setTimeout(() => {
    outcome = {
      result: null,
      bench: null,
      errorCode: null,
      timedOut: true,
      tTransferMs: readyEpoch === null ? null : readyEpoch - workerCreatedEpoch,
      mainMemoryBefore,
      mainMemoryAfter: null,
    };
    worker.terminate();
    settle();
  }, SEARCH_TIMEOUT_MS);

  const message: UiSearchMessage = {
    type: "search",
    requestId: `bench-${String(pattern.index)}-${cache}-${String(repeat)}`,
    releaseId: RELEASE_ID,
    pricingAt: new Date().toISOString(),
    origin: { lat: pattern.origin.lat, lon: pattern.origin.lon },
    minMinutes: pattern.time.minMinutes,
    maxMinutes: pattern.time.maxMinutes,
    vehicleProfile: VEHICLE_PROFILE,
    bench: cache === "cold" ? { cacheBust: nonce } : {},
  };
  worker.postMessage(message);

  try {
    await settled;
    clearTimeout(timeoutId);
    const final: TrialOutcome = outcome ?? {
      result: null,
      bench: null,
      errorCode: null,
      timedOut: true,
      tTransferMs: null,
      mainMemoryBefore,
      mainMemoryAfter: null,
    };
    renderTrialResult(pattern, final.result, final.errorCode, final.timedOut);
    // paint を含めるため二重 requestAnimationFrame を挟んでから確定する。
    await doubleRaf();
    const paintedEpoch = epochNow();
    final.mainMemoryAfter = sampleHeapMiB();
    worker.terminate();

    const bench = final.bench;
    const marks = bench?.marks ?? null;
    const tSearchMs =
      marks !== null && marks.searchStartEpochMs !== null && marks.searchEndEpochMs !== null
        ? marks.searchEndEpochMs - marks.searchStartEpochMs
        : null;
    const memorySamples = [
      final.mainMemoryBefore,
      final.mainMemoryAfter,
      bench?.memory?.beforeMiB ?? null,
      bench?.memory?.afterMiB ?? null,
    ].filter((value): value is number => value !== null);
    const memoryPeakMiB = memorySamples.length === 0 ? null : Math.max(...memorySamples);

    return {
      patternIndex: pattern.index,
      patternId: pattern.id,
      originId: pattern.origin.id,
      origin: { lat: pattern.origin.lat, lon: pattern.origin.lon },
      minMinutes: pattern.time.minMinutes,
      maxMinutes: pattern.time.maxMinutes,
      cache,
      repeat,
      tTransferMs: final.tTransferMs,
      tSearchMs,
      tFirstCandidateMs: paintedEpoch - workerCreatedEpoch,
      timeout: final.timedOut,
      resultStatus: final.result?.status ?? null,
      reason: final.result?.reason ?? null,
      candidateCount: final.result?.candidates.length ?? null,
      errorCode: final.errorCode,
      memoryPeakMiB,
      memorySource: memoryPeakMiB === null ? null : "performance.memory",
      resources: bench?.resources ?? [],
    };
  } finally {
    clearTimeout(timeoutId);
    worker.terminate();
  }
}

function formatMs(value: number | null): string {
  return value === null ? "-" : `${value.toFixed(0)} ms`;
}

function formatBytes(value: number | null): string {
  return value === null ? "-" : `${(value / 1024).toFixed(1)} KiB`;
}

function formatPercent(value: number): string {
  return `${(value * 100).toFixed(1)} %`;
}

function groupRow(label: string, group: BenchAggregate["overall"]): HTMLTableRowElement {
  const tr = document.createElement("tr");
  const cells = [
    label,
    String(group.trialCount),
    formatPercent(group.timeoutRate),
    formatMs(group.tSearch.p50),
    formatMs(group.tSearch.p95),
    formatMs(group.tFirstCandidate.p50),
    formatMs(group.tFirstCandidate.p95),
    formatBytes(group.coldTransferBytes),
    group.memoryPeakMiB === null ? "-" : `${group.memoryPeakMiB.toFixed(1)} MiB`,
  ];
  for (const text of cells) {
    const td = document.createElement("td");
    td.textContent = text;
    tr.appendChild(td);
  }
  return tr;
}

function renderSummary(agg: BenchAggregate, verdicts: JudgeVerdict[]): void {
  const table = document.createElement("table");
  table.className = "bench-table";
  const head = document.createElement("thead");
  const headRow = document.createElement("tr");
  for (const label of [
    "条件",
    "試行",
    "10 秒到達率",
    "探索 p50",
    "探索 p95",
    "初回候補 p50",
    "初回候補 p95",
    "cold 転送量最大",
    "メモリ最大",
  ]) {
    const th = document.createElement("th");
    th.textContent = label;
    headRow.appendChild(th);
  }
  head.appendChild(headRow);
  table.appendChild(head);
  const body = document.createElement("tbody");
  body.appendChild(groupRow("全体", agg.overall));
  body.appendChild(groupRow("cold", agg.cold));
  body.appendChild(groupRow("warm", agg.warm));
  for (const group of agg.byPattern) {
    body.appendChild(groupRow(group.label, group));
  }
  table.appendChild(body);
  el.summary.replaceChildren(table);

  const list = document.createElement("ul");
  for (const verdict of verdicts) {
    const li = document.createElement("li");
    const mark = verdict.verdict === "pass" ? "PASS" : verdict.verdict === "fail" ? "FAIL" : "UNKNOWN";
    li.textContent = `[${mark}] ${verdict.label}: 目標 ${verdict.target.toLocaleString("ja-JP")} / 実測 ${
      verdict.actual === null ? "計測不能" : verdict.actual.toFixed(1)
    }`;
    li.className = `verdict-${verdict.verdict}`;
    list.appendChild(li);
  }
  el.verdicts.replaceChildren(list);
}

function buildCurrentEnvelope(): BenchEnvelope {
  return buildEnvelope({
    releaseId: RELEASE_ID,
    createdAt: new Date().toISOString(),
    device: readDevice(),
    memoryManualMiB: readManualMemory(),
    pageLoad: capturePageLoad(),
    targets: BENCH_TARGETS,
    trials,
  });
}

/** 現時点の試行から envelope を作り、画面・ダウンロード・window へ反映する。 */
function publish(): void {
  const envelope = buildCurrentEnvelope();
  window.__benchResult = envelope;
  const agg = aggregate(envelope);
  renderSummary(agg, judge(agg, envelope.targets));

  const json = JSON.stringify(envelope, null, 2);
  el.json.textContent = json;
  if (objectUrl !== null) {
    URL.revokeObjectURL(objectUrl);
  }
  objectUrl = URL.createObjectURL(new Blob([json], { type: "application/json" }));
  el.download.href = objectUrl;

  const validation = validateEnvelope(envelope);
  el.validate.textContent = validation.valid
    ? "envelope: 検証 OK"
    : `envelope 検証 NG: ${validation.errors.join(" / ")}`;
}

/**
 * envelope を R2 に保存する。計測自体の成否とは独立で、失敗しても例外を外へ出さない
 * （呼び出し側の計測完了処理を止めないため）。メッセージは画面と console.error に出す。
 */
async function uploadResult(envelope: BenchEnvelope): Promise<void> {
  if (uploading) {
    return;
  }
  if (envelope.trials.length === 0) {
    el.uploadStatus.textContent = "計測結果が無いため保存できません";
    return;
  }
  uploading = true;
  el.uploadBtn.disabled = true;
  el.uploadStatus.textContent = "R2 に保存中…";
  try {
    const response = await fetch("/api/bench-result", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(envelope),
    });
    if (!response.ok) {
      throw new Error(`HTTP ${String(response.status)}`);
    }
    const data = (await response.json()) as { key?: unknown };
    el.uploadStatus.textContent =
      typeof data.key === "string" ? `R2 に保存しました: ${data.key}` : "R2 に保存しました";
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    el.uploadStatus.textContent = `R2 への保存に失敗しました: ${message}`;
    console.error("bench result upload failed", err);
  } finally {
    uploading = false;
    el.uploadBtn.disabled = false;
  }
}

/** 手入力欄の変更で envelope を作り直す（手入力メモリの反映先を増やすため）。 */
function republishOnManualInput(): void {
  for (const input of MANUAL_INPUTS) {
    input.addEventListener("change", () => {
      if (trials.length > 0 && !running) {
        publish();
      }
    });
  }
}

async function runAll(): Promise<void> {
  if (running) {
    return;
  }
  const params = new URLSearchParams(window.location.search);
  const patterns = selectPatterns(params.get("patterns"));
  if (patterns.length === 0) {
    // 範囲外・不正な `?patterns=` は全件へフォールバックせず、開始せずに理由を出す。
    el.status.textContent = "patterns の指定が不正です（0〜29 の整数をカンマ区切りで指定してください）";
    return;
  }
  running = true;
  el.start.disabled = true;
  // 計測失敗後も部分結果を手動保存できるよう、終了時に必ず有効化する。
  el.uploadBtn.disabled = true;
  const repeatsParam = Number(params.get("repeats"));
  const repeats =
    Number.isInteger(repeatsParam) && repeatsParam > 0 ? repeatsParam : DEFAULT_REPEATS;
  trials = [];
  const total = patterns.length * repeats * 2;
  // 自動保存は明示オプトイン（`?upload=1`）のときだけ行う。Playwright 代理計測では
  // 既定で 90 コンテキストを回すため、無条件送信はレート制限（429）と console.error で
  // 計測成果物を汚染する。手動保存ボタンは常に残す。
  const autoUpload = params.get("upload") === "1";
  try {
    el.progress.textContent = "warm 計測の前提（キャッシュ載せ）を準備中…";
    await primeArtifactCache();
    let done = 0;
    for (const pattern of patterns) {
      for (const cache of ["cold", "warm"] as const) {
        for (let repeat = 1; repeat <= repeats; repeat++) {
          el.progress.textContent = `${String(done + 1)} / ${String(total)}: ${pattern.label} (${cache} ${String(repeat)}/${String(repeats)})`;
          trials.push(await runTrial(pattern, cache, repeat, newNonce()));
          done += 1;
          publish();
        }
      }
    }
    el.status.textContent = `完了: ${String(trials.length)} 試行`;
    el.progress.textContent = "";
    // 手入力欄の値も反映した最終 envelope を確定させる。
    publish();
    console.log("BENCH_RESULT", JSON.stringify(window.__benchResult ?? null));
    if (autoUpload) {
      // 保存の成否は計測結果に影響させない（失敗しても __benchDone は立てる）。
      try {
        await uploadResult(window.__benchResult ?? buildCurrentEnvelope());
      } catch (err) {
        console.error("bench result upload failed", err);
      }
    }
    window.__benchDone = true;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    el.status.textContent = `計測が失敗しました: ${message}`;
    window.__benchError = message;
  } finally {
    running = false;
    el.start.disabled = false;
    el.uploadBtn.disabled = false;
  }
}

el.start.addEventListener("click", () => {
  void runAll();
});
el.uploadBtn.addEventListener("click", () => {
  void uploadResult(window.__benchResult ?? buildCurrentEnvelope());
});
el.uploadBtn.disabled = true;
republishOnManualInput();
el.status.textContent = "待機中（計測開始を押すか、?auto=1 で自動開始）";

const autoStart = new URLSearchParams(window.location.search).get("auto") === "1";
if (autoStart) {
  void runAll();
}
