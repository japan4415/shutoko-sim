import { createExecutionContext, env, waitOnExecutionContext } from "cloudflare:test";
import { afterEach, describe, expect, it, vi } from "vitest";
import worker from "../src/index";

/** 妥当な envelope（最小限。workers 側は構造だけ検証する）。 */
function validEnvelope(deviceName: string | null = "Pixel 8a"): Record<string, unknown> {
  return {
    schemaVersion: 1,
    createdAt: "2026-09-12T04:07:32.123Z",
    releaseId: "c1-real-v1",
    device: {
      ua: "Mozilla/5.0",
      platform: "Linux armv8l",
      deviceName,
      os: "Android 14",
      browser: "Chrome 130",
      network: "Slow 4G",
      note: null,
    },
    memorySource: null,
    memoryManualMiB: null,
    pageLoad: { url: "http://localhost/bench.html", navigation: null, resources: [] },
    targets: {
      searchP95Ms: 2000,
      firstLoadP95Ms: 8000,
      transferBytesMax: 10485760,
      memoryMiBMax: 128,
    },
    trials: [],
  };
}

function postBench(body: string, headers: Record<string, string> = {}): Request {
  return new Request("http://localhost/api/bench-result", {
    method: "POST",
    headers: { "Content-Type": "application/json", ...headers },
    body,
  });
}

describe("Bench result upload (POST /api/bench-result)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("returns 405 METHOD_NOT_ALLOWED for GET /api/bench-result", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/bench-result", { method: "GET" });
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(405);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "METHOD_NOT_ALLOWED", retryable: false });
  });

  it("stores a valid envelope in R2 and returns key/bytes (200)", async () => {
    const raw = JSON.stringify(validEnvelope());
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(raw), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("application/json; charset=utf-8");
    expect(res.headers.get("Cache-Control")).toBe("no-store");

    const data: any = await res.json();
    expect(typeof data.key).toBe("string");
    expect(data.key.startsWith("bench/")).toBe(true);
    expect(data.key.endsWith(".json")).toBe(true);
    expect(data.bytes).toBe(new TextEncoder().encode(raw).byteLength);

    const obj = await env.ARTIFACTS_BUCKET.get(data.key);
    expect(obj).not.toBeNull();
    const stored = await obj!.text();
    // 生ボディをそのまま保存している（再 stringify していない）。
    expect(stored).toBe(raw);
  });

  it("returns 400 INVALID_BENCH_PAYLOAD when schemaVersion is missing", async () => {
    const envelope = validEnvelope();
    delete envelope.schemaVersion;
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(envelope)), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INVALID_BENCH_PAYLOAD", retryable: false });
  });

  it("returns 400 INVALID_BENCH_PAYLOAD when trials is not an array", async () => {
    const envelope = validEnvelope();
    envelope.trials = "not-an-array";
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(envelope)), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INVALID_BENCH_PAYLOAD", retryable: false });
  });

  it("returns 400 INVALID_BENCH_PAYLOAD when device is not an object", async () => {
    const envelope = validEnvelope();
    envelope.device = "not-an-object";
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(envelope)), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INVALID_BENCH_PAYLOAD", retryable: false });
  });

  it("returns 400 INVALID_BENCH_PAYLOAD on invalid JSON", async () => {
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench("{ not valid json"), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INVALID_BENCH_PAYLOAD", retryable: false });
  });

  it("returns 400 PAYLOAD_TOO_LARGE when Content-Length exceeds 8 MiB", async () => {
    // 実体は送らず Content-Length だけを巨大にする（事前チェックの検証）。
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/bench-result", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Content-Length": String(8 * 1024 * 1024 + 1),
      },
      body: "{}",
    });
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "PAYLOAD_TOO_LARGE", retryable: false });
  });

  it("sanitizes device slug so traversal characters are not added to the key", async () => {
    const envelope = validEnvelope("../../etc/passwd");
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(envelope)), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    const data: any = await res.json();
    const key: string = data.key;
    expect(key.startsWith("bench/")).toBe(true);
    // `bench/` 直後と `.json` を除いた部分に `..` も `/` も含まれない。
    const middle = key.slice("bench/".length, -".json".length);
    expect(middle.includes("..")).toBe(false);
    expect(middle.includes("/")).toBe(false);
    expect(middle.includes("etcpasswd")).toBe(true);
  });

  it("falls back to browser slug when deviceName is null", async () => {
    const envelope = validEnvelope(null);
    (envelope.device as Record<string, unknown>).browser = "Chrome 130";
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(envelope)), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    const data: any = await res.json();
    expect(data.key.startsWith("bench/")).toBe(true);
    expect(data.key.includes("-Chrome130.json")).toBe(true);
  });

  it("returns 400 INVALID_BENCH_PAYLOAD for an empty body", async () => {
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(""), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INVALID_BENCH_PAYLOAD", retryable: false });
  });

  it("returns 400 INVALID_BENCH_PAYLOAD when schemaVersion is a string", async () => {
    const envelope = validEnvelope();
    envelope.schemaVersion = "1";
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(envelope)), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(400);
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INVALID_BENCH_PAYLOAD", retryable: false });
  });

  it("stores an envelope with an empty trials array (200)", async () => {
    const envelope = validEnvelope();
    envelope.trials = [];
    const raw = JSON.stringify(envelope);
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(raw), env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    const data: any = await res.json();
    expect(typeof data.key).toBe("string");
    expect(data.bytes).toBe(new TextEncoder().encode(raw).byteLength);
  });

  it("returns 429 RATE_LIMITED with Retry-After: 60 when IP limit is exceeded", async () => {
    const mockEnv = {
      ...env,
      IP_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: false }),
      } as unknown as RateLimit,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(validEnvelope())), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(429);
    expect(res.headers.get("Retry-After")).toBe("60");
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "RATE_LIMITED", retryable: true });
  });

  it("returns 429 RATE_LIMITED before reading the body when limited", async () => {
    // 巨大ボディを確保せず Content-Length 無しの小さなボディで、レート制限が
    // ボディ読込より前に評価されることを確認する。
    const mockEnv = {
      ...env,
      IP_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: false }),
      } as unknown as RateLimit,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench("{ not valid json"), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(429);
    expect(res.headers.get("Retry-After")).toBe("60");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "RATE_LIMITED", retryable: true });
  });

  it("returns 503 RATE_LIMITER_UNAVAILABLE when the IP rate limiter is missing", async () => {
    const mockEnv = {
      ...env,
      IP_RATE_LIMITER: undefined as unknown as RateLimit,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(validEnvelope())), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(503);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "RATE_LIMITER_UNAVAILABLE", retryable: true });
  });

  it("returns 503 RATE_LIMITER_UNAVAILABLE when GLOBAL_RATE_LIMITER is undefined", async () => {
    const mockEnv = {
      ...env,
      GLOBAL_RATE_LIMITER: undefined as unknown as RateLimit,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(validEnvelope())), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(503);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "RATE_LIMITER_UNAVAILABLE", retryable: true });
  });

  it("returns 503 RATE_LIMITER_UNAVAILABLE when GLOBAL_RATE_LIMITER.limit() rejects", async () => {
    const mockEnv = {
      ...env,
      IP_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: true }),
      } as unknown as RateLimit,
      GLOBAL_RATE_LIMITER: {
        limit: vi.fn().mockRejectedValue(new Error("Global rate limit service error")),
      } as unknown as RateLimit,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(validEnvelope())), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(503);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "RATE_LIMITER_UNAVAILABLE", retryable: true });
  });

  it("returns 429 RATE_LIMITED when the global limit is exceeded", async () => {
    const mockEnv = {
      ...env,
      IP_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: true }),
      } as unknown as RateLimit,
      GLOBAL_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: false }),
      } as unknown as RateLimit,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(validEnvelope())), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(429);
    expect(res.headers.get("Retry-After")).toBe("60");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "RATE_LIMITED", retryable: true });
  });

  it("returns 500 INTERNAL_ERROR (not retryable) when ARTIFACTS_BUCKET is missing", async () => {
    const mockEnv = {
      ...env,
      IP_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: true }),
      } as unknown as RateLimit,
      GLOBAL_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: true }),
      } as unknown as RateLimit,
      ARTIFACTS_BUCKET: undefined as unknown as R2Bucket,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(validEnvelope())), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(500);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INTERNAL_ERROR", retryable: false });
  });

  it("returns 500 INTERNAL_ERROR (retryable) when bucket.put rejects", async () => {
    const mockEnv = {
      ...env,
      IP_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: true }),
      } as unknown as RateLimit,
      GLOBAL_RATE_LIMITER: {
        limit: vi.fn().mockResolvedValue({ success: true }),
      } as unknown as RateLimit,
      ARTIFACTS_BUCKET: {
        ...env.ARTIFACTS_BUCKET,
        head: vi.fn().mockResolvedValue(null),
        put: vi.fn().mockRejectedValue(new Error("R2 temporary failure")),
      } as unknown as R2Bucket,
    };
    const ctx = createExecutionContext();
    const res = await worker.fetch(postBench(JSON.stringify(validEnvelope())), mockEnv, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(500);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "INTERNAL_ERROR", retryable: true });
  });
});
