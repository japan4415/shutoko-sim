// 出発地点確定時に地図をどこまで動かすかの判定（純粋関数）。
// Leaflet に依存させないことで、bounds 内外・reduced-motion の分岐をユニットで固定する。

/** 出発地点へ追従するときの最小ズーム。初期表示（12）より寄せて位置を確認しやすくする。 */
export const FOCUS_ORIGIN_MIN_ZOOM = 13;

/** 地図をどう動かすかの決定。null は「動かさない」。 */
export interface OriginFocusPlan {
  /** setView は pan + zoom、pan は平行移動のみ。 */
  action: "pan" | "setView";
  /** setView のときの目標ズーム。 */
  zoom: number;
  /** Leaflet の animate オプション。reduced-motion では false。 */
  animate: boolean;
}

/**
 * 候補表示時の地図フィットに使うアニメーション指定。
 * prefers-reduced-motion では `animate: false`（design F4）。
 */
export function fitBoundsAnimation(reducedMotion: boolean): { animate: boolean } {
  return { animate: !reducedMotion };
}

/**
 * 出発地点が既に表示範囲内なら動かさない（無駄な移動をしない）。
 * 範囲外なら追従し、現在ズームが FOCUS_ORIGIN_MIN_ZOOM 未満なら合わせて寄せる。
 * prefers-reduced-motion ではアニメーションしない。
 */
export function planOriginFocus(
  insideBounds: boolean,
  currentZoom: number,
  reducedMotion: boolean,
): OriginFocusPlan | null {
  if (insideBounds) {
    return null;
  }
  const animate = !reducedMotion;
  if (currentZoom < FOCUS_ORIGIN_MIN_ZOOM) {
    return { action: "setView", zoom: FOCUS_ORIGIN_MIN_ZOOM, animate };
  }
  return { action: "pan", zoom: currentZoom, animate };
}
