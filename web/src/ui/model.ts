// UI 用の純粋関数群。DOM を触らず、Vitest から決定論的に検証できる。
// 文言対応表は scout-002 F5（docs/interfaces.md:96-104 の status/reason コード）に従う。
import type { Candidate, GeoJsonLineString, SearchResult, SnappedOrigin, Toll } from "../worker/types";

export const RELEASE_ID = "c1-real-v1";
export const VEHICLE_PROFILE = "passenger-car-etc";

/** 神田橋プリセット（crates/routing-core/tests/real_graph_contract.rs:520-528）。 */
export const PRESET_KANDABASHI = { lat: 35.6896727, lon: 139.7644248 } as const;

/** 探索タイムアウト（毫秒）。超過で UI 側が terminate する。 */
export const SEARCH_TIMEOUT_MS = 10_000;

export const TIMEOUT_TEXT = "探索が 10 秒を超えました。再度検索してください。";

/** UI が受理する計画時間の上限（分）。index.html の max 属性（240）と一致させる。 */
export const MAX_PRODUCT_MINUTES = 240;
export const MAX_PRODUCT_SECONDS = MAX_PRODUCT_MINUTES * 60;

/** 一般道アクセスの想定速度（m/s）。エンジンと同じ 30 km/h。 */
const ACCESS_SPEED_MPS = 30 / 3.6;
/** 直線距離に対する迂回係数。エンジンと同じ 1.3。 */
const ACCESS_DETOUR_FACTOR = 1.3;

/**
 * 直線距離（m）から片道アクセス秒を概算する。
 * エンジン（routing-core の `estimated_access_seconds`）と同じ式
 * （直線距離 × 1.3 ÷ 30 km/h、切り上げ）。一般道は探索せず直線距離の概算である。
 */
export function accessSecondsFromMeters(meters: number): number {
  return Math.ceil((meters * ACCESS_DETOUR_FACTOR) / ACCESS_SPEED_MPS);
}

/** 片道アクセス時間（分、四捨五入）。 */
export function accessMinutesFromMeters(meters: number): number {
  return minutesFromSeconds(accessSecondsFromMeters(meters));
}

/** 入力欄ごとのエラー文言。null は正常。range は最小・最大の両方に係る条件エラー。 */
export interface InputFieldErrors {
  lat: string | null;
  lon: string | null;
  minMinutes: string | null;
  maxMinutes: string | null;
  /** 複数欄にまたがる条件エラー（1 ≤ 最小 ≤ 最大 ≤ 240）。 */
  range: string | null;
}

/**
 * 座標・時間入力の検査（欄単位）。aria-invalid / aria-describedby の付与先を
 * 特定するため、どの欄の誤りかを失わずに返す。
 */
export function validateInputFields(
  latText: string,
  lonText: string,
  minText: string,
  maxText: string,
): InputFieldErrors {
  const lat = Number(latText);
  const lon = Number(lonText);
  const min = Number(minText);
  const max = Number(maxText);
  const latError =
    latText.trim() === "" || !Number.isFinite(lat) || lat < -90 || lat > 90
      ? "緯度は -90〜90 の有限の数値で入力してください。"
      : null;
  const lonError =
    lonText.trim() === "" || !Number.isFinite(lon) || lon < -180 || lon > 180
      ? "経度は -180〜180 の有限の数値で入力してください。"
      : null;
  const minError =
    minText.trim() === "" || !Number.isInteger(min)
      ? "最小時間は整数（分）で入力してください。"
      : null;
  const maxError =
    maxText.trim() === "" || !Number.isInteger(max)
      ? "最大時間は整数（分）で入力してください。"
      : null;
  const rangeError =
    latError === null &&
    lonError === null &&
    minError === null &&
    maxError === null &&
    !(1 <= min && min <= max && max <= 240)
      ? "時間の条件は 1 ≤ 最小 ≤ 最大 ≤ 240 の範囲で指定してください。"
      : null;
  return {
    lat: latError,
    lon: lonError,
    minMinutes: minError,
    maxMinutes: maxError,
    range: rangeError,
  };
}

/** 座標・時間入力の検査。理由付きで不正な項目を返す（空欄・非有限・範囲外）。 */
export function validateInputs(
  latText: string,
  lonText: string,
  minText: string,
  maxText: string,
): string[] {
  const fields = validateInputFields(latText, lonText, minText, maxText);
  return [fields.lat, fields.lon, fields.minMinutes, fields.maxMinutes, fields.range].filter(
    (message): message is string => message !== null,
  );
}

/** 秒を分の整数へ換算する（表示用、四捨五入）。 */
export function minutesFromSeconds(seconds: number): number {
  return Math.round(seconds / 60);
}

/**
 * 探索結果の status / reason から UI 文言を作る（scout-002 F5 の対応表）。
 * 候補付き result は候補カード描画が担い、この関数は no_candidates / truncated の
 * ステータス文言と、reason 無し（null）の補助文言を返す。
 *
 * reason ごとの真実の根拠（この版の契約）:
 * - NO_CONNECTION: 検証済み入口が 1 件も無い、または最寄り入口が cap（46 km）超。
 *   nearestAccess があれば距離とアクセス往復時間を示し、直線接続を実経路として出さない。
 * - TIME_WINDOW: 指定枠に収まらない。minPlanSeconds が最大 4 時間を超えるなら
 *   時間枠を広げても届かないため「最大 4 時間では周回できない」と数値で示す。
 *   超えないなら指定枠が原因なので従来どおり時間枠を広げる導線につなぐ。
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
    case "NO_CONNECTION": {
      if (result.nearestAccess !== null) {
        return `出発地点はこの版の対応範囲外です。${unreachableText(result.nearestAccess, result.minPlanSeconds)}`;
      }
      return `出発地点がこの版の対応範囲外です（検証済みの入口が見つかりません）。${SUPPORTED_AREA_TEXT}`;
    }
    case "NO_BILLING_PAIR":
      return "この版には検証済み課金ペアがありません。";
    case "NO_LOOP": {
      // 合法な周回が無い場合は「最短でも N 分」を主張しない（数値の根拠が無い）。
      if (result.nearestAccess !== null) {
        return `周回ルートが見つかりませんでした。${nearestAccessText(result.nearestAccess)}${SUPPORTED_AREA_TEXT}`;
      }
      return `周回ルートが見つかりませんでした。${SUPPORTED_AREA_TEXT}`;
    }
    case "TIME_WINDOW": {
      if (result.minPlanSeconds !== null && result.minPlanSeconds > MAX_PRODUCT_SECONDS) {
        return unreachableText(result.nearestAccess, result.minPlanSeconds);
      }
      return `指定時間枠（${String(minMinutes)}〜${String(maxMinutes)} 分）に収まる候補がありません。時間枠を広げると見つかる可能性があります。`;
    }
    case "NO_HANDOFF":
      return "地図引き継ぎ URL が上限超過のため除外されました。";
    case "SEARCH_LIMIT":
      return "探索が上限に達し、一部の候補だけを表示しています。";
    default:
      return "候補が見つかりませんでした。";
  }
}

/**
 * 対応範囲の説明。候補にできるのは検証済み C1 入出口の組み合わせだけで、
 * アクセス・帰着は一般道探索ではなく直線距離の概算であることを明示する。
 */
export const SUPPORTED_AREA_TEXT =
  "対応範囲は首都高速 都心環状線（C1）とその接続ランプの検証済み入出口です。アクセス・帰着は一般道探索ではなく直線距離の概算で、入口までの往復を含む計画時間が指定の範囲に収まる地点だけを候補にします。";

/**
 * 最寄り入口へのアクセス情報。直線距離と片道の概算時間を示す。
 * 実際の一般道経路ではないため「概算」を必ず添える。
 */
export function nearestAccessText(nearestAccess: SnappedOrigin): string {
  return `最寄り入口まで直線 約 ${distanceText(nearestAccess.distanceMeters)}・片道 約 ${String(
    accessMinutesFromMeters(nearestAccess.distanceMeters),
  )} 分（概算）。`;
}

/**
 * 最大 4 時間では周回できないことの数値根拠を示す文言。
 * - minPlanSeconds があれば「最短でも約 N 分」
 * - nearestAccess があれば「アクセスの往復だけで約 N 分」「最寄り入口まで直線 約 N km」
 * 数値の根拠が 1 つも無ければ距離・時間を捏造しない。
 */
export function unreachableText(
  nearestAccess: SnappedOrigin | null,
  minPlanSeconds: number | null,
): string {
  const reasons: string[] = [];
  if (minPlanSeconds !== null) {
    reasons.push(`周回できる最短の計画時間でも 約 ${String(minutesFromSeconds(minPlanSeconds))} 分`);
  }
  if (nearestAccess !== null) {
    reasons.push(
      `最寄り入口までのアクセス往復だけで 約 ${String(
        accessMinutesFromMeters(nearestAccess.distanceMeters) * 2,
      )} 分`,
    );
  }
  const head = reasons.length > 0 ? `${reasons.join("、")}かかるため、` : "";
  const access = nearestAccess !== null ? nearestAccessText(nearestAccess) : "";
  return `${head}最大 4 時間では周回できません。${access}${SUPPORTED_AREA_TEXT}`;
}

/** 候補ゼロの原因分類。復帰導線（時間を広げる / 出発地点を変える）を分けるために使う。 */
export type NoCandidateCase = "unreachable" | "time_window" | "unsupported_area" | "other";

/**
 * 候補ゼロ時の原因を分類する。
 * - unreachable: 指定枠が最大 4 時間でも届かない（数値根拠あり）。時間を広げる案内は誤り。
 * - time_window: 指定枠が狭いだけで、広げれば見つかる可能性がある。
 * - unsupported_area: 対応範囲・検証済みペア・周回の制約でこの版では作れない。
 */
export function classifyNoCandidates(result: SearchResult): NoCandidateCase {
  if (result.reason === "NO_CONNECTION") {
    return "unsupported_area";
  }
  if (result.reason === "TIME_WINDOW") {
    return result.minPlanSeconds !== null && result.minPlanSeconds > MAX_PRODUCT_SECONDS
      ? "unreachable"
      : "time_window";
  }
  if (result.reason === "NO_LOOP" || result.reason === "NO_BILLING_PAIR") {
    return "unsupported_area";
  }
  return "other";
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

/**
 * 料金表示。amountYen が null なら「料金額: 未算出」（docs/requirements.md:28）。
 * 桁数の異なる金額を比較しやすいよう 3 桁区切りにする（design-review-002 C3）。
 */
export function tollText(toll: Toll): string {
  return toll.amountYen === null
    ? "料金額: 未算出"
    : `料金額: ${toll.amountYen.toLocaleString("ja-JP")} 円`;
}

/** 推薦理由コードの日本語文言（docs/interfaces.md:148-151）。未知コードはそのまま返す。 */
export function reasonText(code: string): string {
  switch (code) {
    case "BEST_TIME_PER_YEN":
      return "時間あたりの料金効率が最良";
    case "BEST_SHUTOKO_TIME":
      return "首都高滞在時間が最長";
    case "ONE_SECTION_TOLL":
      return "1区間料金（最低料金）";
    default:
      return code;
  }
}

/** 時間内訳の文言。入り・首都高・帰り・余裕の内訳を 1 行で示す。
 * 入り・帰りは直線距離ベースの概算値（直線距離 × 1.3 ÷ 30km/h）。 */
export function timeBreakdownText(model: {
  accessMinutes: number;
  shutokoMinutes: number;
  returnMinutes: number;
  bufferMinutes: number;
}): string {
  return `内訳: 入り ${String(model.accessMinutes)}分（概算） / 首都高 ${String(model.shutokoMinutes)}分 / 帰り ${String(model.returnMinutes)}分（概算） / 余裕 ${String(model.bufferMinutes)}分`;
}

/** 距離表示（m → km、小数 1 桁）。 */
export function distanceText(meters: number): string {
  return `${(meters / 1000).toFixed(1)} km`;
}

/**
 * 最安順位ラベル。料金未算出の候補が 1 件でもあると集合全体で金額比較が
 * できないため、allPriced が false なら null（順位を出さない）。docs/requirements.md:24。
 */
export function formatRank(allPriced: boolean, index: number): string | null {
  return allPriced ? `最安順位 ${String(index)} 位` : null;
}

/** 推薦ラベル。BEST_* 系の理由を持つ先頭候補にのみ付ける。 */
export function recommendedLabel(candidate: { reasons: string[] }): string | null {
  return candidate.reasons.some((code) => code === "BEST_TIME_PER_YEN" || code === "BEST_SHUTOKO_TIME")
    ? "推薦"
    : null;
}

/** 住所検索クエリの検査。空白除去後 1〜200 文字（docs/interfaces.md:53）。 */
export function validateAddressQuery(text: string): string | null {
  const trimmed = text.trim();
  if (trimmed === "") {
    return "住所または地名を入力してください。";
  }
  if (trimmed.length > 200) {
    return "住所検索は 200 文字以内で入力してください。";
  }
  return null;
}

/** ジオコーダー Worker のエラーコード → 日本語文言（docs/interfaces.md:63-70）。 */
export function geocodeErrorMessage(code: string): string {
  switch (code) {
    case "INVALID_QUERY":
      return "検索語を確認してください（1〜200 文字）。";
    case "PAYLOAD_TOO_LARGE":
      return "検索語が長すぎます。短くして再試行してください。";
    case "RATE_LIMITED":
      return "検索回数の上限に達しました。しばらく待って再試行してください。";
    case "GEOCODER_TIMEOUT":
      return "住所検索がタイムアウトしました。再試行してください。";
    case "GEOCODER_UNAVAILABLE":
      return "住所検索サービスが一時的に利用できません。再試行してください。";
    case "RATE_LIMITER_UNAVAILABLE":
      return "住所検索を現在受け付けられません。しばらく待って再試行してください。";
    case "FETCH_FAILED":
      return "住所検索の通信に失敗しました。再試行してください。";
    default:
      return "住所検索に失敗しました。再試行してください。";
  }
}

/** 座標ラベル（小数 5 桁、緯度経度の順）。ログには出さない。 */
export function coordinateLabel(lat: number, lon: number): string {
  return `${lat.toFixed(5)}, ${lon.toFixed(5)}`;
}

/** Maps URL 等に使う緯度経度の桁揃え表記（小数 5 桁）。 */
export function formatLatLng(lat: number, lon: number): string {
  return `${lat.toFixed(5)},${lon.toFixed(5)}`;
}

/** GeolocationPositionError.code → 日本語文言。再要求はしない。 */
export function geolocationErrorMessage(code: number): string {
  switch (code) {
    case 1:
      return "位置情報の利用が許可されていません。住所検索を利用してください。";
    case 2:
      return "現在地を取得できませんでした。電波状況を確認して再試行してください。";
    case 3:
      return "現在地の取得がタイムアウトしました。再試行してください。";
    default:
      return "現在地を取得できませんでした。住所検索を利用してください。";
  }
}

/** 候補カードの描画モデル。 */
export interface CardModel {
  id: string;
  /** 1 始まり。色に依存せず候補を識別する番号（design-review-002）。 */
  index: number;
  planMinutes: number;
  /** 余裕（buffer）を含まない総推定時間。 */
  baseMinutes: number;
  accessMinutes: number;
  shutokoMinutes: number;
  returnMinutes: number;
  bufferMinutes: number;
  distanceKm: number;
  toll: string;
  tollShort: string;
  timePerYen: string | null;
  rankLabel: string | null;
  reasons: string[];
  route: string;
  roadNames: string[];
  chargedSection: string;
  warnings: string[];
  mapsUrl: string;
  geometry: GeoJsonLineString;
  entryId: string;
  exitId: string;
  loopEdgeIds: string[];
  edgeIds: string[];
}

function rampName(candidate: Candidate, which: "entry" | "exit"): string {
  const name = which === "entry" ? candidate.entry.name : candidate.exit.name;
  return name ?? candidate[which === "entry" ? "entryId" : "exitId"];
}

/** 料金の短縮表記（カード上部のバッジ向け）。未算出は「未算出」。 */
function tollShortText(toll: Toll): string {
  return toll.amountYen === null
    ? "未算出"
    : `${toll.amountYen.toLocaleString("ja-JP")} 円`;
}

/**
 * 円あたり効率（首都高時間 / 料金）。amountYen が null のときは算出しない。
 * 「1区間の料金で首都高を約 s 分走る」の比較値をカードに添えるための文字列。
 */
function timePerYenText(candidate: Candidate): string | null {
  const amount = candidate.toll.amountYen;
  if (amount === null || amount <= 0) {
    return null;
  }
  const shutokoMinutes = minutesFromSeconds(candidate.duration.shutokoSeconds);
  return `1 円あたり 約 ${(shutokoMinutes / amount).toFixed(2)} 分`;
}

/**
 * Candidate → 表示モデル。警告はコード→日本語文言へ変換する。
 * index 省略時は 1 とし、単票プレビューでも描画できるようにする。
 * rankLabel は list 全体の料金確定状況が分かる呼び出し側で上書きするため null を既定にする。
 */
export function toCardModel(candidate: Candidate, index = 1): CardModel {
  const entry = rampName(candidate, "entry");
  const exit = rampName(candidate, "exit");
  return {
    id: candidate.id,
    index,
    planMinutes: minutesFromSeconds(candidate.duration.planSeconds),
    baseMinutes: minutesFromSeconds(candidate.duration.baseSeconds),
    accessMinutes: minutesFromSeconds(candidate.duration.accessSeconds),
    shutokoMinutes: minutesFromSeconds(candidate.duration.shutokoSeconds),
    returnMinutes: minutesFromSeconds(candidate.duration.returnSeconds),
    bufferMinutes: minutesFromSeconds(candidate.duration.bufferSeconds),
    distanceKm: Number((candidate.distanceMeters / 1000).toFixed(1)),
    toll: tollText(candidate.toll),
    tollShort: tollShortText(candidate.toll),
    timePerYen: timePerYenText(candidate),
    rankLabel: null,
    reasons: candidate.reasons.map(reasonText),
    route: `${entry} → ${exit}`,
    roadNames: candidate.roadNames,
    chargedSection: `課金対象: ${entry} → ${exit} の1区間`,
    warnings: candidate.warnings.map(warningText),
    mapsUrl: candidate.handoff.mapsUrl,
    geometry: candidate.geometry,
    entryId: candidate.entryId,
    exitId: candidate.exitId,
    loopEdgeIds: candidate.loop.edgeIds,
    edgeIds: candidate.edgeIds,
  };
}

/** 候補リストを ID で引き当てる。見つからなければ null。 */
export function selectCandidate<T extends { id: string }>(
  candidates: readonly T[],
  id: string,
): T | null {
  return candidates.find((candidate) => candidate.id === id) ?? null;
}
