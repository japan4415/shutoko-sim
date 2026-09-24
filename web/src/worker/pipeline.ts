// 探索パイプラインの純粋関数群。Worker 本体（search-worker.ts）はこれらを配線するだけ。
// fetch と import は引数注入にし、Vitest からモックで決定論的に検証できるようにする。

import { KNOWN_RELEASES } from "./artifact-hashes";
import type {
  BenchPayload,
  GraphDocument,
  RoutePlanSegmentRole,
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

/**
 * prepare 時に渡す入口アクセス距離の上界（m）。
 *
 * 導出: 片道アクセスの往復だけで最大 240 分を使い切る直線距離は
 * `240*60/2*(30/3.6)/1.3 ≒ 46 153.8 m`（30 km/h・迂回係数 1.3）。cap を 46 000 m に
 * 置くと、これより遠い地点は「アクセス往復だけで製品上限に届く」ため、cap が
 * 時間窓で成立し得る候補を隠すことはない。真の到達判定はエンジン側の
 * 時間窓（`T_plan <= U*60`）が行い、46 km を超える地点はエンジンが
 * `NO_CONNECTION` + `nearestAccess` を返して UI が距離と条件を提示する。
 * Rust 既定値（30,000 m）は変更しない。
 */
export const MAX_ACCESS_DISTANCE_METERS = 46_000;

/** `prepare` の第 2 引数へ渡す SearchLimits JSON。 */
export const SEARCH_LIMITS_JSON = JSON.stringify({
  maxAccessDistanceMeters: MAX_ACCESS_DISTANCE_METERS,
});
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

export const SUPPORTED_GRAPH_SCHEMA_VERSIONS = [2, 3, 4] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function graphContractMismatch(message: string): PipelineError {
  return new PipelineError("ARTIFACT_MISMATCH", message);
}

function validateSchema4BillingPair(value: unknown, index: number): void {
  if (!isRecord(value) || (value.pairKind !== "legacyRing" && value.pairKind !== "radialReturn")) {
    throw graphContractMismatch(`graph.json: billingPairs[${String(index)}] の pairKind が不正`);
  }
  if (value.pairKind === "legacyRing") {
    if (
      !isRecord(value.anchor) ||
      value.anchor.anchorKind !== "sameNode" ||
      !Array.isArray(value.entryToAnchorEdgeIds) ||
      !Array.isArray(value.anchorToExitEdgeIds)
    ) {
      throw graphContractMismatch(`graph.json: billingPairs[${String(index)}] の legacyRing contract が不正`);
    }
  } else if (
    value.routePlanVersion !== 1 ||
    !isRecord(value.routePlan) ||
    !isRecord(value.routePlan.anchor) ||
    value.routePlan.anchor.anchorKind !== "directedJunction" ||
    !Array.isArray(value.resolvedRouteSegments)
  ) {
    throw graphContractMismatch(`graph.json: billingPairs[${String(index)}] の radialReturn contract が不正`);
  }
}

export function parseGraphDocument(graphJson: string): GraphDocument {
  let value: unknown;
  try {
    value = JSON.parse(graphJson);
  } catch {
    throw graphContractMismatch("graph.json: JSON デコード失敗");
  }
  if (!isRecord(value)) {
    throw graphContractMismatch("graph.json: top-level がオブジェクトではありません");
  }
  const schemaVersion = value.schemaVersion;
  if (
    typeof schemaVersion !== "number" ||
    !SUPPORTED_GRAPH_SCHEMA_VERSIONS.includes(schemaVersion as 2 | 3 | 4)
  ) {
    throw graphContractMismatch("graph.json: schemaVersion が 2 / 3 / 4 以外です");
  }
  if (
    typeof value.releaseId !== "string" ||
    typeof value.vehicleProfile !== "string" ||
    !Array.isArray(value.nodes) ||
    !Array.isArray(value.edges) ||
    !Array.isArray(value.billingPairs)
  ) {
    throw graphContractMismatch("graph.json: 必須 field が欠落または型違いです");
  }
  if (schemaVersion === 4) {
    if (!Array.isArray(value.routeMemberships)) {
      throw graphContractMismatch("graph.json: schema 4 の routeMemberships がありません");
    }
    value.billingPairs.forEach(validateSchema4BillingPair);
  } else if (value.routeMemberships !== undefined) {
    throw graphContractMismatch("graph.json: schema 2 / 3 は routeMemberships を持ちません");
  } else {
    value.billingPairs.forEach((pair, index) => {
      if (isRecord(pair) && pair.pairKind !== undefined) {
        throw graphContractMismatch(
          `graph.json: billingPairs[${String(index)}] の pairKind は schema 4 専用です`,
        );
      }
    });
  }
  return value as unknown as GraphDocument;
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

/** WasmPreparedGraph の最小インターフェース（free() を持つ）。 */
export interface WasmPreparedGraphLike {
  free(): void;
}

/** JS glue（wasm-bindgen --target web）が公開する最小の形。 */
export interface WasmGlueModule {
  default (params: { module_or_path: Uint8Array | ArrayBuffer }): Promise<unknown>;
  prepare (graphJson: string, limitsJson: string): WasmPreparedGraphLike;
  searchPrepared (pg: WasmPreparedGraphLike, requestJson: string): string;
  search? (graphJson: string, requestJson: string, limitsJson: string): string;
}

export interface LoadedRelease {
  readonly preparedGraph: WasmPreparedGraphLike;
  searchPrepared(requestJson: string): string;
  free(): void;
  retain(): void;
  release(): void;
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
 * index.d.ts の公開フィールドのみを含め、`type` は含めない。
 */
export function buildSearchRequest(msg: UiSearchMessage): SearchRequest {
  return {
    requestId: msg.requestId,
    releaseId: msg.releaseId,
    ...(msg.originNodeId !== undefined ? { originNodeId: msg.originNodeId } : {}),
    ...(msg.origin !== undefined ? { origin: msg.origin } : {}),
    ...(msg.entryRampId !== undefined ? { entryRampId: msg.entryRampId } : {}),
    ...(msg.exitRampId !== undefined ? { exitRampId: msg.exitRampId } : {}),
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
 * 取得・照合し、WASM を初期化・グラフを prepare して検索境界を返す。
 *
 * - graph.json の期待ハッシュは manifest の artifacts から取る
 * - wasm / glue の期待ハッシュは配信側 engine.json から取る（固定定数は持たない）
 * - engine.json の取得失敗は FETCH_FAILED、形式不正・照合不一致は ARTIFACT_MISMATCH で停止
 *   （以降の fetch は呼ばれない）
 * - glue は text 取得して照合後、同じ URL を importImpl で import し、
 *   init({ module_or_path: wasmBytes }) で初期化する
 * - 初期化後、prepare(graphJson, SEARCH_LIMITS_JSON) を即座に実行して WasmPreparedGraph を構築し、
 *   graphJson は保持せず V8 GC 対象にする（limits は接続判定の上界だけで、成立判定は時間窓）
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
  parseGraphDocument(graphJson);

  const wasmUrl = `${base}/${WASM_ARTIFACT_PATH}${query}`;
  const wasmBytes = await fetchBytes(fetchImpl, wasmUrl, signal);
  await verifyOrStop(wasmUrl, wasmBytes, wasmExpected);

  const glueUrl = `${base}/${GLUE_ARTIFACT_PATH}${query}`;
  const glueText = await fetchText(fetchImpl, glueUrl, signal);
  await verifyOrStop(glueUrl, new TextEncoder().encode(glueText), glueExpected);
  const glue = await importImpl(glueUrl);

  await glue.default({ module_or_path: wasmBytes });

  const preparedGraph = glue.prepare(graphJson, SEARCH_LIMITS_JSON);

  let refCount = 1;
  let freed = false;
  const release: LoadedRelease = {
    preparedGraph,
    searchPrepared(requestJson: string): string {
      if (freed) {
        throw new PipelineError(
          "USE_AFTER_FREE",
          `LoadedRelease for ${releaseId} has already been freed`,
        );
      }
      return glue.searchPrepared(preparedGraph, requestJson);
    },
    retain(): void {
      if (freed) {
        throw new PipelineError(
          "USE_AFTER_FREE",
          `LoadedRelease for ${releaseId} has already been freed`,
        );
      }
      refCount += 1;
    },
    release(): void {
      refCount -= 1;
      if (refCount <= 0 && !freed) {
        freed = true;
        preparedGraph.free();
      }
    },
    free(): void {
      release.release();
    },
  };
  return release;
}

/** 探索結果が実行時契約に合わないときの error.code（UI の既存エラー導線へ出す）。 */
export const RESULT_CONTRACT_MISMATCH = "RESULT_CONTRACT_MISMATCH";

function contractMismatch(message: string): PipelineError {
  return new PipelineError(RESULT_CONTRACT_MISMATCH, message);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

/** 有限かつ非負（距離・秒数は負にならない）。 */
function assertFiniteNonNegative(value: unknown, label: string): void {
  if (!isFiniteNumber(value) || value < 0) {
    throw contractMismatch(`${label} が有限の非負数値ではありません（${String(value)}）`);
  }
}

/**
 * 必須診断フィールド（nearestAccess / minPlanSeconds）をオブジェクトとして検証する。
 *
 * 旧エンジンが新 SearchResult の診断フィールドを欠落させると `undefined` になり、
 * UI の `!== null` ガードをすり抜けて「約 NaN 分」や実行時例外を招く。エンジン成果物は
 * engine.json の sha256 で照合しているが、契約そのものの不整合は型では守れないため、
 * ここで実行時検証して明確な契約不一致として検出する（値の型・有限性・非負を確認）。
 */
function assertDiagnostics(result: Record<string, unknown>): void {
  if (!("nearestAccess" in result) || result.nearestAccess === undefined) {
    throw contractMismatch("nearestAccess フィールドがありません（旧エンジンの探索結果）");
  }
  const nearest = result.nearestAccess;
  if (nearest !== null) {
    if (typeof nearest !== "object" || Array.isArray(nearest)) {
      throw contractMismatch("nearestAccess がオブジェクトでも null でもありません");
    }
    const snapped = nearest as Record<string, unknown>;
    if (typeof snapped.nodeId !== "string") {
      throw contractMismatch("nearestAccess.nodeId が文字列ではありません");
    }
    if (!isFiniteNumber(snapped.lat) || !isFiniteNumber(snapped.lon)) {
      throw contractMismatch("nearestAccess.lat/lon が有限の数値ではありません");
    }
    assertFiniteNonNegative(snapped.distanceMeters, "nearestAccess.distanceMeters");
  }

  if (!("minPlanSeconds" in result) || result.minPlanSeconds === undefined) {
    throw contractMismatch("minPlanSeconds フィールドがありません（旧エンジンの探索結果）");
  }
  const minPlanSeconds = result.minPlanSeconds;
  if (minPlanSeconds !== null) {
    assertFiniteNonNegative(minPlanSeconds, "minPlanSeconds");
  }
}

const ROUTE_ROLES: RoutePlanSegmentRole[] = [
  "entry_approach",
  "mandatory_lap",
  "return_corridor",
  "exit_approach",
];

function assertStringArray(value: unknown, label: string): asserts value is string[] {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string" || item.length === 0)) {
    throw contractMismatch(`${label} が非空文字列配列ではありません`);
  }
}

function assertSha256(value: unknown, label: string): asserts value is string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    throw contractMismatch(`${label} が lowercase SHA-256 ではありません`);
  }
}

function isNonNegativeSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}

function validateEstimatedLegsAndTotals(candidate: Record<string, unknown>, label: string): void {
  const estimated = candidate.estimatedLegs;
  if (!Array.isArray(estimated) || estimated.length !== 2) {
    throw contractMismatch(`${label} estimatedLegs が 2 件ではありません`);
  }
  const expectedRoles = ["surface_access", "surface_return"];
  const allowedFields = new Set(["role", "estimated", "distanceMeters", "durationSeconds"]);
  let surfaceDistance = 0;
  for (let index = 0; index < estimated.length; index += 1) {
    const leg = estimated[index];
    if (
      !isRecord(leg) ||
      Object.keys(leg).length !== allowedFields.size ||
      Object.keys(leg).some((field) => !allowedFields.has(field)) ||
      leg.role !== expectedRoles[index] ||
      leg.estimated !== true ||
      !isNonNegativeSafeInteger(leg.distanceMeters) ||
      !isNonNegativeSafeInteger(leg.durationSeconds) ||
      (leg.distanceMeters === 0) !== (leg.durationSeconds === 0)
    ) {
      throw contractMismatch(`${label} estimatedLegs[${String(index)}] が不正です`);
    }
    surfaceDistance += leg.distanceMeters;
  }
  const duration = candidate.duration;
  if (
    !isRecord(duration) ||
    !isNonNegativeSafeInteger(duration.accessSeconds) ||
    !isNonNegativeSafeInteger(duration.shutokoSeconds) ||
    !isNonNegativeSafeInteger(duration.returnSeconds) ||
    !isNonNegativeSafeInteger(duration.baseSeconds) ||
    !isNonNegativeSafeInteger(duration.bufferSeconds) ||
    !isNonNegativeSafeInteger(duration.planSeconds)
  ) {
    throw contractMismatch(`${label} duration が不正です`);
  }
  const baseSeconds = duration.accessSeconds + duration.shutokoSeconds + duration.returnSeconds;
  const bufferSeconds = Math.max(300, Math.ceil(baseSeconds / 5));
  if (
    duration.accessSeconds !== estimated[0].durationSeconds ||
    duration.returnSeconds !== estimated[1].durationSeconds ||
    duration.baseSeconds !== baseSeconds ||
    duration.bufferSeconds !== bufferSeconds ||
    duration.planSeconds !== baseSeconds + bufferSeconds
  ) {
    throw contractMismatch(`${label} duration と estimatedLegs が一致しません`);
  }
  if (
    !isNonNegativeSafeInteger(candidate.distanceMeters) ||
    !isNonNegativeSafeInteger(candidate.shutokoDistanceMeters) ||
    !Number.isSafeInteger(surfaceDistance)
  ) {
    throw contractMismatch(`${label} distance が不正です`);
  }
  if (candidate.distanceMeters !== candidate.shutokoDistanceMeters + surfaceDistance) {
    throw contractMismatch(`${label} distanceMeters の式が一致しません`);
  }
}

function validateTariffStatus(
  candidate: Record<string, unknown>,
  label: string,
  unpricedBillingMustBeNull: boolean,
): void {
  const toll = candidate.toll;
  const amountYen = isRecord(toll) ? toll.amountYen : undefined;
  const amountValid = amountYen === null || isNonNegativeSafeInteger(amountYen);
  if (
    !isRecord(toll) ||
    !["priced", "unpriced", "expired", "not_applicable"].includes(String(candidate.tariffStatus)) ||
    !amountValid ||
    (candidate.tariffStatus === "priced" ? amountYen === null : amountYen !== null) ||
    (unpricedBillingMustBeNull &&
      candidate.tariffStatus === "unpriced" &&
      toll.billingDistanceMeters != null)
  ) {
    throw contractMismatch(`${label} tariffStatus が toll と一致しません`);
  }
}

function validateTopologyOnlyCandidate(candidate: Record<string, unknown>): void {
  if (
    candidate.eligibilityStatus !== "topology_only" ||
    candidate.loopValidationStatus !== "topology_only" ||
    !isRecord(candidate.loop) ||
    candidate.loop.validated !== false ||
    "anchor" in candidate ||
    "routePlan" in candidate ||
    "edgeRouteLegs" in candidate ||
    !isRecord(candidate.toll) ||
    "chargedSectionCount" in candidate.toll ||
    !isRecord(candidate.handoff) ||
    typeof candidate.handoff.mapsUrl !== "string" ||
    !Array.isArray(candidate.reasons) ||
    !candidate.reasons.includes("TOPOLOGY_ONLY") ||
    candidate.reasons.some(
      (reason) => reason === "ONE_SECTION_TOLL" || reason === "BEST_TIME_PER_YEN" || reason === "BEST_SHUTOKO_TIME",
    )
  ) {
    throw contractMismatch("topologyOnly candidate の status / loop / toll / reasons が不正です");
  }
  validateTariffStatus(candidate, "topologyOnly", false);
  validateEstimatedLegsAndTotals(candidate, "topologyOnly");
}

async function validateRadialCandidate(candidate: unknown): Promise<void> {
  if (
    !isRecord(candidate) ||
    candidate.routePlanVersion !== 1 ||
    !isRecord(candidate.anchor) ||
    candidate.anchor.anchorKind !== "directedJunction" ||
    !isRecord(candidate.routePlan) ||
    !Array.isArray(candidate.edgeRouteLegs) ||
    candidate.edgeRouteLegs.length !== ROUTE_ROLES.length
  ) {
    throw contractMismatch("radialReturn candidate の anchor / routePlan / edgeRouteLegs が不正です");
  }
  assertStringArray(candidate.edgeIds, "radialReturn candidate.edgeIds");
  validateEstimatedLegsAndTotals(candidate, "radialReturn");
  const productEligible =
    candidate.eligibilityStatus === "verified_one_section_ahead" &&
    candidate.loopValidationStatus === "declared_route_validated";
  if (
    !["verified_one_section_ahead", "unverified", "topology_only"].includes(
      String(candidate.eligibilityStatus),
    ) ||
    !["declared_route_validated", "unresolved", "topology_only"].includes(
      String(candidate.loopValidationStatus),
    ) ||
    !Array.isArray(candidate.reasons) ||
    candidate.reasons.includes("ONE_SECTION_TOLL") ||
    (!productEligible &&
      candidate.reasons.some(
        (reason) => reason === "BEST_TIME_PER_YEN" || reason === "BEST_SHUTOKO_TIME",
      ))
  ) {
    throw contractMismatch("radialReturn candidate reasons が不正です");
  }
  assertStringArray(candidate.routePlan.membershipIds, "radialReturn routePlan.membershipIds");
  const resolved = candidate.routePlan.resolvedRouteSegments;
  if (!Array.isArray(resolved) || resolved.length !== ROUTE_ROLES.length) {
    throw contractMismatch("radialReturn resolvedRouteSegments は 4 件である必要があります");
  }
  const resolvedById = new Map<string, Record<string, unknown>>();
  for (let index = 0; index < resolved.length; index += 1) {
    const segment = resolved[index];
    if (
      !isRecord(segment) ||
      segment.role !== ROUTE_ROLES[index] ||
      typeof segment.resolvedSegmentId !== "string" ||
      resolvedById.has(segment.resolvedSegmentId) ||
      typeof segment.membershipId !== "string" ||
      !candidate.routePlan.membershipIds.includes(segment.membershipId)
    ) {
      throw contractMismatch("radialReturn resolvedRouteSegments の role / 参照が不正です");
    }
    assertStringArray(segment.sourceSegmentIds, "resolvedRouteSegment.sourceSegmentIds");
    assertSha256(segment.edgeIdsSha256, "resolvedRouteSegment.edgeIdsSha256");
    resolvedById.set(segment.resolvedSegmentId, segment);
  }
  let nextIndex = 0;
  const covered = new Set<number>();
  for (let index = 0; index < candidate.edgeRouteLegs.length; index += 1) {
    const leg = candidate.edgeRouteLegs[index];
    if (!isRecord(leg) || leg.role !== ROUTE_ROLES[index]) {
      throw contractMismatch("edgeRouteLegs の role が不正です");
    }
    const segment = resolvedById.get(String(leg.resolvedSegmentId));
    if (
      segment === undefined ||
      segment.role !== leg.role ||
      !Number.isInteger(leg.startEdgeIndex) ||
      typeof leg.startEdgeIndex !== "number" ||
      leg.startEdgeIndex !== nextIndex ||
      !Number.isInteger(leg.endEdgeIndexExclusive) ||
      typeof leg.endEdgeIndexExclusive !== "number" ||
      leg.endEdgeIndexExclusive <= leg.startEdgeIndex ||
      leg.endEdgeIndexExclusive > candidate.edgeIds.length
    ) {
      throw contractMismatch("edgeRouteLegs に重複・欠落があります");
    }
    const expectedHash = segment.edgeIdsSha256;
    assertSha256(expectedHash, "resolvedRouteSegment.edgeIdsSha256");
    const bytes = new TextEncoder().encode(
      JSON.stringify(candidate.edgeIds.slice(leg.startEdgeIndex, leg.endEdgeIndexExclusive)),
    );
    const actualHash = await hexDigest(bytes.buffer as ArrayBuffer);
    if (actualHash !== expectedHash) {
      throw contractMismatch("edgeRouteLegs の edgeIds スライスと hash が一致しません");
    }
    for (let edgeIndex = leg.startEdgeIndex; edgeIndex < leg.endEdgeIndexExclusive; edgeIndex += 1) {
      if (covered.has(edgeIndex)) {
        throw contractMismatch("edgeRouteLegs に重複があります");
      }
      covered.add(edgeIndex);
    }
    nextIndex = leg.endEdgeIndexExclusive;
  }
  if (nextIndex !== candidate.edgeIds.length || covered.size !== candidate.edgeIds.length) {
    throw contractMismatch("edgeRouteLegs が edgeIds を完全分割していません");
  }
  if (!isRecord(candidate.toll) || "chargedSectionCount" in candidate.toll) {
    throw contractMismatch("radialReturn toll に chargedSectionCount があります");
  }
  validateTariffStatus(candidate, "radialReturn", true);
  if (!isRecord(candidate.handoff) || candidate.handoff.enabled !== false) {
    throw contractMismatch("radialReturn handoff が無効ではありません");
  }
}

/**
 * search() の戻り値 JSON 文字列を SearchResult に展開する。
 * 必須診断フィールドを実行時検証し、欠落・型違いは RESULT_CONTRACT_MISMATCH で停止する
 * （http レイヤの ARTIFACT_MISMATCH と同様、部分データを UI に流さない）。
 */
export async function parseSearchResult(resultJson: string): Promise<SearchResult> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(resultJson);
  } catch {
    throw contractMismatch("探索結果 JSON のデコードに失敗しました");
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw contractMismatch("探索結果がオブジェクトではありません");
  }
  const result = parsed as Record<string, unknown>;
  if (!Array.isArray(result.candidates)) {
    throw contractMismatch("candidates が配列ではありません");
  }
  if (typeof result.status !== "string") {
    throw contractMismatch("status が文字列ではありません");
  }
  if (typeof result.reason !== "string" && result.reason !== null) {
    throw contractMismatch("reason が文字列でも null でもありません");
  }
  assertDiagnostics(result);
  for (const value of result.candidates) {
    if (!isRecord(value)) {
      throw contractMismatch("candidate がオブジェクトではありません");
    }
    if (value.pairKind === "radialReturn") {
      await validateRadialCandidate(value);
    } else if (value.pairKind === "topologyOnly") {
      validateTopologyOnlyCandidate(value);
    } else if (value.pairKind !== undefined && value.pairKind !== "legacyRing") {
      throw contractMismatch("candidate.pairKind が未知です");
    }
  }
  return parsed as SearchResult;
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

export interface ReleaseStoreOptions {
  fetchImpl?: FetchLike;
  importImpl?: ImportLike;
  knownReleases?: readonly string[];
  onLoadStart?: (releaseId: string) => void;
  onLoadEnd?: (releaseId: string) => void;
}

/**
 * リリースのロード・キャッシュ・世代管理・参照解放を一元管理するストア。
 * - releaseId のロードを重複排除
 * - 世代管理（latestGeneration）により、古いリクエストの遅延完了で新しい loaded キャッシュが上書きされるのを防止
 * - 呼び出し側が acquire() すると retain() され、利用中は release() されるまで決して free() されない
 * - 二重 free() の排除と use-after-free の数学的防止
 */
export class ReleaseStore {
  private loaded: { releaseId: string; state: LoadedRelease; generation: number } | null = null;
  private latestGeneration = 0;
  private readonly inFlightLoads = new Map<string, Promise<LoadedRelease>>();
  private readonly inFlightControllers = new Map<string, AbortController>();
  private disposed = false;
  private readonly fetchImpl: FetchLike;
  private readonly importImpl?: ImportLike;
  private readonly knownReleases: readonly string[];
  private readonly onLoadStart?: (releaseId: string) => void;
  private readonly onLoadEnd?: (releaseId: string) => void;

  constructor(options: ReleaseStoreOptions = {}) {
    this.fetchImpl =
      options.fetchImpl ??
      (typeof fetch !== "undefined"
        ? fetch
        : async () => {
            throw new Error("fetch is not available");
          });
    this.importImpl = options.importImpl;
    this.knownReleases = options.knownReleases ?? KNOWN_RELEASES;
    this.onLoadStart = options.onLoadStart;
    this.onLoadEnd = options.onLoadEnd;
  }

  get currentLoaded(): { releaseId: string; state: LoadedRelease } | null {
    if (this.loaded === null) return null;
    return { releaseId: this.loaded.releaseId, state: this.loaded.state };
  }

  /**
   * releaseId の LoadedRelease を確保し、呼び出し元のために retain() して返す。
   * 呼び出し側は使い終わったら必ず state.release() を呼ぶこと。
   */
  async acquire(releaseId: string, cacheBust?: string): Promise<LoadedRelease> {
    if (this.disposed) {
      throw new PipelineError("DISPOSED", "ReleaseStore has been disposed");
    }

    if (!this.knownReleases.includes(releaseId)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `未知の releaseId です: ${releaseId}`);
    }

    if (this.loaded !== null && this.loaded.releaseId === releaseId) {
      this.loaded.state.retain();
      return this.loaded.state;
    }

    const generation = ++this.latestGeneration;

    let inFlight = this.inFlightLoads.get(releaseId);
    if (inFlight === undefined) {
      if (this.onLoadStart !== undefined) {
        this.onLoadStart(releaseId);
      }
      const controller = new AbortController();
      this.inFlightControllers.set(releaseId, controller);
      const loadPromise = loadRelease(this.fetchImpl, releaseId, this.importImpl, controller.signal, {
        cacheBust,
      })
        .then((state) => {
          this.inFlightLoads.delete(releaseId);
          this.inFlightControllers.delete(releaseId);
          if (this.disposed) {
            state.release(); // dispose 済みのためキャッシュに載せずその場で解放
            throw new PipelineError("DISPOSED", `ReleaseStore was disposed while loading ${releaseId}`);
          }
          if (this.onLoadEnd !== undefined) {
            this.onLoadEnd(releaseId);
          }
          if (this.loaded === null || generation >= this.loaded.generation) {
            const oldLoaded = this.loaded;
            state.retain(); // loaded キャッシュ用の参照 (+1)
            this.loaded = { releaseId, state, generation };
            if (oldLoaded !== null) {
              oldLoaded.state.release(); // 旧 loaded キャッシュの解放 (-1)
            }
          }
          return state; // 初期 refCount=1 をこの最初の caller が保持
        })
        .catch((err: unknown) => {
          this.inFlightLoads.delete(releaseId);
          this.inFlightControllers.delete(releaseId);
          throw err;
        });

      this.inFlightLoads.set(releaseId, loadPromise);
      return loadPromise;
    } else {
      // 進行中の同一 releaseId ロードに相乗りする 2人目以降の caller
      return inFlight.then((state) => {
        if (this.disposed) {
          throw new PipelineError("DISPOSED", `ReleaseStore was disposed while loading ${releaseId}`);
        }
        state.retain();
        return state;
      });
    }
  }

  dispose(): void {
    this.disposed = true;
    if (this.loaded !== null) {
      this.loaded.state.release();
      this.loaded = null;
    }
    for (const controller of this.inFlightControllers.values()) {
      controller.abort();
    }
    this.inFlightControllers.clear();
    this.inFlightLoads.clear();
  }
}
