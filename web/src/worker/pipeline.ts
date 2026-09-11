// 探索パイプラインの純粋関数群。Worker 本体（search-worker.ts）はこれらを配線するだけ。
// fetch と import は引数注入にし、Vitest からモックで決定論的に検証できるようにする。
import type { ArtifactExpectation, ReleaseArtifactHashes } from "./artifact-hashes";
import type { SearchResult, SearchRequest, UiSearchMessage } from "./types";

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

/**
 * 1 リリース分の成果物を manifest → graph → wasm → glue の順で取得・照合し、
 * WASM を初期化して検索境界を返す。
 *
 * - graph.json の期待ハッシュは manifest の artifacts から取る
 * - wasm / glue は Worker 側固定ハッシュ（artifact-hashes.ts）と照合
 * - 照合不一致は PipelineError("ARTIFACT_MISMATCH") で停止（以降の fetch は呼ばれない）
 * - glue は text 取得して照合後、同じ URL を importImpl で import し、
 *   init({ module_or_path: wasmBytes }) で初期化する
 */
export async function loadRelease(
  fetchImpl: FetchLike,
  releaseId: string,
  hashes: ReleaseArtifactHashes,
  importImpl: ImportLike = (url: string) => import(url),
  signal?: AbortSignal,
): Promise<LoadedRelease> {
  const base = `/releases/${encodeURIComponent(releaseId)}`;

  const manifestText = await fetchText(fetchImpl, `${base}/manifest.json`, signal);
  let manifest: ManifestLike;
  try {
    manifest = JSON.parse(manifestText) as ManifestLike;
  } catch {
    throw new PipelineError("ARTIFACT_MISMATCH", `${base}/manifest.json: JSON デコード失敗`);
  }
  const graphEntry = (manifest.artifacts ?? []).find((a) => a.path === "graph.json");
  if (!graphEntry) {
    throw new PipelineError("ARTIFACT_MISMATCH", `${base}/manifest.json: artifacts に graph.json 無し`);
  }

  const graphBytes = await fetchBytes(fetchImpl, `${base}/graph.json`, signal);
  await verifyOrStop(`${base}/graph.json`, graphBytes, graphEntry);
  const graphJson = new TextDecoder().decode(graphBytes);

  const wasmBytes = await fetchBytes(fetchImpl, `${base}/shutoko_routing_bg.wasm`, signal);
  await verifyOrStop(`${base}/shutoko_routing_bg.wasm`, wasmBytes, hashes.wasm);

  const glueUrl = `${base}/shutoko_routing.js`;
  const glueText = await fetchText(fetchImpl, glueUrl, signal);
  await verifyOrStop(glueUrl, new TextEncoder().encode(glueText), hashes.glue);
  const glue = await importImpl(glueUrl);

  await glue.default({ module_or_path: wasmBytes });

  return { graphJson, search: glue.search };
}

/** search() の戻り値 JSON 文字列を SearchResult に展開する。 */
export function parseSearchResult(resultJson: string): SearchResult {
  return JSON.parse(resultJson) as SearchResult;
}
