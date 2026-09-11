import { createExecutionContext, env, waitOnExecutionContext } from "cloudflare:test";
import { beforeAll, describe, expect, it } from "vitest";
import realGraphJsonText from "../../fixtures/generated/graph.json?raw";
import worker from "../src/index";

describe("Releases delivery", () => {
  beforeAll(async () => {
    // Seed R2 bucket
    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/manifest.json",
      JSON.stringify({ schemaVersion: 1, releaseId: "c1-real-v1" }),
      {
        httpMetadata: {
          contentType: "application/json; charset=utf-8",
        },
      }
    );

    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/engine.json",
      JSON.stringify({ schemaVersion: 1, releaseId: "c1-real-v1", artifacts: [] }),
      {
        httpMetadata: {
          contentType: "application/json; charset=utf-8",
        },
      }
    );

    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/graph.json",
      JSON.stringify({ nodes: [], edges: [] })
    );

    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/snap-index.json",
      JSON.stringify({ items: [] })
    );

    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/shutoko_routing_bg.wasm",
      new Uint8Array([0x00, 0x61, 0x73, 0x6d])
    );

    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/shutoko_routing.js",
      "export default function() {}"
    );

    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/shutoko_routing.d.ts",
      "export declare function init(): void;"
    );

    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/index.d.ts",
      "export * from './shutoko_routing';"
    );
  });

  it("gets allowed manifest.json with 200, Content-Type, Cache-Control, and ETag", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/c1-real-v1/manifest.json", {
      method: "GET",
    });
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("application/json; charset=utf-8");
    expect(res.headers.get("Cache-Control")).toBe(
      "public, max-age=300, stale-while-revalidate=60"
    );
    expect(res.headers.get("ETag")).toBeTruthy();

    const data: any = await res.json();
    expect(data.releaseId).toBe("c1-real-v1");
  });

  it("gets graph.json and wasm with correct Content-Type and immutable Cache-Control", async () => {
    const ctx = createExecutionContext();

    // graph.json
    const graphReq = new Request("http://localhost/releases/c1-real-v1/graph.json");
    const graphRes = await worker.fetch(graphReq, env, ctx);
    expect(graphRes.status).toBe(200);
    expect(graphRes.headers.get("Content-Type")).toBe("application/json; charset=utf-8");
    expect(graphRes.headers.get("Cache-Control")).toBe(
      "public, max-age=31536000, immutable"
    );

    // wasm
    const wasmReq = new Request(
      "http://localhost/releases/c1-real-v1/shutoko_routing_bg.wasm"
    );
    const wasmRes = await worker.fetch(wasmReq, env, ctx);
    expect(wasmRes.status).toBe(200);
    expect(wasmRes.headers.get("Content-Type")).toBe("application/wasm");
    expect(wasmRes.headers.get("Cache-Control")).toBe(
      "public, max-age=31536000, immutable"
    );

    // js
    const jsReq = new Request("http://localhost/releases/c1-real-v1/shutoko_routing.js");
    const jsRes = await worker.fetch(jsReq, env, ctx);
    expect(jsRes.status).toBe(200);
    expect(jsRes.headers.get("Content-Type")).toBe("text/javascript; charset=utf-8");
    expect(jsRes.headers.get("Cache-Control")).toBe(
      "public, max-age=31536000, immutable"
    );

    // d.ts
    const dtsReq = new Request("http://localhost/releases/c1-real-v1/shutoko_routing.d.ts");
    const dtsRes = await worker.fetch(dtsReq, env, ctx);
    expect(dtsRes.status).toBe(200);
    expect(dtsRes.headers.get("Content-Type")).toBe("text/plain; charset=utf-8");
    expect(dtsRes.headers.get("Cache-Control")).toBe(
      "public, max-age=31536000, immutable"
    );
    await waitOnExecutionContext(ctx);
  });

  it("gets engine.json with 200, JSON Content-Type, and manifest-equivalent Cache-Control", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/c1-real-v1/engine.json", {
      method: "GET",
    });
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("application/json; charset=utf-8");
    expect(res.headers.get("Cache-Control")).toBe(
      "public, max-age=300, stale-while-revalidate=60"
    );
    expect(res.headers.get("ETag")).toBeTruthy();

    const data: any = await res.json();
    expect(data.schemaVersion).toBe(1);
    expect(data.releaseId).toBe("c1-real-v1");
  });

  it("returns 404 for engine.json when the release has no engine.json in R2", async () => {
    // c1-real-v3 は manifest だけを置き engine.json を置かない（manifest 実在ゲートを
    // 通過したうえで、engine.json 自体の不在が 404 になることを確認する）。
    const customEnv = {
      ...env,
      ALLOWED_RELEASES: "c1-real-v1,c1-real-v3",
    };
    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v3/manifest.json",
      JSON.stringify({ schemaVersion: 1, releaseId: "c1-real-v3" })
    );

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/c1-real-v3/engine.json");
    const res = await worker.fetch(req, customEnv, ctx);
    expect(res.status).toBe(404);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error.code).toBe("NOT_FOUND");
    await waitOnExecutionContext(ctx);
  });

  it("returns 304 when If-None-Match matches ETag", async () => {
    const ctx = createExecutionContext();
    const req1 = new Request("http://localhost/releases/c1-real-v1/manifest.json");
    const res1 = await worker.fetch(req1, env, ctx);
    const etag = res1.headers.get("ETag")!;
    expect(etag).toBeTruthy();

    const req2 = new Request("http://localhost/releases/c1-real-v1/manifest.json", {
      headers: { "If-None-Match": etag },
    });
    const res2 = await worker.fetch(req2, env, ctx);
    expect(res2.status).toBe(304);
    expect(res2.headers.get("ETag")).toBe(etag);
    const body = await res2.text();
    expect(body).toBe("");
    await waitOnExecutionContext(ctx);
  });

  it("returns 404 for unknown releaseId", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/unknown-release/manifest.json");
    const res = await worker.fetch(req, env, ctx);
    expect(res.status).toBe(404);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "NOT_FOUND", retryable: false });
    await waitOnExecutionContext(ctx);
  });

  it("returns 404 for non-allowlisted artifact", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/c1-real-v1/forbidden.txt");
    const res = await worker.fetch(req, env, ctx);
    expect(res.status).toBe(404);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error.code).toBe("NOT_FOUND");
    await waitOnExecutionContext(ctx);
  });

  it("returns 404 for path traversal attempts", async () => {
    const ctx = createExecutionContext();
    const badPaths = [
      "http://localhost/releases/c1-real-v1/../manifest.json",
      "http://localhost/releases/c1-real-v1/%2e%2e/manifest.json",
      "http://localhost/releases/c1-real-v1/sub/manifest.json",
    ];
    for (const p of badPaths) {
      const req = new Request(p);
      const res = await worker.fetch(req, env, ctx);
      expect(res.status).toBe(404);
      expect(res.headers.get("Cache-Control")).toBe("no-store");
    }
    await waitOnExecutionContext(ctx);
  });

  it("returns 404 when manifest is missing from R2 even if releaseId is allowed", async () => {
    // c1-real-v2 is in env allowed releases, but no manifest exists in R2
    const customEnv = {
      ...env,
      ALLOWED_RELEASES: "c1-real-v1,c1-real-v2",
    };
    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v2/graph.json",
      JSON.stringify({ nodes: [] })
    );

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/c1-real-v2/graph.json");
    const res = await worker.fetch(req, customEnv, ctx);
    expect(res.status).toBe(404);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error.code).toBe("NOT_FOUND");
    await waitOnExecutionContext(ctx);
  });

  it("returns 405 for POST /releases/...", async () => {
    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/c1-real-v1/manifest.json", {
      method: "POST",
    });
    const res = await worker.fetch(req, env, ctx);
    expect(res.status).toBe(405);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const data: any = await res.json();
    expect(data.error).toEqual({ code: "METHOD_NOT_ALLOWED", retryable: false });
    await waitOnExecutionContext(ctx);
  });

  it("returns same headers with empty body for HEAD request", async () => {
    const ctx = createExecutionContext();
    const getReq = new Request("http://localhost/releases/c1-real-v1/manifest.json", {
      method: "GET",
    });
    const getRes = await worker.fetch(getReq, env, ctx);

    const headReq = new Request("http://localhost/releases/c1-real-v1/manifest.json", {
      method: "HEAD",
    });
    const headRes = await worker.fetch(headReq, env, ctx);

    expect(headRes.status).toBe(200);
    expect(headRes.headers.get("Content-Type")).toBe(getRes.headers.get("Content-Type"));
    expect(headRes.headers.get("Cache-Control")).toBe(getRes.headers.get("Cache-Control"));
    expect(headRes.headers.get("ETag")).toBe(getRes.headers.get("ETag"));
    const body = await headRes.text();
    expect(body).toBe("");
    await waitOnExecutionContext(ctx);
  });

  it("streams real graph.json (approx 2.9MB) without buffering body in handler", async () => {
    // Seed real graph.json
    await env.ARTIFACTS_BUCKET.put(
      "releases/c1-real-v1/graph.json",
      realGraphJsonText,
      {
        httpMetadata: {
          contentType: "application/json; charset=utf-8",
        },
      }
    );

    const ctx = createExecutionContext();
    const req = new Request("http://localhost/releases/c1-real-v1/graph.json");
    const res = await worker.fetch(req, env, ctx);
    await waitOnExecutionContext(ctx);

    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("application/json; charset=utf-8");
    // Verify that response body is a ReadableStream and has NOT been read by worker fetch/handler
    expect(res.body).toBeInstanceOf(ReadableStream);
    expect(res.bodyUsed).toBe(false);

    // Consume body and check byte equality
    const responseText = await res.text();
    expect(responseText.length).toBe(realGraphJsonText.length);
    expect(responseText).toBe(realGraphJsonText);
  });
});
