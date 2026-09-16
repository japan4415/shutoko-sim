// ランプ台帳（ramps.json）および端点能力（manifest.json）の検証・選択・絞り込みロジック。
// DOM を触らず純粋関数として Vitest から決定論的に検証できる。

import {
  PipelineError,
  verifyArtifact,
  type FetchLike,
  type FetchResponseLike,
} from "../worker/pipeline";

export type RampKind = "general_entry" | "general_exit" | "boundary_in" | "boundary_out";
export type RampStatus = "active" | "closed";
export type SupportState = "verified_bound" | "unsupported" | "not_routable";
export type RoutingCapability = "routable" | "structural_no_loop" | "unsupported" | "not_routable";

export interface RampItem {
  id: string;
  facilityId: string;
  name: string;
  route: string;
  direction: string;
  kind: RampKind;
  lat: number;
  lon: number;
  status: RampStatus;
  supportState: SupportState;
  supportReason: string;
  routingCapability: RoutingCapability;
  routingCapabilityReason: string;
  bound: boolean;
  edgeId?: string;
  nodeId?: string;
  mainlineNodeId?: string;
}

export interface EndpointCapabilities {
  routableEntryCount: number;
  routableExitCount: number;
  structuralNoLoopEntryCount: number;
  structuralNoLoopExitCount: number;
  routableEntryRampIds: string[];
  routableExitRampIds: string[];
  structuralNoLoopEntryRampIds?: string[];
  structuralNoLoopExitRampIds?: string[];
}

export interface ManifestArtifactEntry {
  path: string;
  sha256: string;
  byteLength: number;
}

export interface ManifestData {
  schemaVersion: number;
  releaseId: string;
  artifacts?: ManifestArtifactEntry[];
  coverage?: {
    endpointCapabilities?: EndpointCapabilities;
    [key: string]: unknown;
  };
  [key: string]: unknown;
}

export interface RampsArtifact {
  schemaVersion: number;
  releaseId: string;
  sourceDate?: string;
  totalRamps?: number;
  boundRamps?: number;
  ramps: RampItem[];
}

export interface RampsDataset {
  manifest: ManifestData;
  capabilities: EndpointCapabilities;
  ramps: RampItem[];
  rampMap: Map<string, RampItem>;
}

export type EligibilityCategory =
  | "routable"
  | "structural_no_loop"
  | "unsupported"
  | "boundary"
  | "closed"
  | "wrong_kind";

export interface RampEligibility {
  selectable: boolean;
  statusLabel: string;
  reason: string;
  category: EligibilityCategory;
}

export interface FilterRampsResult {
  items: Array<{
    ramp: RampItem;
    eligibility: RampEligibility;
  }>;
  totalCount: number;
  selectableCount: number;
  matchedCount: number;
  matchedSelectableCount: number;
}

/** 路線コード → 日本語表記（路線名付き）。 */
export const ROUTE_NAMES: Record<string, string> = {
  C1: "C1 都心環状線",
  C2: "C2 中央環状線",
  "1H": "1号 羽田線",
  "1U": "1号 上野線",
  "2": "2号 目黒線",
  "3": "3号 渋谷線",
  "4": "4号 新宿線",
  "5": "5号 池袋線",
  "6M": "6号 向島線",
  "6S": "6号 三郷線",
  "7": "7号 小松川線",
  "9": "9号 深川線",
  "10": "10号 晴海線",
  "11": "11号 台場線",
  B: "B 湾岸線",
  K1: "K1 横羽線",
  K2: "K2 三ツ沢線",
  K3: "K3 狩場線",
  K5: "K5 大黒線",
  K6: "K6 川崎線",
  K7: "K7 横浜北線・北西線",
  S1: "S1 川口線",
  S2: "S2 埼玉新都心線",
  S5: "S5 埼玉大宮線",
  Y: "Y 八重洲線",
};

/** 方向コード → 日本語表記。 */
export const DIRECTION_NAMES: Record<string, string> = {
  inner: "内回り",
  outer: "外回り",
  inbound: "上り",
  outbound: "下り",
  west: "西行き",
  east: "東行き",
  north: "北行き",
  south: "南行き",
};

/** 首都高の標準路線順序（一覧表示用）。 */
export const ROUTE_ORDER = [
  "C1",
  "C2",
  "Y",
  "1H",
  "1U",
  "2",
  "3",
  "4",
  "5",
  "6M",
  "6S",
  "7",
  "9",
  "10",
  "11",
  "B",
  "K1",
  "K2",
  "K3",
  "K5",
  "K6",
  "K7",
  "S1",
  "S2",
  "S5",
];

/** 方向の標準順序。 */
export const DIRECTION_ORDER = [
  "inner",
  "outer",
  "inbound",
  "outbound",
  "east",
  "west",
  "north",
  "south",
];

export function formatRoute(route: string): string {
  return ROUTE_NAMES[route] ?? route;
}

export function formatDirection(direction: string): string {
  return DIRECTION_NAMES[direction] ?? direction;
}

const ALLOWED_KINDS = new Set<RampKind>([
  "general_entry",
  "general_exit",
  "boundary_in",
  "boundary_out",
]);
const ALLOWED_STATUSES = new Set<RampStatus>(["active", "closed"]);
const ALLOWED_SUPPORT_STATES = new Set<SupportState>([
  "verified_bound",
  "unsupported",
  "not_routable",
]);
const ALLOWED_CAPABILITIES = new Set<RoutingCapability>([
  "routable",
  "structural_no_loop",
  "unsupported",
  "not_routable",
]);

/**
 * ランプ台帳オブジェクトの整合性を検証し、不正があれば PipelineError を投げる（fail closed）。
 */
export function validateRampsArtifact(
  raw: unknown,
  capabilities?: EndpointCapabilities,
  expectedReleaseId?: string,
): RampItem[] {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw new PipelineError("ARTIFACT_MISMATCH", "ramps.json がオブジェクトではありません");
  }

  const obj = raw as Record<string, unknown>;
  if (obj.schemaVersion !== 1) {
    throw new PipelineError(
      "ARTIFACT_MISMATCH",
      `ramps.json schemaVersion が不正です（expected 1, got ${String(obj.schemaVersion)}）`,
    );
  }

  if (typeof obj.releaseId !== "string") {
    throw new PipelineError("ARTIFACT_MISMATCH", "ramps.json releaseId が文字列ではありません");
  }

  if (expectedReleaseId !== undefined && obj.releaseId !== expectedReleaseId) {
    throw new PipelineError(
      "ARTIFACT_MISMATCH",
      `ramps.json releaseId 不一致（expected ${expectedReleaseId}, got ${obj.releaseId}）`,
    );
  }

  if (!Array.isArray(obj.ramps) || obj.ramps.length === 0) {
    throw new PipelineError("ARTIFACT_MISMATCH", "ramps.json ramps 配列が空または存在しません");
  }

  const seenIds = new Set<string>();
  const validatedRamps: RampItem[] = [];

  for (let i = 0; i < obj.ramps.length; i++) {
    const item = obj.ramps[i];
    if (item === null || typeof item !== "object" || Array.isArray(item)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json ramps[${i}] がオブジェクトではありません`);
    }

    const r = item as Record<string, unknown>;
    if (typeof r.id !== "string" || r.id === "") {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json ramps[${i}].id が不正です`);
    }
    if (seenIds.has(r.id)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json 重複 ID: ${r.id}`);
    }
    seenIds.add(r.id);

    if (typeof r.facilityId !== "string" || typeof r.name !== "string" || typeof r.route !== "string" || typeof r.direction !== "string") {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json ramps[${i}] の基本属性が不正です`);
    }

    if (!ALLOWED_KINDS.has(r.kind as RampKind)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json 未知の kind: ${String(r.kind)} (${r.id})`);
    }
    if (!ALLOWED_STATUSES.has(r.status as RampStatus)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json 未知の status: ${String(r.status)} (${r.id})`);
    }
    if (!ALLOWED_SUPPORT_STATES.has(r.supportState as SupportState)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json 未知の supportState: ${String(r.supportState)} (${r.id})`);
    }
    if (!ALLOWED_CAPABILITIES.has(r.routingCapability as RoutingCapability)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json 未知の routingCapability: ${String(r.routingCapability)} (${r.id})`);
    }
    if (typeof r.lat !== "number" || !Number.isFinite(r.lat) || typeof r.lon !== "number" || !Number.isFinite(r.lon)) {
      throw new PipelineError("ARTIFACT_MISMATCH", `ramps.json ramps[${i}] 座標が有限数値ではありません (${r.id})`);
    }

    validatedRamps.push({
      id: r.id,
      facilityId: r.facilityId,
      name: r.name,
      route: r.route,
      direction: r.direction,
      kind: r.kind as RampKind,
      lat: r.lat,
      lon: r.lon,
      status: r.status as RampStatus,
      supportState: r.supportState as SupportState,
      supportReason: typeof r.supportReason === "string" ? r.supportReason : "",
      routingCapability: r.routingCapability as RoutingCapability,
      routingCapabilityReason: typeof r.routingCapabilityReason === "string" ? r.routingCapabilityReason : "",
      bound: Boolean(r.bound),
      edgeId: typeof r.edgeId === "string" ? r.edgeId : undefined,
      nodeId: typeof r.nodeId === "string" ? r.nodeId : undefined,
      mainlineNodeId: typeof r.mainlineNodeId === "string" ? r.mainlineNodeId : undefined,
    });
  }

  // manifest.coverage.endpointCapabilities との整合性確認
  if (capabilities !== undefined) {
    const rampMap = new Map(validatedRamps.map((item) => [item.id, item]));

    let routableEntryCount = 0;
    let routableExitCount = 0;
    let structuralNoLoopEntryCount = 0;
    let structuralNoLoopExitCount = 0;

    for (const item of validatedRamps) {
      if (item.kind === "general_entry") {
        if (item.routingCapability === "routable") routableEntryCount++;
        else if (item.routingCapability === "structural_no_loop") structuralNoLoopEntryCount++;
      } else if (item.kind === "general_exit") {
        if (item.routingCapability === "routable") routableExitCount++;
        else if (item.routingCapability === "structural_no_loop") structuralNoLoopExitCount++;
      }
    }

    if (routableEntryCount !== capabilities.routableEntryCount) {
      throw new PipelineError(
        "ARTIFACT_MISMATCH",
        `routableEntryCount 不整合（expected ${capabilities.routableEntryCount}, got ${routableEntryCount}）`,
      );
    }
    if (routableExitCount !== capabilities.routableExitCount) {
      throw new PipelineError(
        "ARTIFACT_MISMATCH",
        `routableExitCount 不整合（expected ${capabilities.routableExitCount}, got ${routableExitCount}）`,
      );
    }
    if (structuralNoLoopEntryCount !== capabilities.structuralNoLoopEntryCount) {
      throw new PipelineError(
        "ARTIFACT_MISMATCH",
        `structuralNoLoopEntryCount 不整合（expected ${capabilities.structuralNoLoopEntryCount}, got ${structuralNoLoopEntryCount}）`,
      );
    }
    if (structuralNoLoopExitCount !== capabilities.structuralNoLoopExitCount) {
      throw new PipelineError(
        "ARTIFACT_MISMATCH",
        `structuralNoLoopExitCount 不整合（expected ${capabilities.structuralNoLoopExitCount}, got ${structuralNoLoopExitCount}）`,
      );
    }

    for (const id of capabilities.routableEntryRampIds) {
      const r = rampMap.get(id);
      if (!r || r.kind !== "general_entry" || r.routingCapability !== "routable") {
        throw new PipelineError("ARTIFACT_MISMATCH", `manifest routableEntryRampIds [${id}] が台帳と不一致`);
      }
    }
    for (const id of capabilities.routableExitRampIds) {
      const r = rampMap.get(id);
      if (!r || r.kind !== "general_exit" || r.routingCapability !== "routable") {
        throw new PipelineError("ARTIFACT_MISMATCH", `manifest routableExitRampIds [${id}] が台帳と不一致`);
      }
    }
    if (capabilities.structuralNoLoopEntryRampIds) {
      for (const id of capabilities.structuralNoLoopEntryRampIds) {
        const r = rampMap.get(id);
        if (!r || r.kind !== "general_entry" || r.routingCapability !== "structural_no_loop") {
          throw new PipelineError("ARTIFACT_MISMATCH", `manifest structuralNoLoopEntryRampIds [${id}] が台帳と不一致`);
        }
      }
    }
    if (capabilities.structuralNoLoopExitRampIds) {
      for (const id of capabilities.structuralNoLoopExitRampIds) {
        const r = rampMap.get(id);
        if (!r || r.kind !== "general_exit" || r.routingCapability !== "structural_no_loop") {
          throw new PipelineError("ARTIFACT_MISMATCH", `manifest structuralNoLoopExitRampIds [${id}] が台帳と不一致`);
        }
      }
    }
  }

  return validatedRamps;
}

/**
 * 入口／出口としての選択可否・状態ラベル・理由を判定する。
 */
export function getRampEligibility(ramp: RampItem, role: "entry" | "exit"): RampEligibility {
  if (role === "entry") {
    if (ramp.kind !== "general_entry") {
      if (ramp.kind === "general_exit") {
        return {
          selectable: false,
          statusLabel: "出口専用",
          reason: "出口専用ランプのため入口としては選択できません。",
          category: "wrong_kind",
        };
      }
      return {
        selectable: false,
        statusLabel: "境界JCT",
        reason: ramp.supportReason || "高速道路境界JCTのため一般入口としては選択できません。",
        category: "boundary",
      };
    }
    if (ramp.status === "closed") {
      const note = ramp.supportReason ? `（${ramp.supportReason}）` : "";
      return {
        selectable: false,
        statusLabel: "閉鎖済み",
        reason: `閉鎖済み施設のため選択できません${note}。`,
        category: "closed",
      };
    }
    if (ramp.routingCapability === "structural_no_loop") {
      return {
        selectable: false,
        statusLabel: "周回不可",
        reason: ramp.routingCapabilityReason || "有向実グラフ上で5km以上の循環SCCへ接続できないため選択できません（構造的NO_LOOP）。",
        category: "structural_no_loop",
      };
    }
    if (ramp.routingCapability === "unsupported") {
      const note = ramp.supportReason || ramp.routingCapabilityReason || "OSM segmentが未監査またはグラフ外のため未対応です。";
      return {
        selectable: false,
        statusLabel: "未対応",
        reason: `未対応: ${note}`,
        category: "unsupported",
      };
    }
    if (ramp.routingCapability === "routable") {
      return {
        selectable: true,
        statusLabel: "選択可能",
        reason: "",
        category: "routable",
      };
    }
    return {
      selectable: false,
      statusLabel: "非対応",
      reason: ramp.supportReason || "選択できません。",
      category: "unsupported",
    };
  }

  // role === "exit"
  if (ramp.kind !== "general_exit") {
    if (ramp.kind === "general_entry") {
      return {
        selectable: false,
        statusLabel: "入口専用",
        reason: "入口専用ランプのため出口としては選択できません。",
        category: "wrong_kind",
      };
    }
    return {
      selectable: false,
      statusLabel: "境界JCT",
      reason: ramp.supportReason || "高速道路境界JCTのため一般出口としては選択できません。",
      category: "boundary",
    };
  }
  if (ramp.status === "closed") {
    const note = ramp.supportReason ? `（${ramp.supportReason}）` : "";
    return {
      selectable: false,
      statusLabel: "閉鎖済み",
      reason: `閉鎖済み施設のため選択できません${note}。`,
      category: "closed",
    };
  }
  if (ramp.routingCapability === "structural_no_loop") {
    return {
      selectable: false,
      statusLabel: "周回不可",
      reason: ramp.routingCapabilityReason || "有向実グラフ上で5km以上の循環SCCへ接続できないため選択できません（構造的NO_LOOP）。",
      category: "structural_no_loop",
    };
  }
  if (ramp.routingCapability === "unsupported") {
    const note = ramp.supportReason || ramp.routingCapabilityReason || "OSM segmentが未監査またはグラフ外のため未対応です。";
    return {
      selectable: false,
      statusLabel: "未対応",
      reason: `未対応: ${note}`,
      category: "unsupported",
    };
  }
  if (ramp.routingCapability === "routable") {
    return {
      selectable: true,
      statusLabel: "選択可能",
      reason: "",
      category: "routable",
    };
  }
  return {
    selectable: false,
    statusLabel: "非対応",
    reason: ramp.supportReason || "選択できません。",
    category: "unsupported",
  };
}

/**
 * 検索キーワードでランプ一覧を絞り込む。
 * 施設名・路線・方向・IDのいずれかに部分一致するものを抽出する。
 * 表示順序: 選択可能（routable）を優先し、次いで路線・方向・施設名順に整列。
 */
export function filterRamps(
  ramps: RampItem[],
  query: string,
  role: "entry" | "exit",
): FilterRampsResult {
  const terms = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter((t) => t.length > 0);

  let totalCount = 0;
  let selectableCount = 0;
  let matchedCount = 0;
  let matchedSelectableCount = 0;

  const itemsWithEligibility = ramps.map((ramp) => {
    const eligibility = getRampEligibility(ramp, role);
    totalCount++;
    if (eligibility.selectable) {
      selectableCount++;
    }
    return { ramp, eligibility };
  });

  const matchedItems = itemsWithEligibility.filter(({ ramp, eligibility }) => {
    if (terms.length === 0) {
      matchedCount++;
      if (eligibility.selectable) matchedSelectableCount++;
      return true;
    }

    const searchable = [
      ramp.id,
      ramp.facilityId,
      ramp.name,
      ramp.route,
      formatRoute(ramp.route),
      ramp.direction,
      formatDirection(ramp.direction),
    ]
      .join(" ")
      .toLowerCase();

    const matches = terms.every((t) => searchable.includes(t));
    if (matches) {
      matchedCount++;
      if (eligibility.selectable) matchedSelectableCount++;
    }
    return matches;
  });

  // ソート順序:
  // 1. 選択可能（selectable === true）を先頭に
  // 2. 路線順序（ROUTE_ORDER）
  // 3. 方向順序（DIRECTION_ORDER）
  // 4. 施設名（name）五十音順
  matchedItems.sort((a, b) => {
    if (a.eligibility.selectable !== b.eligibility.selectable) {
      return a.eligibility.selectable ? -1 : 1;
    }

    const aRouteIdx = ROUTE_ORDER.indexOf(a.ramp.route);
    const bRouteIdx = ROUTE_ORDER.indexOf(b.ramp.route);
    const aRouteScore = aRouteIdx >= 0 ? aRouteIdx : 999;
    const bRouteScore = bRouteIdx >= 0 ? bRouteIdx : 999;
    if (aRouteScore !== bRouteScore) {
      return aRouteScore - bRouteScore;
    }

    const aDirIdx = DIRECTION_ORDER.indexOf(a.ramp.direction);
    const bDirIdx = DIRECTION_ORDER.indexOf(b.ramp.direction);
    const aDirScore = aDirIdx >= 0 ? aDirIdx : 999;
    const bDirScore = bDirIdx >= 0 ? bDirIdx : 999;
    if (aDirScore !== bDirScore) {
      return aDirScore - bDirScore;
    }

    return a.ramp.name.localeCompare(b.ramp.name, "ja");
  });

  return {
    items: matchedItems,
    totalCount,
    selectableCount,
    matchedCount,
    matchedSelectableCount,
  };
}

/**
 * 件数インフォメーション文言を生成する。
 */
export function formatCountInfo(result: FilterRampsResult, isFiltered: boolean): string {
  if (!isFiltered) {
    return `全 ${result.totalCount} 件（選択可能 ${result.selectableCount} 件）`;
  }
  if (result.matchedCount === 0) {
    return `該当するランプが見つかりませんでした（0 件 / 全 ${result.totalCount} 件）。別の検索キーワード（施設名・路線・方向・ID）をお試しください。`;
  }
  return `該当 ${result.matchedCount} 件（うち選択可能 ${result.matchedSelectableCount} 件）/ 全体 ${result.totalCount} 件（選択可能 ${result.selectableCount} 件）`;
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

/**
 * manifest.json と ramps.json を配信信頼境界（sha256/byteLength）で取得・照合し、
 * 正本データセットを構築する。
 */
export async function loadRampsDataset(
  fetchImpl: FetchLike,
  releaseId: string,
  signal?: AbortSignal,
  options: { cacheBust?: string } = {},
): Promise<RampsDataset> {
  const base = `/releases/${encodeURIComponent(releaseId)}`;
  const query =
    options.cacheBust === undefined || options.cacheBust === ""
      ? ""
      : `?bench=${encodeURIComponent(options.cacheBust)}`;

  const manifestUrl = `${base}/manifest.json${query}`;
  const manifestText = await fetchText(fetchImpl, manifestUrl, signal);
  let manifest: ManifestData;
  try {
    manifest = JSON.parse(manifestText) as ManifestData;
  } catch {
    throw new PipelineError("ARTIFACT_MISMATCH", `${manifestUrl}: JSON デコード失敗`);
  }

  if (manifest === null || typeof manifest !== "object" || manifest.schemaVersion !== 1 || manifest.releaseId !== releaseId) {
    throw new PipelineError("ARTIFACT_MISMATCH", `${manifestUrl}: schemaVersion/releaseId 不正`);
  }

  const rampsEntry = (manifest.artifacts ?? []).find((a) => a.path === "ramps.json");
  if (!rampsEntry) {
    throw new PipelineError("ARTIFACT_MISMATCH", `${manifestUrl}: artifacts に ramps.json 無し`);
  }

  const endpointCapabilities = manifest.coverage?.endpointCapabilities;
  if (!endpointCapabilities) {
    throw new PipelineError("ARTIFACT_MISMATCH", `${manifestUrl}: coverage.endpointCapabilities 無し`);
  }

  const rampsUrl = `${base}/ramps.json${query}`;
  const rampsBytes = await fetchBytes(fetchImpl, rampsUrl, signal);

  if (!(await verifyArtifact(rampsBytes, rampsEntry))) {
    throw new PipelineError(
      "ARTIFACT_MISMATCH",
      `${rampsUrl}: sha256/byteLength が期待値と不一致（expected ${rampsEntry.sha256}/${String(rampsEntry.byteLength)}）`,
    );
  }

  const rampsText = new TextDecoder().decode(rampsBytes);
  let rawRamps: unknown;
  try {
    rawRamps = JSON.parse(rampsText);
  } catch {
    throw new PipelineError("ARTIFACT_MISMATCH", `${rampsUrl}: JSON デコード失敗`);
  }

  const ramps = validateRampsArtifact(rawRamps, endpointCapabilities, releaseId);
  const rampMap = new Map<string, RampItem>();
  for (const r of ramps) {
    rampMap.set(r.id, r);
  }

  return {
    manifest,
    capabilities: endpointCapabilities,
    ramps,
    rampMap,
  };
}

export interface ExplicitSearchCondition {
  entryRampId: string | null;
  exitRampId: string | null;
  minMinutes: number;
  maxMinutes: number;
  origin: { lat: number; lon: number } | null;
}

/**
 * 明示OD検索の入力妥当性を判定する。
 */
export function validateExplicitSearch(
  entryRampId: string | null,
  exitRampId: string | null,
  dataset?: RampsDataset,
): { valid: boolean; error: string | null } {
  if (!entryRampId && !exitRampId) {
    return {
      valid: false,
      error: "入口と出口が未選択です。両方のランプを選択してください。",
    };
  }
  if (!entryRampId) {
    return {
      valid: false,
      error: "入口が未選択です。有効な入口ランプを選択してください。",
    };
  }
  if (!exitRampId) {
    return {
      valid: false,
      error: "出口が未選択です。有効な出口ランプを選択してください。",
    };
  }

  if (dataset !== undefined) {
    const entry = dataset.rampMap.get(entryRampId);
    if (!entry || !getRampEligibility(entry, "entry").selectable) {
      return {
        valid: false,
        error: "選択された入口ランプは現在選択できません。",
      };
    }
    const exit = dataset.rampMap.get(exitRampId);
    if (!exit || !getRampEligibility(exit, "exit").selectable) {
      return {
        valid: false,
        error: "選択された出口ランプは現在選択できません。",
      };
    }
  }

  return { valid: true, error: null };
}

/**
 * 直前の成功探索と同じ条件で再押下された場合のメッセージ。
 */
export function duplicateOperationMessage(
  current: ExplicitSearchCondition,
  last: ExplicitSearchCondition | null,
): string | null {
  if (last === null) return null;
  if (
    current.entryRampId === last.entryRampId &&
    current.exitRampId === last.exitRampId &&
    current.minMinutes === last.minMinutes &&
    current.maxMinutes === last.maxMinutes &&
    current.origin?.lat === last.origin?.lat &&
    current.origin?.lon === last.origin?.lon
  ) {
    return "現在の検索条件と同じ結果が既に表示されています。";
  }
  return null;
}
