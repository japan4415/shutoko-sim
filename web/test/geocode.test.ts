// geocode クライアントのユニットテスト（node 環境、fetch は注入モック。ネットワークは叩かない）。
import { describe, expect, it, vi } from "vitest";
import { GeocodeError, geocode, type GeocodeCandidate } from "../src/geocode";

/** JSON ボディを返す簡易 Response 互換モック。 */
function mockResponse(status: number, body: unknown, isJson = true): Response {
  const text = isJson ? JSON.stringify(body) : String(body);
  return {
    ok: status >= 200 && status < 300,
    status,
    async json(): Promise<unknown> {
      if (!isJson) {
        throw new Error("invalid json");
      }
      return body;
    },
    async text(): Promise<string> {
      return text;
    },
  } as unknown as Response;
}

/** 呼び出しを記録する fetch モック。 */
function recordingFetch(response: Response): { fetchFn: typeof fetch; calls: unknown[] } {
  const calls: unknown[] = [];
  const fetchFn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ input, init });
    return response;
  }) as unknown as typeof fetch;
  return { fetchFn, calls };
}

describe("geocode", () => {
  it("正常系: candidates をそのまま返す", async () => {
    const candidates: GeocodeCandidate[] = [
      { label: "東京都千代田区大手町", lat: 35.685982, lon: 139.762329 },
      { label: "東京都千代田区丸の内", lat: 35.681, lon: 139.767 },
    ];
    const { fetchFn } = recordingFetch(mockResponse(200, { candidates }));

    await expect(geocode("東京都千代田区", fetchFn)).resolves.toEqual(candidates);
  });

  it("空候補配列も正常系として返す", async () => {
    const { fetchFn } = recordingFetch(mockResponse(200, { candidates: [] }));
    await expect(geocode("存在しない住所", fetchFn)).resolves.toEqual([]);
  });

  it("リクエストのボディ・ヘッダ・メソッドが正しい", async () => {
    const { fetchFn, calls } = recordingFetch(mockResponse(200, { candidates: [] }));
    await geocode("東京都千代田区", fetchFn);

    expect(calls).toHaveLength(1);
    const call = calls[0] as { input: unknown; init: RequestInit };
    expect(call.input).toBe("/api/geocode");
    expect(call.init.method).toBe("POST");
    expect((call.init.headers as Record<string, string>)["Content-Type"]).toBe("application/json");
    expect(call.init.body).toBe(JSON.stringify({ query: "東京都千代田区" }));
  });

  it("signal を fetch へ渡す", async () => {
    const { fetchFn, calls } = recordingFetch(mockResponse(200, { candidates: [] }));
    const controller = new AbortController();
    await geocode("東京都千代田区", fetchFn, controller.signal);
    const call = calls[0] as { init: RequestInit };
    expect(call.init.signal).toBe(controller.signal);
  });

  it("400 INVALID_QUERY は code を保持して throw", async () => {
    const { fetchFn } = recordingFetch(
      mockResponse(400, { error: { code: "INVALID_QUERY", retryable: false } }),
    );
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({
      code: "INVALID_QUERY",
      retryable: false,
    });
  });

  it("429 RATE_LIMITED は retryable: true で throw", async () => {
    const { fetchFn } = recordingFetch(
      mockResponse(429, { error: { code: "RATE_LIMITED", retryable: true } }),
    );
    await expect(geocode("x", fetchFn)).rejects.toBeInstanceOf(GeocodeError);
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({
      code: "RATE_LIMITED",
      retryable: true,
    });
  });

  it("504 GEOCODER_TIMEOUT を透過", async () => {
    const { fetchFn } = recordingFetch(
      mockResponse(504, { error: { code: "GEOCODER_TIMEOUT", retryable: true } }),
    );
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({
      code: "GEOCODER_TIMEOUT",
      retryable: true,
    });
  });

  it("502 GEOCODER_UNAVAILABLE を透過", async () => {
    const { fetchFn } = recordingFetch(
      mockResponse(502, { error: { code: "GEOCODER_UNAVAILABLE", retryable: true } }),
    );
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({
      code: "GEOCODER_UNAVAILABLE",
      retryable: true,
    });
  });

  it("503 RATE_LIMITER_UNAVAILABLE を透過", async () => {
    const { fetchFn } = recordingFetch(
      mockResponse(503, { error: { code: "RATE_LIMITER_UNAVAILABLE", retryable: true } }),
    );
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({
      code: "RATE_LIMITER_UNAVAILABLE",
      retryable: true,
    });
  });

  it("非 JSON のエラー本文は FETCH_FAILED", async () => {
    const { fetchFn } = recordingFetch(mockResponse(502, "upstream exploded", false));
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({
      code: "FETCH_FAILED",
      retryable: true,
    });
  });

  it("ネットワーク失敗（reject）は FETCH_FAILED", async () => {
    const fetchFn = vi.fn(async () => {
      throw new Error("offline");
    }) as unknown as typeof fetch;
    await expect(geocode("x", fetchFn)).rejects.toBeInstanceOf(GeocodeError);
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({
      code: "FETCH_FAILED",
      retryable: true,
    });
  });

  it("中断（AbortError）は FETCH_FAILED", async () => {
    const fetchFn = vi.fn(async () => {
      const err = new Error("The operation was aborted");
      err.name = "AbortError";
      throw err;
    }) as unknown as typeof fetch;
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({ code: "FETCH_FAILED" });
  });

  it("candidates が配列でない成功本文は INVALID_RESPONSE", async () => {
    const { fetchFn } = recordingFetch(mockResponse(200, { candidates: {} }));
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
  });

  it("label 欠落・非文字列は INVALID_RESPONSE", async () => {
    const { fetchFn } = recordingFetch(
      mockResponse(200, { candidates: [{ lat: 35.6, lon: 139.7 }] }),
    );
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
  });

  it("有限でない lat/lon は INVALID_RESPONSE", async () => {
    const cases: unknown[] = [
      { label: "a", lat: Number.NaN, lon: 139.7 },
      { label: "a", lat: 35.6, lon: Number.POSITIVE_INFINITY },
      { label: "a", lat: "35.6", lon: 139.7 },
    ];
    for (const candidate of cases) {
      const { fetchFn } = recordingFetch(mockResponse(200, { candidates: [candidate] }));
      await expect(geocode("x", fetchFn)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
    }
  });

  it("範囲外の lat/lon は INVALID_RESPONSE", async () => {
    const cases: unknown[] = [
      { label: "a", lat: 91, lon: 139.7 },
      { label: "a", lat: -91, lon: 139.7 },
      { label: "a", lat: 35.6, lon: 181 },
      { label: "a", lat: 35.6, lon: -181 },
    ];
    for (const candidate of cases) {
      const { fetchFn } = recordingFetch(mockResponse(200, { candidates: [candidate] }));
      await expect(geocode("x", fetchFn)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
    }
  });

  it("成功本文が JSON でなければ INVALID_RESPONSE", async () => {
    const { fetchFn } = recordingFetch(mockResponse(200, "not json", false));
    await expect(geocode("x", fetchFn)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
  });

  it("プライバシー: クエリ文字列が例外メッセージに含まれない", async () => {
    const secretQuery = "東京都秘密住所12345";
    const responses: Response[] = [
      mockResponse(400, { error: { code: "INVALID_QUERY", retryable: false } }),
      mockResponse(200, { candidates: [{ label: 1, lat: Number.NaN, lon: 0 }] }),
      mockResponse(502, "upstream exploded", false),
    ];
    for (const response of responses) {
      const { fetchFn } = recordingFetch(response);
      try {
        await geocode(secretQuery, fetchFn);
        throw new Error("expected rejection");
      } catch (err) {
        const error = err as Error;
        expect(error.message).not.toContain(secretQuery);
        expect(error.message).not.toContain("東京都");
      }
    }
  });
});
