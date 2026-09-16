export interface LatLng {
  lat: number;
  lon: number;
}

export interface SearchRequest {
  requestId: string;
  releaseId: string;
  originNodeId?: string;
  origin?: LatLng;
  entryRampId?: string;
  exitRampId?: string;
  minMinutes: number;
  maxMinutes: number;
  vehicleProfile: string;
  pricingAt: string;
}

export interface SearchLimits {
  maxExpandedStates?: number;
  beamWidth?: number;
  maxLoopEdges?: number;
  /**
   * Minimum loop distance (metres) required for a valid loop cycle.
   * Internal JCT micro-loops under this threshold are rejected. Default: 5,000m.
   */
  minLoopMeters?: number;
  /**
   * Maximum number of Entry access points to try for coordinate-input searches.
   * 0 (or omit) means unlimited — all Entry access points in the graph are candidates.
   * Default: 0 (unlimited).
   */
  maxAccessEntries?: number;
  /**
   * Maximum straight-line distance (metres) from the user's coordinate to the nearest
   * Entry access point.  If the nearest entry exceeds this distance, NO_CONNECTION is
   * returned.  0.0 (or omit) means unlimited.  Default: 30000 (30 km).
   */
  maxAccessDistanceMeters?: number;
  maxPairs?: number;
  maxCandidates?: number;
  /** Maximum number of nodes allowed in the graph. Default: 1,000,000. */
  maxGraphNodes?: number;
  /** Maximum number of edges allowed in the graph. Default: 3,000,000. */
  maxGraphEdges?: number;
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
  rampId?: string | null;
  route?: string | null;
  direction?: string | null;
}

export type RampKind =
  | "general_entry"
  | "general_exit"
  | "boundary_in"
  | "boundary_out";

export interface Ramp {
  id: string;
  facilityId: string;
  name: string;
  route: string;
  direction: string;
  kind: RampKind;
  edgeId: string;
  nodeId: string;
  mainlineNodeId: string;
  restrictions?: string[];
}

export interface OdTariff {
  entryRampId: string;
  exitRampId: string;
  billingDistanceMeters: number;
  amountYen?: number | null;
  effectiveFrom?: string | null;
  effectiveTo?: string | null;
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
  billingDistanceMeters?: number | null;
  tollSource?: string | null;
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
  /**
   * Nearest Entry access point to the coordinate origin (`origin` input).
   * Reported even when no candidate is produced, including the cap-exceeded
   * `NO_CONNECTION` early return. `null` for `originNodeId` input and when the
   * graph has no Entry access points at all.
   */
  nearestAccess: SnappedOrigin | null;
  /**
   * Shortest `planSeconds` (`baseSeconds + bufferSeconds`) among legal loops
   * whose loop time is within the 240-minute product cap. Loops the requested
   * time window rejects are included, so this is the numeric basis for a
   * `TIME_WINDOW` rejection: `minPlanSeconds > 240 * 60` means no window up to
   * the 240-minute product limit can ever succeed.
   *
   * That reading is conditional. The enumeration is bounded by the product cap
   * (loop seconds only) and by resource limits (`SearchLimits.beamWidth`,
   * `maxExpandedStates`, `maxPairs`); the value is the smallest *enumerated*
   * loop and may overstate the true global minimum. When a resource limit cut
   * the enumeration short, the minimum is not provable and this is `null`, so
   * callers must not claim "the shortest loop takes N minutes" or "even four
   * hours cannot work" without a non-null value.
   *
   * `null` when no legal loop exists (e.g. `NO_CONNECTION`, `NO_LOOP`, or a
   * search that never reached the loop-enumeration stage) and when the
   * enumeration was truncated.
   */
  minPlanSeconds: number | null;
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
