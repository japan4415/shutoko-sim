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
});
