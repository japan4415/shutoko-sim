// 計測ページ（bench.html）が回す代表パターンの定義。
//
// 出発地点は scout-003 F5 で実 WASM を実行して候補生成を確認済みの 10 件。
// 末尾 2 件（銀座入口・代官町入口）は 200m 以内に接続可能な一般道が無い
// `status: "no_candidates"` / `reason: "NO_CONNECTION"` の系統を被覆するための起点。
//
// 注意: 探索コストは min/max の時間条件に依存せず起点のみで決まる（scout-003 F4）。
// 時間条件 3 件は探索の仕事量を変えず、TIME_WINDOW による候補フィルタの分岐を
// 被覆するために残している。

/** 計測用の出発地点。 */
export interface BenchOrigin {
  id: string;
  label: string;
  lat: number;
  lon: number;
  /** 200m 以内に接続できる一般道が無く NO_CONNECTION になると想定される起点。 */
  noConnection: boolean;
}

/** 計測用の時間条件（分）。 */
export interface BenchTimeCondition {
  id: "short" | "mid" | "long";
  label: string;
  minMinutes: number;
  maxMinutes: number;
}

/** 出発地点 × 時間条件の 1 パターン。`index` が URL クエリ `?patterns=` の指定値。 */
export interface BenchPattern {
  index: number;
  id: string;
  label: string;
  origin: BenchOrigin;
  time: BenchTimeCondition;
}

/** 代表出発地点 10 件（scout-003 F5 の実測座標）。 */
export const BENCH_ORIGINS: readonly BenchOrigin[] = [
  { id: "kandabashi", label: "神田橋入口", lat: 35.689673, lon: 139.764425, noConnection: false },
  { id: "takaracho", label: "宝町入口", lat: 35.678188, lon: 139.774484, noConnection: false },
  { id: "kasumigaseki-in", label: "霞が関入口(内)", lat: 35.673967, lon: 139.747932, noConnection: false },
  { id: "shibakoen-in", label: "芝公園入口(内)", lat: 35.6535, lon: 139.749818, noConnection: false },
  { id: "shibakoen-out", label: "芝公園入口(外)", lat: 35.654923, lon: 139.744355, noConnection: false },
  { id: "kasumigaseki-out", label: "霞が関入口(外)", lat: 35.674176, lon: 139.747945, noConnection: false },
  { id: "nihonbashi", label: "日本橋", lat: 35.6838, lon: 139.7745, noConnection: false },
  { id: "yurakucho", label: "有楽町", lat: 35.6751, lon: 139.7638, noConnection: false },
  { id: "ginza", label: "銀座入口（NO_CONNECTION 想定）", lat: 35.66695, lon: 139.767957, noConnection: true },
  { id: "daikancho", label: "代官町入口（NO_CONNECTION 想定）", lat: 35.689332, lon: 139.75253, noConnection: true },
];

/** 時間条件 3 件（short / mid / long）。 */
export const BENCH_TIME_CONDITIONS: readonly BenchTimeCondition[] = [
  { id: "short", label: "15〜30 分", minMinutes: 15, maxMinutes: 30 },
  { id: "mid", label: "15〜60 分", minMinutes: 15, maxMinutes: 60 },
  { id: "long", label: "15〜120 分", minMinutes: 15, maxMinutes: 120 },
];

/** 全パターン（10 地点 × 3 条件 = 30 件）。index はこの配列の位置と一致する。 */
export const BENCH_PATTERNS: readonly BenchPattern[] = BENCH_ORIGINS.flatMap((origin, originIndex) =>
  BENCH_TIME_CONDITIONS.map((time, timeIndex) => ({
    index: originIndex * BENCH_TIME_CONDITIONS.length + timeIndex,
    id: `${origin.id}-${time.id}`,
    label: `${origin.label} / ${time.label}`,
    origin,
    time,
  })),
);

/**
 * `?patterns=0,5` の値から有効なパターンを解決する。
 *
 * - 未指定（`null`）または空文字は全 30 件（既定）。
 * - 範囲外（0 未満・30 以上）や整数として読めない値は**黙って全件へフォールバックせず無視**する。
 *   例: `"0,99"` → パターン 0 の 1 件、`"99"` / `"30"` / `"-1"` / `"1.5"` / `"abc"` → 0 件。
 * - 有効な index が 1 つも無い場合は空配列を返す。呼び出し側（bench ページ / ランナー）が
 *   「不正な指定」として扱い、全件を回してしまう事故を防ぐ。
 */
export function selectPatterns(spec: string | null): readonly BenchPattern[] {
  if (spec === null || spec.trim() === "") {
    return BENCH_PATTERNS;
  }
  const indexes = new Set<number>();
  for (const part of spec.split(",")) {
    const text = part.trim();
    // 10 進の整数リテラルだけを受け付ける（"1.5" / "1e1" / "0x10" / "+5" は無効）。
    if (!/^\d+$/.test(text)) {
      continue;
    }
    const value = Number(text);
    if (value < BENCH_PATTERNS.length) {
      indexes.add(value);
    }
  }
  return BENCH_PATTERNS.filter((pattern) => indexes.has(pattern.index));
}
