// 出発地点確定時の地図追従判定（src/map/focus.ts）のユニットテスト。
// Leaflet に依存しない純粋関数なので、bounds 内外と reduced-motion の分岐だけを固定する。
import { describe, expect, it } from "vitest";
import { FOCUS_ORIGIN_MIN_ZOOM, fitBoundsAnimation, planOriginFocus } from "../src/map/focus";

describe("planOriginFocus", () => {
  it("既に表示範囲内なら動かさない（無駄な移動をしない）", () => {
    expect(planOriginFocus(true, 12, false)).toBeNull();
    expect(planOriginFocus(true, 16, true)).toBeNull();
  });

  it("範囲外かつ低ズームなら setView で寄せる", () => {
    expect(planOriginFocus(false, 12, false)).toEqual({
      action: "setView",
      zoom: FOCUS_ORIGIN_MIN_ZOOM,
      animate: true,
    });
  });

  it("範囲外でも十分に寄っていれば pan のみ（ズームを変えない）", () => {
    expect(planOriginFocus(false, 15, false)).toEqual({ action: "pan", zoom: 15, animate: true });
    expect(planOriginFocus(false, FOCUS_ORIGIN_MIN_ZOOM, false)).toEqual({
      action: "pan",
      zoom: FOCUS_ORIGIN_MIN_ZOOM,
      animate: true,
    });
  });

  it("prefers-reduced-motion では animate:false（移動自体はする）", () => {
    expect(planOriginFocus(false, 12, true)).toEqual({
      action: "setView",
      zoom: FOCUS_ORIGIN_MIN_ZOOM,
      animate: false,
    });
    expect(planOriginFocus(false, 15, true)).toEqual({ action: "pan", zoom: 15, animate: false });
  });
});

describe("fitBoundsAnimation", () => {
  it("通常はアニメーションし、prefers-reduced-motion では止める（design F4）", () => {
    expect(fitBoundsAnimation(false)).toEqual({ animate: true });
    expect(fitBoundsAnimation(true)).toEqual({ animate: false });
  });
});
