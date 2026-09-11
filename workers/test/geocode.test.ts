import { createExecutionContext, env, waitOnExecutionContext } from "cloudflare:test";
import { afterEach, describe, expect, it, vi } from "vitest";
import worker from "../src/index";

const DUMMY_API_KEY = "mock-secret-key-12345";
const SECRET_UPSTREAM_MARKER = "INTERNAL_UPSTREAM_TRACE_SECRET_99999";

describe("Geocode proxy (POST /api/geocode)", () => {
  const testEnv = {
    ...env,
    GEOCODER_API_KEY: DUMMY_API_KEY,
  };

  afterEach(() => {
    vi.restoreAllMocks();
  });

  async function assertNoSecretLeak(res: Response, bodyText: string) {
    expect(bodyText).not.toContain(DUMMY_API_KEY);
    expect(bodyText).not.toContain(SECRET_UPSTREAM_MARKER);
    for (const [key, val] of res.headers.entries()) {
      expect(key).not.toContain(DUMMY_API_KEY);
      expect(val).not.toContain(DUMMY_API_KEY);
      expect(key).not.toContain(SECRET_UPSTREAM_MARKER);
      expect(val).not.toContain(SECRET_UPSTREAM_MARKER);
    }
  }

  it("normal geocode returns normalized candidates (max 5, [lon, lat] -> {lat, lon}) with 200 and Cache-Control: no-store", async () => {
    const mockGsiResponse = [
      {
        geometry: { type: "Point", coordinates: [139.762329, 35.685982] },
        properties: { title: "東京都千代田区大手町一丁目１番" },
      },
      {
        geometry: { type: "Point", coordinates: [139.767, 35.681] },
        properties: { title: "東京都千代田区丸の内一丁目" },
      },
      {
        geometry: { type: "Point", coordinates: [139.75, 35.68] },
        properties: { title: "東京都千代田区千代田" },
      },
      {
        geometry: { type: "Point", coordinates: [139.74, 35.67] },
        properties: { title: "東京都千代田区霞が関一丁目" },
      },
      {
        geometry: { type: "Point", coordinates: [139.73, 35.66] },
        properties: { title: "東京都港区新橋一丁目" },
      },
      {
        geometry: { type: "Point", coordinates: [139.72, 35.65] },
        properties: { title: "東京都港区芝公園一丁目" }, // 6th item (should be truncated)
      },
    ];

    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response(JSON.stringify(mockGsiResponse), { status: 200 }))
    );

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "東京都千代田区" }),
    });

    const res = await worker.fetch(req, testEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    expect(res.headers.get("Content-Type")).toBe("application/json; charset=utf-8");

    const text = await res.text();
    await assertNoSecretLeak(res, text);

    const data = JSON.parse(text);
    expect(data.candidates).toHaveLength(5);
    expect(data.candidates[0]).toEqual({
      label: "東京都千代田区大手町一丁目１番",
      lat: 35.685982,
      lon: 139.762329,
    });
  });

  it("returns 200 with empty candidates array when 0 results found", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response(JSON.stringify([]), { status: 200 }))
    );

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "存在しない架空住所9999" }),
    });

    const res = await worker.fetch(req, testEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    expect(res.headers.get("Cache-Control")).toBe("no-store");

    const text = await res.text();
    await assertNoSecretLeak(res, text);

    const data = JSON.parse(text);
    expect(data.candidates).toEqual([]);
  });

  it("returns 400 PAYLOAD_TOO_LARGE when payload exceeds 4,096 bytes", async () => {
    const longQuery = "a".repeat(4100);
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: longQuery }),
    });

    const res = await worker.fetch(req, testEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    expect(res.headers.get("Cache-Control")).toBe("no-store");

    const text = await res.text();
    await assertNoSecretLeak(res, text);

    const data = JSON.parse(text);
    expect(data.error).toEqual({
      code: "PAYLOAD_TOO_LARGE",
      retryable: false,
    });
  });

  it("returns 400 INVALID_QUERY on invalid JSON", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{ not valid json",
    });

    const res = await worker.fetch(req, testEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    expect(res.headers.get("Cache-Control")).toBe("no-store");

    const text = await res.text();
    await assertNoSecretLeak(res, text);

    const data = JSON.parse(text);
    expect(data.error).toEqual({
      code: "INVALID_QUERY",
      retryable: false,
    });
  });

  it("returns 400 INVALID_QUERY on empty query or query > 200 chars", async () => {
    const ctx = createExecutionContext();

    // Empty query
    const reqEmpty = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "   " }),
    });
    const resEmpty = await worker.fetch(reqEmpty, testEnv, ctx);
    expect(resEmpty.status).toBe(400);
    const dataEmpty: any = await resEmpty.json();
    expect(dataEmpty.error.code).toBe("INVALID_QUERY");
    expect(dataEmpty.error.retryable).toBe(false);

    // Missing query field
    const reqMissing = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    const resMissing = await worker.fetch(reqMissing, testEnv, ctx);
    expect(resMissing.status).toBe(400);
    const dataMissing: any = await resMissing.json();
    expect(dataMissing.error.code).toBe("INVALID_QUERY");

    // 201 chars query
    const req201 = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "あ".repeat(201) }),
    });
    const res201 = await worker.fetch(req201, testEnv, ctx);
    expect(res201.status).toBe(400);
    const data201: any = await res201.json();
    expect(data201.error.code).toBe("INVALID_QUERY");

    await waitOnExecutionContext(ctx);
  });

  it("returns 502 GEOCODER_UNAVAILABLE on upstream 500 without leaking upstream message", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(`Database error: ${SECRET_UPSTREAM_MARKER}`, {
          status: 500,
        })
      )
    );

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "東京都千代田区" }),
    });

    const res = await worker.fetch(req, testEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(502);
    expect(res.headers.get("Cache-Control")).toBe("no-store");

    const text = await res.text();
    await assertNoSecretLeak(res, text);

    const data = JSON.parse(text);
    expect(data.error).toEqual({
      code: "GEOCODER_UNAVAILABLE",
      retryable: true,
    });
  });

  it("returns 504 GEOCODER_TIMEOUT when upstream times out (AbortError)", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((_url: string, init?: RequestInit) => {
        return new Promise((_, reject) => {
          if (init?.signal) {
            init.signal.addEventListener("abort", () => {
              const err = new Error("The operation was aborted");
              err.name = "AbortError";
              reject(err);
            });
          }
          // Immediate abort to simulate 5s timeout trigger
          const err = new Error("The operation was aborted");
          err.name = "AbortError";
          reject(err);
        });
      })
    );

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "東京都千代田区" }),
    });

    const res = await worker.fetch(req, testEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(504);
    expect(res.headers.get("Cache-Control")).toBe("no-store");

    const text = await res.text();
    await assertNoSecretLeak(res, text);

    const data = JSON.parse(text);
    expect(data.error).toEqual({
      code: "GEOCODER_TIMEOUT",
      retryable: true,
    });
  });

  it("returns 429 RATE_LIMITED with Retry-After: 60 when rate limit is exceeded", async () => {
    const mockEnv = {
      ...testEnv,
      IP_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: false }),
      } as unknown as RateLimit,
    };

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ query: "東京都千代田区" }),
    });

    const res = await worker.fetch(req, mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(429);
    expect(res.headers.get("Retry-After")).toBe("60");
    expect(res.headers.get("Cache-Control")).toBe("no-store");

    const text = await res.text();
    await assertNoSecretLeak(res, text);

    const data = JSON.parse(text);
    expect(data.error).toEqual({
      code: "RATE_LIMITED",
      retryable: true,
    });
  });

  describe("Fail-closed rate limiter behavior", () => {
    it("returns 503 RATE_LIMITER_UNAVAILABLE when IP_RATE_LIMITER is undefined and does not call upstream", async () => {
      const fetchSpy = vi.fn();
      vi.stubGlobal("fetch", fetchSpy);

      const mockEnv = {
        ...testEnv,
        IP_RATE_LIMITER: undefined as unknown as RateLimit,
      };

      const ctx = createExecutionContext();
      const req = new Request("http://localhost/api/geocode", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: "東京都千代田区" }),
      });

      const res = await worker.fetch(req, mockEnv, ctx);
      await waitOnExecutionContext(ctx);

      expect(res.status).toBe(503);
      expect(res.headers.get("Cache-Control")).toBe("no-store");

      const text = await res.text();
      await assertNoSecretLeak(res, text);

      const data = JSON.parse(text);
      expect(data.error).toEqual({
        code: "RATE_LIMITER_UNAVAILABLE",
        retryable: true,
      });
      expect(fetchSpy).not.toHaveBeenCalled();
    });

    it("returns 503 RATE_LIMITER_UNAVAILABLE when GLOBAL_RATE_LIMITER is undefined and does not call upstream", async () => {
      const fetchSpy = vi.fn();
      vi.stubGlobal("fetch", fetchSpy);

      const mockEnv = {
        ...testEnv,
        GLOBAL_RATE_LIMITER: undefined as unknown as RateLimit,
      };

      const ctx = createExecutionContext();
      const req = new Request("http://localhost/api/geocode", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: "東京都千代田区" }),
      });

      const res = await worker.fetch(req, mockEnv, ctx);
      await waitOnExecutionContext(ctx);

      expect(res.status).toBe(503);
      expect(res.headers.get("Cache-Control")).toBe("no-store");

      const text = await res.text();
      await assertNoSecretLeak(res, text);

      const data = JSON.parse(text);
      expect(data.error).toEqual({
        code: "RATE_LIMITER_UNAVAILABLE",
        retryable: true,
      });
      expect(fetchSpy).not.toHaveBeenCalled();
    });

    it("returns 503 RATE_LIMITER_UNAVAILABLE when limit() throws/rejects and does not call upstream", async () => {
      const fetchSpy = vi.fn();
      vi.stubGlobal("fetch", fetchSpy);

      const mockEnv = {
        ...testEnv,
        IP_RATE_LIMITER: {
          limit: vi.fn().mockRejectedValue(new Error("Rate limit service connection error")),
        } as unknown as RateLimit,
      };

      const ctx = createExecutionContext();
      const req = new Request("http://localhost/api/geocode", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: "東京都千代田区" }),
      });

      const res = await worker.fetch(req, mockEnv, ctx);
      await waitOnExecutionContext(ctx);

      expect(res.status).toBe(503);
      expect(res.headers.get("Cache-Control")).toBe("no-store");

      const text = await res.text();
      await assertNoSecretLeak(res, text);

      const data = JSON.parse(text);
      expect(data.error).toEqual({
        code: "RATE_LIMITER_UNAVAILABLE",
        retryable: true,
      });
      expect(fetchSpy).not.toHaveBeenCalled();
    });

    it("returns 503 RATE_LIMITER_UNAVAILABLE when GLOBAL_RATE_LIMITER.limit() throws/rejects", async () => {
      const fetchSpy = vi.fn();
      vi.stubGlobal("fetch", fetchSpy);

      const mockEnv = {
        ...testEnv,
        IP_RATE_LIMITER: {
          limit: vi.fn().mockResolvedValue({ success: true }),
        } as unknown as RateLimit,
        GLOBAL_RATE_LIMITER: {
          limit: vi.fn().mockRejectedValue(new Error("Global rate limit service error")),
        } as unknown as RateLimit,
      };

      const ctx = createExecutionContext();
      const req = new Request("http://localhost/api/geocode", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: "東京都千代田区" }),
      });

      const res = await worker.fetch(req, mockEnv, ctx);
      await waitOnExecutionContext(ctx);

      expect(res.status).toBe(503);
      expect(res.headers.get("Cache-Control")).toBe("no-store");

      const text = await res.text();
      await assertNoSecretLeak(res, text);

      const data = JSON.parse(text);
      expect(data.error).toEqual({
        code: "RATE_LIMITER_UNAVAILABLE",
        retryable: true,
      });
      expect(fetchSpy).not.toHaveBeenCalled();
    });
  });
});
