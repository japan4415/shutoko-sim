// UI ↔ Web Worker のメッセージ契約（docs/interfaces.md「ブラウザの探索境界」）。
import type { LatLng, SearchResult, SearchRequest } from "../../../crates/routing-wasm/types/index.d";

export type { LatLng, SearchResult, SearchRequest };

/** UI → Worker の検索依頼エンベロープ。`type` は SearchRequest JSON には含めない。 */
export interface UiSearchMessage {
  type: "search";
  requestId: string;
  releaseId: string;
  pricingAt: string;
  origin?: LatLng;
  originNodeId?: string;
  minMinutes: number;
  maxMinutes: number;
  vehicleProfile: string;
}

/** Worker → UI の応答。 */
export type WorkerResponse =
  | { type: "ready"; releaseId: string }
  | { type: "result"; requestId: string; result: SearchResult }
  | { type: "error"; requestId: string; code: string; message?: string };
