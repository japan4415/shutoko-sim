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

export interface DeviceVerificationReleaseConfig {
  manifestJson: string;
  evaluatedAt: string;
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
  deviceVerification?: DeviceVerificationReleaseConfig;
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

// --- 料金 v3（tariff model v1）の製品スコープ ---
// 値は crates/routing-core/src/tariff.rs の PRODUCT_* 定数と同値であり、この範囲を
// 別の条件へ変えてはならない。Web reader は候補の toll がこのスコープと一致することを
// 実行時検証し、ずれた候補は RESULT_CONTRACT_MISMATCH として捨てる。

/** 製品の車種。 */
export type ProductVehicleClass = "ordinary";
/** 製品の支払方法。 */
export type ProductPaymentMethod = "etc";
/** 製品の料金種別。割引適用前の基本料金のみを扱う。 */
export type ProductFareBasis = "base_toll_excluding_discounts";

/** 割引として除外する識別子（すべてが tariff rule の discountsExcluded に入る）。 */
export type ExcludedTariffDiscount =
  | "midnight_discount"
  | "central_tokyo_inflow_discount"
  | "environmental_road_pricing_discount"
  | "etc2_discount"
  | "frequent_user_discount";

/** 製品の profile。車種・支払方法・料金種別と対になる。 */
export type ProductVehicleProfile = "passenger-car-etc";

/** 料金 v3 の出所。`official_distance_rule` は距離基準の公定料金のみ。 */
export type TariffTollSource = "official_distance_rule" | "table";

/** 候補の toll に付く料金 v3 の証拠（tariff v3 を持つ release でのみ現れる）。 */
export interface TariffProvenance {
  /** 適用された期間の出所。 */
  tollSource?: string | null;
  /** 適用された TariffAssignment の ID。 */
  assignmentId?: string | null;
  /** 適用された TariffRule の ID。 */
  ruleId?: string | null;
  /** 適用された期間そのものの BillingDistanceEvidence の ID。 */
  evidenceId?: string | null;
  /** OD 全体の BillingDistanceEvidence の ID。 */
  distanceEvidenceId?: string | null;
  /** 画面表示に使う料金ラベル。 */
  fareLabel?: string | null;
  vehicleClass?: string | null;
  paymentMethod?: string | null;
  fareBasis?: string | null;
  /** 割引を一切適用していないことを示すフラグ。 */
  discountsExcluded?: boolean;
}

/** 料金 v3 の丸め規則。 */
export interface TariffRoundingV3 {
  mode: string;
  multipleYen: number;
}

/** 料金規則が依拠する出典。documentId を持つ場合は版 PDF の特定ページを指す。 */
export interface TariffSourceRefV3 {
  documentId?: string | null;
  source?: string | null;
  page?: number | null;
  status?: string | null;
  location: string;
}

/** 依拠する公定料金表の版。 */
export interface TariffDocumentV3 {
  documentId: string;
  edition: string;
  url: string;
  cachePath: string;
  documentSha256?: string | null;
  status: string;
  reviewedAt?: string | null;
  reviewMethod?: string[];
  manualAcquisitionRequired?: boolean;
}

/** 半開区間 [effectiveFrom, effectiveTo) の距離基準料金規則。 */
export interface TariffRuleV3 {
  ruleId: string;
  vehicleClass: string;
  paymentMethod: string;
  fareBasis: string;
  discountsExcluded?: string[];
  distanceUnitMeters: number;
  effectiveFrom: string;
  effectiveTo: string | null;
  /** 100m あたりのマイクロ円単価。 */
  rateMicrosYenPerUnit: number;
  terminalChargeYen: number;
  taxBasisPoints: number;
  minimumYen: number;
  maximumYen: number;
  minimumDistanceMeters?: number | null;
  rounding: TariffRoundingV3;
  sourceRefs: TariffSourceRefV3[];
}

/** OD 割当の、1 期間ぶんの料金。 */
export interface TariffPriceV3 {
  status: string;
  tariffStatus: string;
  amountYen?: number | null;
  /** 当該版の PDF セルに記載された観測基本料金額。 */
  observedBaseFareYen?: number | null;
  observedDistanceMeters?: number | null;
  effectiveFrom: string;
  effectiveTo: string | null;
  ruleId: string;
  /** 当該期間の証拠 ID。 */
  evidenceId: string;
  distanceEvidenceId: string;
}

/** 人手レビュー済みの料金距離の証拠（PDF セル 1 セル = 1 レコード）。 */
export interface DistanceEvidenceV3 {
  evidenceId: string;
  documentId: string;
  edition: string;
  documentSha256: string;
  page: number;
  routeLabel: string;
  rowLabel: string;
  columnLabel: string;
  cell: string;
  fareVariant: string;
  ink: string;
  distanceMeters: number;
  distanceLabel: string;
  observedBaseFareYen: number;
  calculatedBaseFareYen: number;
  entryRampId: string;
  exitRampId: string;
  url: string;
  reviewedAt: string;
  reviewMethod?: string[];
}

/** まだ PDF レビューが済む前の証拠。金額は持たない（unpriced のまま扱う）。 */
export interface PendingEvidenceV3 {
  evidenceId: string;
  documentId: string;
  edition: string;
  page?: number | null;
  rowLabel: string;
  columnLabel: string;
  cell?: string | null;
  fareVariant: string;
  status: string;
  observedBaseFareYen?: number | null;
  observedDistanceMeters?: number | null;
  reviewedAt?: string | null;
}

/** OD 1 件に対する料金割当。 */
export interface TariffAssignmentV3 {
  assignmentId: string;
  odKey: string;
  /** 同じ OD を共有する課金ペアの ID。 */
  pairIds: string[];
  entryName: string;
  exitName: string;
  entryRampId: string;
  exitRampId: string;
  vehicleProfile: string;
  vehicleClass: string;
  paymentMethod: string;
  fareBasis: string;
  billingDistanceMeters: number;
  distanceEvidenceId: string;
  verificationStatus: string;
  endpointBindingStatus?: string | null;
  endpointBindingReason?: string | null;
  prices: TariffPriceV3[];
}

/** od-tariffs.json（version 3）の全体形。 */
export interface OdTariffsFileV3 {
  version: 3;
  source: string;
  sourceDate: string;
  vehicleProfile: string;
  vehicleClass: string;
  paymentMethod: string;
  fareBasis: string;
  fareLabel: string;
  discountsExcluded: string[];
  documents: TariffDocumentV3[];
  tariffRules: TariffRuleV3[];
  distanceEvidence: DistanceEvidenceV3[];
  pendingEvidence: PendingEvidenceV3[];
  assignments: TariffAssignmentV3[];
  /** version 2 時代の互換フィールド。 */
  rules?: unknown;
  verifiedOdPairs?: OdTariff[];
  deprecatedAssignments?: unknown[];
  migration?: unknown;
  pendingResolution?: unknown;
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

export type PairKind = "legacyRing" | "radialReturn";
export type AnchorKind = "sameNode" | "directedJunction";
export type RoutePlanSegmentRole =
  | "entry_approach"
  | "mandatory_lap"
  | "return_corridor"
  | "exit_approach";
export type PairEligibilityStatus =
  | "verified_one_section_ahead"
  | "unverified"
  | "topology_only";
export type LoopValidationStatus =
  | "declared_route_validated"
  | "unresolved"
  | "topology_only";
export type TariffStatus = "priced" | "unpriced" | "expired" | "not_applicable";
export type EndpointSupportState = "verified_bound" | "unsupported" | "unresolved";
export type RoutingCapability = "routable" | "structural_no_loop" | "unsupported";

export interface Price {
  amountYen: number;
  effectiveFrom: string;
  effectiveTo: string | null;
}

export interface PairEligibility {
  status: PairEligibilityStatus;
  oneSectionAheadVerified: boolean;
}

export interface LoopValidation {
  status: LoopValidationStatus;
}

/**
 * graph.json の billingPairs[].tariff（料金 v3）。
 *
 * build 時点の値をそのまま持つ。`status: "priced"` のときは `amountYen` と
 * `observedBaseFareYen` が一致し、`assignmentId` / `ruleId` / `evidenceId` /
 * `distanceEvidenceId` / `tollSource` と製品スコープが必ず揃う。
 * `observedBaseFareYen` は当該版 PDF セルに記載された観測額、`observedDistanceMeters`
 * は同じセルが表す料金距離で、`amountYen` の算出根拠そのもの。
 */
export interface Tariff extends TariffProvenance {
  status: TariffStatus;
  amountYen: number | null;
  /** 当該版 PDF セルに記載された観測基本料金額。 */
  observedBaseFareYen?: number | null;
  /** 当該版 PDF セルが表す料金距離。 */
  observedDistanceMeters?: number | null;
  billingDistanceMeters: number | null;
  effectiveFrom?: string | null;
  effectiveTo?: string | null;
  /** 全期間の価格。2026-10 改定後は 2 期間になる。 */
  prices: Price[];
}

export interface SameNodeAnchor {
  anchorKind: "sameNode";
  nodeId: string;
  routeId: string;
  direction: string;
  arcPolicy: "sameNodeLoop";
}

export interface ExcludedShortConnector {
  fromNodeId: string;
  toNodeId: string;
  osmWayId: number;
  edgeCount: number;
  distanceMeters: number;
}

export interface DirectedJunctionAnchor {
  anchorKind: "directedJunction";
  mergeNodeId: string;
  branchNodeId: string;
  mergeTerminalEdgeId: string;
  branchInitialEdgeId: string;
  routeId: string;
  direction: string;
  arcPolicy: "ordinaryLongArc";
  excludedShortConnector: ExcludedShortConnector;
}

export type RouteAnchor = SameNodeAnchor | DirectedJunctionAnchor;

export interface DirectedEndpointSegment {
  segmentId: string;
  osmWayIds: number[];
  osmNodeIds: number[];
  edgeIds: string[];
  fromNodeId: string;
  toNodeId: string;
  edgeIdsSha256: string;
}

export interface BillingEndpoint {
  rampId: string;
  name: string;
  supportState: EndpointSupportState;
  directedSegments: DirectedEndpointSegment[];
  bindingCandidates?: unknown[];
}

export interface EntryCorridor {
  membershipId: string;
  terminalEdgeId: string;
  mergeNodeId: string;
}

export interface MandatoryLap {
  membershipId: string;
  firstEdgeId: string;
  lastEdgeId: string;
  lapCount: 1;
}

export interface ReturnCorridor {
  membershipId: string;
  startNodeId: string;
  initialEdgeId: string;
  firstGeneralExit: {
    rule: "firstGeneralExit";
    expectedRampId: string;
    exactDirectedBinding: EndpointSupportState;
  };
}

export interface GraphRoutePlanV1 {
  entryCorridor: EntryCorridor;
  anchor: DirectedJunctionAnchor;
  mandatoryLap: MandatoryLap;
  returnCorridor: ReturnCorridor;
}

export interface ResolvedRouteSegment {
  resolvedSegmentId: string;
  role: RoutePlanSegmentRole;
  membershipId: string;
  sourceSegmentIds: string[];
  edgeIds: string[];
  edgeIdsSha256: string;
}

export interface LegacyRingBillingPairV2 {
  id: string;
  pairKind: "legacyRing";
  vehicleProfile: string;
  entryId: string;
  exitId: string;
  entryRampId?: string | null;
  exitRampId?: string | null;
  /** 正規化された入口・出口の名称（画面表示と OD 突合に使う）。 */
  entryName?: string | null;
  exitName?: string | null;
  /** 適用された料金割当と各期間の価格（tariff v3 以降）。 */
  assignmentId?: string | null;
  prices?: Price[];
  billingDistanceMeters?: number | null;
  anchor: SameNodeAnchor;
  entryToAnchorEdgeIds: string[];
  anchorToExitEdgeIds: string[];
  pairEligibility: PairEligibility;
  loopValidation: LoopValidation;
  tariff: Tariff;
}

export interface RadialReturnBillingPair {
  id: string;
  pairKind: "radialReturn";
  routePlanVersion: 1;
  vehicleProfile: string;
  entryId: string;
  exitId: string;
  entryEndpoint: BillingEndpoint;
  exitEndpoint: BillingEndpoint;
  routePlan: GraphRoutePlanV1;
  resolvedRouteSegments: ResolvedRouteSegment[];
  routingCapability: RoutingCapability;
  pairEligibility: PairEligibility;
  loopValidation: LoopValidation;
  tariff: Tariff;
}

export type GraphBillingPairV2 = LegacyRingBillingPairV2 | RadialReturnBillingPair;

export interface RouteMembershipSegment {
  segmentId: string;
  sourceKind: "relationMainline" | "boundRamp";
  sourceRelationId: string | null;
  sourceSnapshotSha256: string;
  bindingEvidenceId: string | null;
  orderedEdgeIds: string[];
  orderedEdgeIdsSha256: string;
}

export interface RouteMembershipIndex {
  membershipId: string;
  routeId: string;
  direction: string;
  directionMappingVersion: "osm-relation-role/v1";
  segments: RouteMembershipSegment[];
}

export interface GraphDocument {
  schemaVersion: 2 | 3 | 4;
  releaseId: string;
  vehicleProfile: string;
  nodes: unknown[];
  edges: unknown[];
  billingPairs: unknown[];
  forbiddenTransitions?: string[][];
  ramps?: Ramp[];
  odTariffs?: OdTariff[];
  routeMemberships?: RouteMembershipIndex[];
}

/**
 * C1 legacyRing 候補の toll。
 *
 * 料金 v3 を持つ release では `TariffProvenance` のフィールドがすべて揃い、
 * `amountYen` は「普通車ETC基本料金（割引適用前）」の額になる。`tariffStatus` が
 * `priced` でないときは、金額・料金距離・期間・各 ID がすべて null / 不在のまま
 * 残る（engine は未確定を金額で埋めない）。
 */
export interface LegacyToll extends TariffProvenance {
  billingPairId: string;
  chargedSectionCount: number;
  amountYen: number | null;
  pricingAt: string;
  effectiveFrom: string | null;
  effectiveTo: string | null;
  billingDistanceMeters?: number | null;
}

/**
 * radialReturn / topologyOnly 候補の toll。`LegacyToll` から
 * `chargedSectionCount` だけが無い形で、料金 v3 の証拠は同じ規則で付く。
 */
export interface RadialToll extends TariffProvenance {
  billingPairId: string;
  amountYen: number | null;
  pricingAt: string;
  effectiveFrom: string | null;
  effectiveTo: string | null;
  billingDistanceMeters?: number | null;
}

export type TopologyOnlyToll = RadialToll;

export type Toll = LegacyToll | RadialToll | TopologyOnlyToll;

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

export type MapsHandoffLegRole = "surface_access" | "loop_transfer" | "surface_return";

export interface MapsHandoffLegWire {
  role: MapsHandoffLegRole;
  mapsUrl: string;
  urlSha256: string;
}

export type RadialHandoff =
  | {
      enabled: true;
      legUrls: [MapsHandoffLegWire, MapsHandoffLegWire, MapsHandoffLegWire];
      disabledReason: null;
    }
  | {
      enabled: false;
      legUrls: [];
      disabledReason: "device_verification_pending";
    };

export type DeviceVerificationOs = "android" | "ios";
export type DeviceVerificationClient = "web" | "app";
export type DeviceVerificationResult = "passed" | "failed" | "missing" | "expired";

export interface DeviceVerificationLeg {
  role: MapsHandoffLegRole;
  urlSha256: string;
  expectedRoad: string;
  expectedDirection: string;
}

interface DeviceVerificationRecordBase {
  os: DeviceVerificationOs;
  osVersion: string;
  client: DeviceVerificationClient;
  clientName: string;
  clientVersion: string;
}

export type DeviceVerificationRecord =
  | (DeviceVerificationRecordBase & {
      verifiedAt: string;
      result: Exclude<DeviceVerificationResult, "missing">;
      expiresAt: string;
    })
  | (DeviceVerificationRecordBase & {
      verifiedAt: null;
      result: Extract<DeviceVerificationResult, "missing">;
      expiresAt: null;
    });

export interface DeviceVerificationManifest {
  schemaVersion: 1;
  routePlanId: string;
  releaseId: string;
  urlBuilderVersion: "google-maps-split/v1";
  legs: DeviceVerificationLeg[];
  verifications: DeviceVerificationRecord[];
}

interface CandidateBase {
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
  reasons: string[];
  /** Warning codes: "HANDOFF_WAYPOINTS_UNVERIFIED", "STATIC_TRAVEL_TIME" */
  warnings: string[];
}

export interface LegacyCandidate extends CandidateBase {
  pairKind?: "legacyRing";
  /** 料金の確定状況。旧 engine の LegacyCandidate は付けないため optional とする。 */
  tariffStatus?: TariffStatus;
  toll: LegacyToll;
  loop: Loop;
  handoff: Handoff;
}

export interface CandidateResolvedRouteSegment {
  resolvedSegmentId: string;
  role: RoutePlanSegmentRole;
  membershipId: string;
  sourceSegmentIds: string[];
  edgeIdsSha256: string;
}

export interface CandidateRoutePlan {
  membershipIds: string[];
  resolvedRouteSegments: CandidateResolvedRouteSegment[];
}

export interface EdgeRouteLeg {
  role: RoutePlanSegmentRole;
  resolvedSegmentId: string;
  startEdgeIndex: number;
  endEdgeIndexExclusive: number;
}

export interface EstimatedLeg {
  role: "surface_access" | "surface_return";
  estimated: true;
  distanceMeters: number;
  durationSeconds: number;
}

export interface TopologyOnlyCandidate extends CandidateBase {
  pairKind: "topologyOnly";
  estimatedLegs: EstimatedLeg[];
  eligibilityStatus: "topology_only";
  loopValidationStatus: "topology_only";
  tariffStatus: TariffStatus;
  toll: TopologyOnlyToll;
  loop: Loop;
  handoff: Handoff;
}

export interface RadialCandidate extends CandidateBase {
  pairKind: "radialReturn";
  routePlanVersion: 1;
  anchor: DirectedJunctionAnchor;
  routePlan: CandidateRoutePlan;
  edgeRouteLegs: EdgeRouteLeg[];
  estimatedLegs: EstimatedLeg[];
  eligibilityStatus: PairEligibilityStatus;
  loopValidationStatus: LoopValidationStatus;
  tariffStatus: TariffStatus;
  toll: RadialToll;
  handoff: RadialHandoff;
}

export type Candidate = LegacyCandidate | TopologyOnlyCandidate | RadialCandidate;

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
