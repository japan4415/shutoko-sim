export interface LatLng {
  lat: number;
  lon: number;
}

export interface SearchRequest {
  requestId: string;
  releaseId: string;
  originNodeId?: string;
  origin?: LatLng;
  minMinutes: number;
  maxMinutes: number;
  vehicleProfile: string;
  pricingAt: string;
}

export interface SearchLimits {
  maxExpandedStates?: number;
  beamWidth?: number;
  maxLoopEdges?: number;
  maxLocalEdges?: number;
  maxPairs?: number;
  maxCandidates?: number;
}

export interface SnappedOrigin {
  nodeId: string;
  lat: number;
  lon: number;
  distanceMeters: number;
}

export interface RampInfo {
  edgeId: string;
  name: string | null;
}

export interface GeoJsonLineString {
  type: "LineString";
  coordinates: [number, number][]; // [lon, lat]
}

export interface Duration {
  accessSeconds: number;
  shutokoSeconds: number;
  returnSeconds: number;
  baseSeconds: number;
  bufferSeconds: number;
  planSeconds: number;
}

export interface Toll {
  billingPairId: string;
  chargedSectionCount: number;
  amountYen: number | null;
  pricingAt: string;
  effectiveFrom: string | null;
  effectiveTo: string | null;
}

export interface Loop {
  anchorNodeId: string;
  edgeIds: string[];
  durationSeconds: number;
  distanceMeters: number;
  validated: boolean;
}

export interface Handoff {
  origin: LatLng;
  destination: LatLng;
  waypoints: LatLng[];
  mapsUrl: string;
  verificationSetVersion: string | null;
}

export interface Candidate {
  id: string;
  releaseId: string;
  origin: LatLng | null;
  originNodeId: string;
  snappedOrigin: SnappedOrigin;
  entry: RampInfo;
  exit: RampInfo;
  entryId: string;
  exitId: string;
  roadNames: string[];
  edgeIds: string[];
  geometry: GeoJsonLineString;
  duration: Duration;
  distanceMeters: number;
  shutokoDistanceMeters: number;
  toll: Toll;
  loop: Loop;
  reasons: string[];
  /** Warning codes: "HANDOFF_WAYPOINTS_UNVERIFIED", "STATIC_TRAVEL_TIME" */
  warnings: string[];
  handoff: Handoff;
}

export interface SearchResult {
  requestId: string;
  releaseId: string;
  status: "ok" | "no_candidates" | "truncated";
  reason: string | null;
  rankingMode: "shutoko_time" | "time_per_yen" | string;
  expandedStates: number;
  candidates: Candidate[];
}

export interface RoutingErrorPayload {
  code: string;
  message: string;
}

/**
 * Execute experimental route search on a prevalidated graph JSON.
 *
 * Convenience API — parses all three JSON arguments and rebuilds the index on
 * every call.  For repeated searches on the same graph, prefer `prepare` +
 * `searchPrepared` to avoid rebuilding the index each time.
 *
 * @param graphJson Serialized Graph JSON string
 * @param requestJson Serialized SearchRequest JSON string
 * @param limitsJson Serialized SearchLimits JSON string (empty object "{}" for defaults)
 * @returns Serialized SearchResult JSON string
 * @throws JavaScript Error with JSON serialized RoutingErrorPayload as its message on invalid input
 */
export function search(
  graphJson: string,
  requestJson: string,
  limitsJson: string
): string;

/**
 * An opaque handle wrapping a PreparedGraph.
 *
 * Obtain via `prepare()`; pass to `searchPrepared()` for fast repeated search
 * without rebuilding the index on every call.
 *
 * Call `.free()` when done to release Wasm memory.
 */
export class WasmPreparedGraph {
  free(): void;
}

/**
 * Build a prepared graph from JSON strings (the expensive, one-time step).
 *
 * @param graphJson  Serialized Graph JSON string
 * @param limitsJson Serialized SearchLimits JSON string (use "{}" for defaults)
 * @returns An opaque WasmPreparedGraph handle
 * @throws JavaScript Error with JSON serialized RoutingErrorPayload on invalid input
 */
export function prepare(graphJson: string, limitsJson: string): WasmPreparedGraph;

/**
 * Execute a route search on an already-prepared graph (fast, per-search call).
 *
 * Skips index rebuilding; only validates the request and runs the search
 * algorithm.
 *
 * @param pg          A WasmPreparedGraph handle obtained from `prepare`
 * @param requestJson Serialized SearchRequest JSON string
 * @returns Serialized SearchResult JSON string
 * @throws JavaScript Error with JSON serialized RoutingErrorPayload on invalid input
 */
export function searchPrepared(pg: WasmPreparedGraph, requestJson: string): string;

export default function init(module_or_path?: unknown): Promise<unknown>;
