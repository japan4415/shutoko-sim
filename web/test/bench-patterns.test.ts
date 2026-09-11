// `?patterns=` の解決（selectPatterns）のユニットテスト。
// 範囲外・非整数を黙って全件へフォールバックさせないこと（verify-001 G-2）を境界値で固定する。
import { describe, expect, it } from "vitest";
import { BENCH_PATTERNS, selectPatterns } from "../src/bench/patterns";

/** 解決結果を index の配列にして比較しやすくする。 */
function indexes(spec: string | null): number[] {
  return selectPatterns(spec).map((pattern) => pattern.index);
}

describe("selectPatterns の既定", () => {
  it("未指定（null）は全 30 件を返す", () => {
    expect(indexes(null)).toEqual(Array.from({ length: 30 }, (_, index) => index));
  });

  it("空文字・空白のみは全 30 件を返す（未指定と同じ扱い）", () => {
    expect(indexes("")).toHaveLength(30);
    expect(indexes("   ")).toHaveLength(30);
  });

  it("BENCH_PATTERNS は 30 件で index が 0..29 と一致する", () => {
    expect(BENCH_PATTERNS).toHaveLength(30);
    expect(BENCH_PATTERNS.map((pattern) => pattern.index)).toEqual(
      Array.from({ length: 30 }, (_, index) => index),
    );
  });
});

describe("selectPatterns の有効値", () => {
  it("指定した index だけを昇順（BENCH_PATTERNS の順）で返す", () => {
    expect(indexes("0,5")).toEqual([0, 5]);
    expect(indexes("29,0")).toEqual([0, 29]);
  });

  it("重複した index は 1 件に畳む", () => {
    expect(indexes("3,3,3")).toEqual([3]);
  });

  it("前後の空白は無視する", () => {
    expect(indexes(" 0 , 7 ")).toEqual([0, 7]);
  });
});

describe("selectPatterns の境界（範囲外・非整数は無視する）", () => {
  it("範囲外・非整数のみの指定は空配列（全件へフォールバックしない）", () => {
    for (const spec of ["99", "30", "-1", "1.5", "abc"]) {
      expect(indexes(spec), `spec=${spec}`).toEqual([]);
    }
  });

  it("有効と範囲外が混ざったら有効な index だけを返す", () => {
    expect(indexes("0,99")).toEqual([0]);
    expect(indexes("99,0")).toEqual([0]);
    expect(indexes("30,-1,2,abc,1.5")).toEqual([2]);
  });

  it("境界の 0 と 29 は有効、30 と -1 は無効", () => {
    expect(indexes("0")).toEqual([0]);
    expect(indexes("29")).toEqual([29]);
    expect(indexes("30")).toEqual([]);
    expect(indexes("-1")).toEqual([]);
  });

  it("数値に見える別表記（指数・16 進・符号・空要素）は有効な index とみなさない", () => {
    for (const spec of ["1e1", "0x10", "+5", "5.", ".5"]) {
      expect(indexes(spec), `spec=${spec}`).toEqual([]);
    }
    // "0," の空要素は無視され、有効な 0 だけが残る。
    expect(indexes("0,")).toEqual([0]);
    expect(indexes("5,,7")).toEqual([5, 7]);
  });
});
