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

/**
 * 探索時に prepare へ渡す入口アクセス距離の上界（m）。
 * web/src/worker/pipeline.ts の MAX_ACCESS_DISTANCE_METERS と同じ値でなければならない。
 * 契約テスト（ui-model.test.ts）が両者の一致を検証する。
 */
export const MAX_ACCESS_DISTANCE_METERS = 46_000;

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
 * 秒を分へ切り上げる。上限超過の根拠値を四捨五入で上限以下に見せないための換算
 * （14401〜14429 秒は Math.round だと 240 分になり「4 時間に収まるのに到達不能」と
 * 矛盾して読める）。docs/interfaces.md の minPlanSeconds 契約に対応する。
 */
export function minutesCeilFromSeconds(seconds: number): number {
  return Math.ceil(seconds / 60);
}

/**
 * 計画時間（`baseSeconds + bufferSeconds`）から `baseSeconds` を復元する。
 * エンジンは `buffer = max(300, ceil(base/5))`、`plan = base + buffer` の単調増加で
 * 計算するため plan から base は一意に定まる。UI が「最小時間を何分まで下げれば
 * 既知の最短周回を下限に含められるか」を数値で示すために使う。
 */
export function baseSecondsFromPlanSeconds(planSeconds: number): number {
  let lo = 0;
  let hi = Math.max(0, planSeconds);
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2) + 1;
    if (mid + Math.max(300, Math.ceil(mid / 5)) <= planSeconds) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  return lo;
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
 *   超えないなら指定枠が原因なので timeWindowActions の判定に従う。既知の最短周回が
 *   最小時間で除外されている場合は最小時間を下げる案内を出し、上限（240 分）で
 *   最大時間を広げられないときは拡大を促さない。
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
      if (result.nearestAccess !== null && isAccessBeyondCap(result.nearestAccess)) {
        // cap 超過: 最寄り入口が遠く、時間枠でも到達できない。距離と時間を数値で示す。
        return `出発地点はこの版の対応範囲外です。${unreachableText(result.nearestAccess, result.minPlanSeconds)}`;
      }
      if (result.nearestAccess !== null) {
        // 最寄りは近いのに検証済み入口へ接続できない経路。時間の話とは切り離す。
        return `出発地点に接続できる検証済みの入口が見つかりませんでした。${nearestAccessText(result.nearestAccess)}${SUPPORTED_AREA_TEXT}`;
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
      const actions = timeWindowActions(result, minMinutes, maxMinutes);
      if (actions.lowerMinMinutes !== null) {
        // 既知の最短周回が下限（最小時間）で除外されている。最大側ではなく最小側を下げる。
        return `指定時間枠（${String(minMinutes)}〜${String(maxMinutes)} 分）に収まる候補がありません。最小時間を ${String(
          actions.lowerMinMinutes,
        )} 分に下げると見つかる可能性があります。`;
      }
      if (actions.widenMaxMinutes !== null) {
        return `指定時間枠（${String(minMinutes)}〜${String(maxMinutes)} 分）に収まる候補がありません。時間枠を広げると見つかる可能性があります。`;
      }
      // 上限（240 分）でこれ以上広げられず、値も証明できない（打切り等）。誤った拡大を促さない。
      return `指定時間枠（${String(minMinutes)}〜${String(maxMinutes)} 分）に収まる候補がありません。時間枠を広げられないため、出発地点や条件を見直してください。`;
    }
    case "NO_HANDOFF":
      return "地図引き継ぎ URL が上限超過のため除外されました。";
    case "SEARCH_LIMIT": {
      // 打切りは「可能性」ではなく確定事実（`status: "truncated"` のときだけ reason が立つ）。
      // 候補が残っている場合は「一部だけ」であることを落とさない（review R3-05）。
      const truncated = "探索が上限に達したため、結果は打ち切られています。";
      return result.candidates.length > 0
        ? `${truncated}表示しているのは検証済みの一部の候補だけです。`
        : truncated;
    }
    default:
      return "候補が見つかりませんでした。";
  }
}

/**
 * 座標入力の NO_CONNECTION が「最寄り入口が cap 超過」によるものかを判定する。
 *
 * エンジンの NO_CONNECTION には別経路がある: `max_access_entries` により
 * 検証済み課金ペアの入口がアクセス候補に含まれない場合で、このとき
 * `nearestAccess` は近距離でも返る。その場合に「アクセス往復だけで 4 時間」と
 * 断定すると嘘になるため、UI はこの判定でガードする（opus5 F2）。
 * エンジンは `distanceMeters > cap` のときだけ cap 超過 NO_CONNECTION を返し、
 * `nearestAccess` は cap 判定前に構築されるため、同じ比較で正確に区別できる。
 */
export function isAccessBeyondCap(nearestAccess: SnappedOrigin): boolean {
  return nearestAccess.distanceMeters > MAX_ACCESS_DISTANCE_METERS;
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
 * - minPlanSeconds があれば「確認できた範囲で最も短い計画時間は 約 N 分」
 * - nearestAccess があれば「アクセスの往復だけで約 N 分」「最寄り入口まで直線 約 N km」
 * 数値の根拠が 1 つも無ければ距離・時間を捏造しない。
 *
 * minPlanSeconds は「ループ部分が 240 分以内」の周回だけを列挙した範囲での最小値で、
 * 列挙外のより長いループがより小さい plan を持つ可能性を排除できない（docs/interfaces.md）。
 * したがって絶対的な「最短」とは断定せず、値の出所（確認できた範囲）を必ず添える。
 *
 * 表示値は 240 分との比較だけ切り上げる（14401〜14429 秒が丸めで 240 分になり
 * 「4 時間に収まるのに到達不能」と矛盾して読めるため）。分表示は「約」付きの概数であり、
 * 切り上げは上限超過という結論を安全側に見せるだけで数値の捏造ではない。
 *
 * 「最大 4 時間では周回できません」の根拠は経路で異なる:
 * - minPlanSeconds が non-null: 列挙が資源上限で打ち切られていないので、列挙された
 *   最小計画時間が上限を超えることから厳密に成立する。
 * - minPlanSeconds が null（cap 超過 NO_CONNECTION）: アクセス往復だけで 240 分以上あり、
 *   さらに周回と余裕時間が加わることから成立する。往復がちょうど 240 分でも単独では
 *   超えないため、周回・余裕の加算を文言に含める。
 */
export function unreachableText(
  nearestAccess: SnappedOrigin | null,
  minPlanSeconds: number | null,
): string {
  const access = nearestAccess !== null ? nearestAccessText(nearestAccess) : "";
  if (minPlanSeconds !== null) {
    // 計画時間は上限超過の直接の根拠。14401〜14429 秒は丸めると 240 分になり
    // 「4 時間に収まるのに到達不能」と矛盾して読めるため、切り上げて「4 時間超」を明示する。
    const evidence = [
      `確認できた範囲で最も短い計画時間は 約 ${String(minutesCeilFromSeconds(minPlanSeconds))} 分（4 時間超）`,
    ];
    if (nearestAccess !== null) {
      evidence.push(
        `最寄り入口までのアクセス往復だけで 約 ${String(
          accessMinutesFromMeters(nearestAccess.distanceMeters) * 2,
        )} 分`,
      );
    }
    return `${evidence.join("、")}かかるため、最大 4 時間では周回できません。${access}${SUPPORTED_AREA_TEXT}`;
  }
  if (nearestAccess !== null) {
    // minPlanSeconds が無い cap 超過 NO_CONNECTION はアクセス往復だけが数値根拠。
    // 往復が上限ちょうど（約 240 分）でも単独では超えないため、周回と余裕時間が
    // 加わって超えることを明示する（往復だけを単独根拠に見せない）。
    return `最寄り入口までのアクセス往復だけで 約 ${String(
      accessMinutesFromMeters(nearestAccess.distanceMeters) * 2,
    )} 分かかるうえ、周回と余裕時間も加わるため、最大 4 時間では周回できません。${access}${SUPPORTED_AREA_TEXT}`;
  }
  return `最大 4 時間では周回できません。${SUPPORTED_AREA_TEXT}`;
}

/**
 * TIME_WINDOW の候補ゼロ時に提示する復帰操作。実際に値が変わる操作だけを返す
 * （値が変わらない操作を成功として告げない）。
 * - `lowerMinMinutes`: 既知の最短周回が最小時間（下限）で除外されている場合の下限値。
 *   エンジンの下限判定は `base < minMinutes*60` なので、`baseSecondsFromPlanSeconds` で
 *   復元した base から「下限以下の最大の分」を求める。base が 60 秒未満の周回は製品下限
 *   1 分でも含められないため `null`（review R3-02）。
 * - `widenMaxMinutes`: 最大時間の拡大値。製品上限 240 分では広げられないため `null`。
 */
export interface TimeWindowActions {
  lowerMinMinutes: number | null;
  widenMaxMinutes: number | null;
}

export function timeWindowActions(
  result: SearchResult,
  minMinutes: number,
  maxMinutes: number,
): TimeWindowActions {
  const widenMaxMinutes =
    maxMinutes < MAX_PRODUCT_MINUTES
      ? Math.min(MAX_PRODUCT_MINUTES, Math.max(maxMinutes + 30, 30))
      : null;
  let lowerMinMinutes: number | null = null;
  const planSeconds = result.minPlanSeconds;
  if (planSeconds !== null && planSeconds <= MAX_PRODUCT_SECONDS) {
    const baseSeconds = baseSecondsFromPlanSeconds(planSeconds);
    // エンジンは `base < minMinutes*60` の周回を棄却する。製品下限は 1 分（60 秒）なので、
    // base が 60 秒未満の合法周回はどの最小時間でも含められない。1 分へ下げれば含められると
    // 誤案内しないため、その場合は操作を出さない（review R3-02）。
    if (baseSeconds >= 60) {
      const next = Math.floor(baseSeconds / 60);
      if (next < minMinutes) {
        lowerMinMinutes = next;
      }
    }
  }
  return { lowerMinMinutes, widenMaxMinutes };
}

/**
 * 「最小時間を N 分に下げる」ボタン押下時の結果。押下時点の現在値と比較して、実際に
 * 値を下げられる場合だけ適用する。復帰パネルは条件変更で失効させるが、手入力と競合しても
 * 同値・引上げを「下げました」と偽らないための実行時ガードを二重に持つ（review R3-01）。
 * 値が変わらない操作は成功として告げず、利用者に条件の見直しを促す。
 */
export interface LowerMinClick {
  /** 適用する新しい最小時間（分）。null なら値も文言も変えない。 */
  nextValue: number | null;
  /** #status へ出す文言。 */
  message: string;
}

export function lowerMinClickOutcome(
  currentMinutes: number,
  nextMinMinutes: number,
): LowerMinClick {
  if (!Number.isFinite(currentMinutes)) {
    return {
      nextValue: null,
      message: "最小時間の入力が有効な数値ではありません。値を確認してください。",
    };
  }
  if (nextMinMinutes >= currentMinutes) {
    return {
      nextValue: null,
      message: `最小時間は ${String(currentMinutes)} 分のままです。値を下げられないため、出発地点や条件を見直してください。`,
    };
  }
  return {
    nextValue: nextMinMinutes,
    message: `最小時間を ${String(nextMinMinutes)} 分に下げました。再検索してください。`,
  };
}

/** 候補ゼロの原因分類。復帰導線（時間を変える / 出発地点を変える / 再試行）を分けるために使う。 */
export type NoCandidateCase = "unreachable" | "time_window" | "unsupported_area" | "retry";

/**
 * 候補ゼロ時の原因を分類する。
 * - unreachable: 指定枠が最大 4 時間でも届かない（数値根拠あり）。時間を広げる案内は誤り。
 * - time_window: 指定枠が狭いだけで、時間枠を変えれば見つかる可能性がある。
 * - unsupported_area: 対応範囲・検証済みペア・周回の制約でこの版では作れない。
 * - retry: 探索打切り（SEARCH_LIMIT）・引き継ぎ除外（NO_HANDOFF）など、時間枠では説明できず
 *   条件変更や再試行が必要なもの。時間枠を広げれば解決すると偽らない。
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
  return "retry";
}

/** Worker error / ステータス文言の一覧（docs/interfaces.md の error.code と対応）。 */
export function errorMessage(code: string): string {
  switch (code) {
    case "ARTIFACT_MISMATCH":
      return "成果物が期待値と一致しません（ARTIFACT_MISMATCH）。再読み込みしてください。部分データでは探索しません。";
    case "FETCH_FAILED":
      return "成果物の取得に失敗しました（FETCH_FAILED）。再読み込みしてください。";
    case "RESULT_CONTRACT_MISMATCH":
      return "探索結果が実行中のエンジンと一致しません（RESULT_CONTRACT_MISMATCH）。再読み込みしてください。";
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
