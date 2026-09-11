// UI 用の純粋関数群。DOM を触らず、Vitest から決定論的に検証できる。
// 文言対応表は scout-002 F5（docs/interfaces.md:96-104 の status/reason コード）に従う。
import type { Candidate, SearchResult, Toll } from "../worker/types";

export const RELEASE_ID = "c1-real-v1";
export const VEHICLE_PROFILE = "passenger-car-etc";

/** 神田橋プリセット（crates/routing-core/tests/real_graph_contract.rs:520-528）。 */
export const PRESET_KANDABASHI = { lat: 35.6896727, lon: 139.7644248 } as const;

/** 探索タイムアウト（毫秒）。超過で UI 側が terminate する。 */
export const SEARCH_TIMEOUT_MS = 10_000;

export const TIMEOUT_TEXT = "探索が 10 秒を超えました。再度検索してください。";

/** 座標・時間入力の検査。理由付きで不正な項目を返す（空欄・非有限・範囲外）。 */
export function validateInputs(
  latText: string,
  lonText: string,
  minText: string,
  maxText: string,
): string[] {
  const errors: string[] = [];
  const lat = Number(latText);
  const lon = Number(lonText);
  if (latText.trim() === "" || !Number.isFinite(lat) || lat < -90 || lat > 90) {
    errors.push("緯度は -90〜90 の有限の数値で入力してください。");
  }
  if (lonText.trim() === "" || !Number.isFinite(lon) || lon < -180 || lon > 180) {
    errors.push("経度は -180〜180 の有限の数値で入力してください。");
  }
  const min = Number(minText);
  const max = Number(maxText);
  if (minText.trim() === "" || !Number.isInteger(min)) {
    errors.push("最小時間は整数（分）で入力してください。");
  }
  if (maxText.trim() === "" || !Number.isInteger(max)) {
    errors.push("最大時間は整数（分）で入力してください。");
  }
  if (
    errors.length === 0 &&
    !(1 <= min && min <= max && max <= 240)
  ) {
    errors.push("時間の条件は 1 ≤ 最小 ≤ 最大 ≤ 240 の範囲で指定してください。");
  }
  return errors;
}

/** 秒を分の整数へ換算する（表示用、四捨五入）。 */
export function minutesFromSeconds(seconds: number): number {
  return Math.round(seconds / 60);
}

/**
 * 探索結果の status / reason から UI 文言を作る（scout-002 F5 の対応表）。
 * 候補付き result は候補カード描画が担い、この関数は no_candidates / truncated の
 * ステータス文言と、reason 無し（null）の補助文言を返す。
 */
export function statusMessage(
  result: SearchResult,
  minMinutes: number,
  maxMinutes: number,
): string {
  if (result.status === "ok") {
    return `候補が ${String(result.candidates.length)} 件見つかりました。`;
  }
  switch (result.reason) {
    case "NO_CONNECTION":
      return "200m 以内に接続できる一般道がありません。近くの座標を入力し直してください。";
    case "NO_BILLING_PAIR":
      return "この版には検証済み課金ペアがありません。";
    case "NO_LOOP":
      return "周回ルートが見つかりませんでした。";
    case "TIME_WINDOW":
      return `指定時間枠（${String(minMinutes)}〜${String(maxMinutes)} 分）に収まる候補がありません。時間枠を広げると見つかる可能性があります。`;
    case "NO_HANDOFF":
      return "地図引き継ぎ URL が上限超過のため除外されました。";
    case "SEARCH_LIMIT":
      return "探索が上限に達し、一部の候補だけを表示しています。";
    default:
      return "候補が見つかりませんでした。";
  }
}

/** Worker error / ステータス文言の一覧（docs/interfaces.md の error.code と対応）。 */
export function errorMessage(code: string): string {
  switch (code) {
    case "ARTIFACT_MISMATCH":
      return "成果物が期待値と一致しません（ARTIFACT_MISMATCH）。再読み込みしてください。部分データでは探索しません。";
    case "FETCH_FAILED":
      return "成果物の取得に失敗しました（FETCH_FAILED）。再読み込みしてください。";
    case "TIMEOUT":
      return TIMEOUT_TEXT;
    default:
      return `探索を完了できませんでした（${code}）。`;
  }
}

/** 警告コードの日本語文言（docs/interfaces.md:130-133）。 */
export function warningText(code: string): string {
  switch (code) {
    case "HANDOFF_WAYPOINTS_UNVERIFIED":
      return "経由地点の選定ルールは暫定（実機検証未了）";
    case "STATIC_TRAVEL_TIME":
      return "静的速度に基づく推定（渋滞・規制は未反映）";
    default:
      return code;
  }
}

/** 料金表示。amountYen が null なら「料金額: 未算出」（docs/requirements.md:28）。 */
export function tollText(toll: Toll): string {
  return toll.amountYen === null ? "料金額: 未算出" : `料金額: ${String(toll.amountYen)} 円`;
}

/** 候補カードの描画モデル。 */
export interface CardModel {
  id: string;
  planMinutes: number;
  toll: string;
  route: string;
  roadNames: string[];
  chargedSection: string;
  warnings: string[];
  mapsUrl: string;
}

function rampName(candidate: Candidate, which: "entry" | "exit"): string {
  const name = which === "entry" ? candidate.entry.name : candidate.exit.name;
  return name ?? candidate[which === "entry" ? "entryId" : "exitId"];
}

/** Candidate → 表示モデル。警告はコード→日本語文言へ変換する。 */
export function toCardModel(candidate: Candidate): CardModel {
  const entry = rampName(candidate, "entry");
  const exit = rampName(candidate, "exit");
  return {
    id: candidate.id,
    planMinutes: minutesFromSeconds(candidate.duration.planSeconds),
    toll: tollText(candidate.toll),
    route: `${entry} → ${exit}`,
    roadNames: candidate.roadNames,
    chargedSection: `課金対象: ${entry} → ${exit} の1区間`,
    warnings: candidate.warnings.map(warningText),
    mapsUrl: candidate.handoff.mapsUrl,
  };
}
