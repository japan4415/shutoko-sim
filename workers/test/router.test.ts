import { createExecutionContext, env, waitOnExecutionContext } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import worker from "../src/index";

describe("Router & Fallback", () => {
  it("returns 404 for unknown route", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/unknown/route");
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(404);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({
      code: "NOT_FOUND",
      retryable: false,
    });
  });

  it("returns 405 for GET /api/geocode", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "GET",
    });
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(405);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({
      code: "METHOD_NOT_ALLOWED",
      retryable: false,
    });
  });

  it("returns 405 for PUT /api/geocode", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/api/geocode", {
      method: "PUT",
    });
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(405);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({
      code: "METHOD_NOT_ALLOWED",
      retryable: false,
    });
  });

  describe("URL normalization & malformed paths", () => {
    it("returns 404 with no-store for double slash (leading, middle, trailing)", async () => {
      const doubleSlashCases = [
        "http://localhost//releases/c1-real-v1/manifest.json",
        "http://localhost/releases//c1-real-v1/manifest.json",
        "http://localhost/releases/c1-real-v1//manifest.json",
        "http://localhost/releases/c1-real-v1/manifest.json/",
        "http://localhost/releases/c1-real-v1/manifest.json//",
      ];

      for (const url of doubleSlashCases) {
        const ctx = createExecutionContext();
        const req = new Request(url);
        const res = await worker.fetch(req, env, ctx);
        await waitOnExecutionContext(ctx);

        expect(res.status, `Expected 404 for ${url}`).toBe(404);
        expect(res.headers.get("Cache-Control")).toBe("no-store");
        const data: any = await res.json();
        expect(data.error).toEqual({
          code: "NOT_FOUND",
          retryable: false,
        });
      }
    });

    it("returns 404 with no-store for %2F, %5C, %2E%2E, and backslash", async () => {
      const invalidEncodedCases = [
        "http://localhost/releases%2Fc1-real-v1/manifest.json",
        "http://localhost/releases/c1%2Freal-v1/manifest.json",
        "http://localhost/releases/c1-real-v1%2Fmanifest.json",
        "http://localhost/releases%5Cc1-real-v1/manifest.json",
        "http://localhost/releases/c1-real-v1%5Cmanifest.json",
        "http://localhost/releases/c1-real-v1/%2E%2E/manifest.json",
        "http://localhost/releases/%2e%2e/c1-real-v1/manifest.json",
        "http://localhost/releases/c1-real-v1%2emanifest.json",
        "http://localhost/releases\\c1-real-v1/manifest.json",
      ];

      for (const url of invalidEncodedCases) {
        const ctx = createExecutionContext();
        const req = new Request(url);
        const res = await worker.fetch(req, env, ctx);
        await waitOnExecutionContext(ctx);

        expect(res.status, `Expected 404 for ${url}`).toBe(404);
        expect(res.headers.get("Cache-Control")).toBe("no-store");
        const data: any = await res.json();
        expect(data.error).toEqual({
          code: "NOT_FOUND",
          retryable: false,
        });
      }
    });

    it("returns 404 with no-store for /api/geocode/ (trailing slash)", async () => {
      const ctx = createExecutionContext();
      const req = new Request("http://localhost/api/geocode/", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ query: "東京" }),
      });
      const res = await worker.fetch(req, env, ctx);
      await waitOnExecutionContext(ctx);

      expect(res.status).toBe(404);
      expect(res.headers.get("Cache-Control")).toBe("no-store");
      const data: any = await res.json();
      expect(data.error).toEqual({
        code: "NOT_FOUND",
        retryable: false,
      });
    });

    it("returns 404 with no-store when segment count is not 3 for /releases", async () => {
      const badSegmentCases = [
        "http://localhost/releases",
        "http://localhost/releases/c1-real-v1",
        "http://localhost/releases/c1-real-v1/extra/manifest.json",
      ];

      for (const url of badSegmentCases) {
        const ctx = createExecutionContext();
        const req = new Request(url);
        const res = await worker.fetch(req, env, ctx);
        await waitOnExecutionContext(ctx);

        expect(res.status, `Expected 404 for ${url}`).toBe(404);
        expect(res.headers.get("Cache-Control")).toBe("no-store");
        const data: any = await res.json();
        expect(data.error).toEqual({
          code: "NOT_FOUND",
          retryable: false,
        });
      }
    });
  });
});
