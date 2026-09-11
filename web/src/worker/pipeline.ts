// 探索パイプラインの純粋関数群。Worker 本体（search-worker.ts）はこれらを配線するだけ。
// fetch と import は引数注入にし、Vitest からモックで決定論的に検証できるようにする。
import type {
  BenchPayload,
  SearchResult,
  SearchRequest,
  UiSearchMessage,
  WorkerResponse,
} from "./types";

/** 成果物 1 件の期待値（sha256 hex 小文字 / バイト長）。 */
export interface ArtifactExpectation {
  sha256: string;
  byteLength: number;
}

/** 配信側 engine.json のスキーマ版。 */
export const ENGINE_SCHEMA_VERSION = 1;
/** engine.json で期待値を持つ成果物（照合対象）。 */
export const WASM_ARTIFACT_PATH = "shutoko_routing_bg.wasm";
export const GLUE_ARTIFACT_PATH = "shutoko_routing.js";

/** パイプライン停止理由。code は UI へそのまま error メッセージで返す。 */
export class PipelineError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

/** fetch の最小互換（テストでは同一形状のモックを注入する）。 */
export interface FetchResponseLike {
  ok: boolean;
  status?: number;
  text(): Promise<string>;
  arrayBuffer(): Promise<ArrayBuffer>;
}

export type FetchLike = (url: string, init?: { signal?: AbortSignal }) => Promise<FetchResponseLike>;
export type ImportLike = (url: string) => Promise<WasmGlueModule>;

/** JS glue（wasm-bindgen --target web）が公開する最小の形。 */
export interface WasmGlueModule {
  default (params: { module_or_path: Uint8Array | ArrayBuffer }): Promise<unknown>;
  search (graphJson: string, requestJson: string, limitsJson: string): string;
}

export interface LoadedRelease {
  graphJson: string;
  search (graphJson: string, requestJson: string, limitsJson: string): string;
}

/** ArrayBuffer から SHA-256 の小文字 hex を返す。 */
export async function hexDigest(buf: ArrayBuffer): Promise<string> {
  const digest: ArrayBuffer = await crypto.subtle.digest("SHA-256", buf);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/** バイト長と SHA-256 の両方が一致するか。 */
export async function verifyArtifact(
  bytes: Uint8Array,
  expected: ArtifactExpectation,
): Promise<boolean> {
  if (bytes.byteLength !== expected.byteLength) {
    return false;
  }
  const hex = await hexDigest(new Uint8Array(bytes).buffer as ArrayBuffer);
  return hex === expected.sha256.toLowerCase();
}

/**
 * UI メッセージから SearchRequest JSON のオブジェクトを作る。
 * index.d.ts の 8 フィールド（requestId, releaseId, originNodeId, origin,
 * minMinutes, maxMinutes, vehicleProfile, pricingAt）のみを含め、`type` は含めない。
 */
export function buildSearchRequest(msg: UiSearchMessage): SearchRequest {
  return {
    requestId: msg.requestId,
    releaseId: msg.releaseId,
    ...(msg.originNodeId !== undefined ? { originNodeId: msg.originNodeId } : {}),
    ...(msg.origin !== undefined ? { origin: msg.origin } : {}),
    minMinutes: msg.minMinutes,
    maxMinutes: msg.maxMinutes,
    vehicleProfile: msg.vehicleProfile,
    pricingAt: msg.pricingAt,
  };
}

/**
 * WASM 境界が throw した Error を展開する。
 * message は RoutingErrorPayload JSON（`{ code, message }`）を想定し、
 * code をそのまま UI の error に載せる（INVALID_INPUT 等）。
 */
export function parseWasmError(err: unknown): { code: string; message: string } {
  const raw = err instanceof Error ? err.message : String(err);
  try {
    const payload: unknown = JSON.parse(raw);
    if (
      payload !== null &&
      typeof payload === "object" &&
      typeof (payload as { code?: unknown }).code === "string"
    ) {
      const p = payload as { code: string; message?: unknown };
      return { code: p.code, message: typeof p.message === "string" ? p.message : "" };
    }
  } catch {
    // JSON でない message は下記のフォールバックで扱う
  }
  return { code: "WASM_ERROR", message: raw };
}

async function fetchText(fetchImpl: FetchLike, url: string, signal?: AbortSignal): Promise<string> {
  let res: FetchResponseLike;
  try {
    res = await fetchImpl(url, { signal });
  } catch (err) {
    throw new PipelineError("FETCH_FAILED", `${url}: ${err instanceof Error ? err.message : String(err)}`);
  }
  if (!res.ok) {
    throw new PipelineError("FETCH_FAILED", `${url}: HTTP ${String(res.status ?? "?")}`);
  }
  return await res.text();
}

async function fetchBytes(fetchImpl: FetchLike, url: string, signal?: AbortSignal): Promise<Uint8Array> {
  let res: FetchResponseLike;
  try {
    res = await fetchImpl(url, { signal });
  } catch (err) {
    throw new PipelineError("FETCH_FAILED", `${url}: ${err instanceof Error ? err.message : String(err)}`);
  }
  if (!res.ok) {
    throw new PipelineError("FETCH_FAILED", `${url}: HTTP ${String(res.status ?? "?")}`);
  }
  return new Uint8Array(await res.arrayBuffer());
}

async function verifyOrStop(name: string, bytes: Uint8Array, expected: ArtifactExpectation): Promise<void> {
  if (!(await verifyArtifact(bytes, expected))) {
    throw new PipelineError(
      "ARTIFACT_MISMATCH",
      `${name}: sha256/byteLength が期待値と不一致（expected ${expected.sha256}/${String(expected.byteLength)}）`,
    );
  }
}

interface ManifestLike {
  artifacts?: { path: string; sha256: string; byteLength: number }[];
}

/** 配信側 engine.json の形。期待値は投入時に実ファイルから計算される。 */
interface EngineLike {
  schemaVersion?: unknown;
  releaseId?: unknown;
  artifacts?: { path?: unknown; sha256?: unknown; byteLength?: unknown }[];
}

/**
 * engine.json（配信側が実ファイルから計算した期待値）から 1 件分の期待値を取り出す。
 * 形式不正（エントリ欠落・型違い）は ARTIFACT_MISMATCH で停止する。
 */
function engineExpectation(engine: EngineLike, path: string, engineUrl: string): ArtifactExpectation {
  const entry = (engine.artifacts ?? []).find((a) => a.path === path);
  if (
    entry === undefined ||
    typeof entry.sha256 !== "string" ||
    typeof entry.byteLength !== "number"
  ) {
    throw new PipelineError(
      "ARTIFACT_MISMATCH",
      `${engineUrl}: artifacts に ${path} の有効なエントリが無い`,
    );
  }
  return { sha256: entry.sha256, byteLength: entry.byteLength };
}

/** loadRelease の任意オプション（計測ページ専用。通常 UI は指定しない）。 */
export interface LoadReleaseOptions {
  /**
   * 成果物 URL に付けるキャッシュ回避クエリ（`?bench=<nonce>`）。
   * 配信側ルータ（workers/src/index.ts の getRawPath）はクエリを落とすため
   * 同一成果物が返り、sha256 照合もバイト列に対して行われるので壊れない。
   * 省略時は URL を一切変えない（通常経路の挙動は不変）。
   */
  cacheBust?: string;
}

/**
 * 1 リリース分の成果物を manifest → engine.json → graph.json → wasm → glue の順で
 * 取得・照合し、WASM を初期化して検索境界を返す。
 *
 * - graph.json の期待ハッシュは manifest の artifacts から取る
 * - wasm / glue の期待ハッシュは配信側 engine.json から取る（固定定数は持たない）
 * - engine.json の取得失敗は FETCH_FAILED、形式不正・照合不一致は ARTIFACT_MISMATCH で停止
 *   （以降の fetch は呼ばれない）
 * - glue は text 取得して照合後、同じ URL を importImpl で import し、
 *   init({ module_or_path: wasmBytes }) で初期化する
 */
export async function loadRelease(
  fetchImpl: FetchLike,
  releaseId: string,
  importImpl: ImportLike = (url: string) => import(url),
  signal?: AbortSignal,
  options: LoadReleaseOptions = {},
): Promise<LoadedRelease> {
  const base = `/releases/${encodeURIComponent(releaseId)}`;
  // cacheBust 未指定なら空文字（既存の URL と完全に同一）。
  const query =
    options.cacheBust === undefined || options.cacheBust === ""
      ? ""
      : `?bench=${encodeURIComponent(options.cacheBust)}`;

  const manifestUrl = `${base}/manifest.json${query}`;
  const manifestText = await fetchText(fetchImpl, manifestUrl, signal);
  let manifest: ManifestLike;
  try {
    manifest = JSON.parse(manifestText) as ManifestLike;
  } catch {
    throw new PipelineError("ARTIFACT_MISMATCH", `${manifestUrl}: JSON デコード失敗`);
  }
  const graphEntry = (manifest.artifacts ?? []).find((a) => a.path === "graph.json");
  if (!graphEntry) {
    throw new PipelineError("ARTIFACT_MISMATCH", `${manifestUrl}: artifacts に graph.json 無し`);
  }

  const engineUrl = `${base}/engine.json${query}`;
  const engineText = await fetchText(fetchImpl, engineUrl, signal);
  let engine: EngineLike;
  try {
    engine = JSON.parse(engineText) as EngineLike;
  } catch {
    throw new PipelineError("ARTIFACT_MISMATCH", `${engineUrl}: JSON デコード失敗`);
  }
  if (
    engine === null ||
    typeof engine !== "object" ||
    engine.schemaVersion !== ENGINE_SCHEMA_VERSION ||
    engine.releaseId !== releaseId
  ) {
    throw new PipelineError(
      "ARTIFACT_MISMATCH",
      `${engineUrl}: schemaVersion/releaseId が不正（expected ${String(ENGINE_SCHEMA_VERSION)}/${releaseId}）`,
    );
  }
  const wasmExpected = engineExpectation(engine, WASM_ARTIFACT_PATH, engineUrl);
  const glueExpected = engineExpectation(engine, GLUE_ARTIFACT_PATH, engineUrl);

  const graphUrl = `${base}/graph.json${query}`;
  const graphBytes = await fetchBytes(fetchImpl, graphUrl, signal);
  await verifyOrStop(graphUrl, graphBytes, graphEntry);
  const graphJson = new TextDecoder().decode(graphBytes);

  const wasmUrl = `${base}/${WASM_ARTIFACT_PATH}${query}`;
  const wasmBytes = await fetchBytes(fetchImpl, wasmUrl, signal);
  await verifyOrStop(wasmUrl, wasmBytes, wasmExpected);

  const glueUrl = `${base}/${GLUE_ARTIFACT_PATH}${query}`;
  const glueText = await fetchText(fetchImpl, glueUrl, signal);
  await verifyOrStop(glueUrl, new TextEncoder().encode(glueText), glueExpected);
  const glue = await importImpl(glueUrl);

  await glue.default({ module_or_path: wasmBytes });

  return { graphJson, search: glue.search };
}

/** search() の戻り値 JSON 文字列を SearchResult に展開する。 */
export function parseSearchResult(resultJson: string): SearchResult {
  return JSON.parse(resultJson) as SearchResult;
}

/**
 * `result` 応答を組み立てる。
 * `bench` が undefined のときは `bench` キー自体を付けない（通常 UI の応答形を変えない）。
 */
export function buildResultResponse(
  requestId: string,
  result: SearchResult,
  bench?: BenchPayload,
): WorkerResponse {
  return {
    type: "result",
    requestId,
    result,
    ...(bench === undefined ? {} : { bench }),
  };
}
