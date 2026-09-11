import { createErrorResponse } from "./errors";
import { Env, HandlerResult } from "./types";

const ALLOWED_ARTIFACTS = new Set([
  "manifest.json",
  "graph.json",
  "snap-index.json",
  "shutoko_routing_bg.wasm",
  "shutoko_routing.js",
  "shutoko_routing.d.ts",
  "index.d.ts",
]);

const RELEASE_ID_REGEX = /^[a-z0-9][a-z0-9.-]{0,63}$/;

function getContentType(artifact: string): string {
  if (artifact.endsWith(".wasm")) {
    return "application/wasm";
  }
  if (artifact.endsWith(".json")) {
    return "application/json; charset=utf-8";
  }
  if (artifact.endsWith(".d.ts")) {
    return "text/plain; charset=utf-8";
  }
  if (artifact.endsWith(".js")) {
    return "text/javascript; charset=utf-8";
  }
  return "application/octet-stream";
}

function matchEtag(clientEtag: string, serverEtag: string): boolean {
  const cleanClient = clientEtag.trim().replace(/^W\//, "").replace(/^"|"$/g, "");
  const cleanServer = serverEtag.trim().replace(/^W\//, "").replace(/^"|"$/g, "");
  return cleanClient === cleanServer || clientEtag.trim() === "*";
}

export async function handleReleases(
  request: Request,
  env: Env,
  releaseId: string,
  artifact: string
): Promise<HandlerResult> {
  const logMeta = { releaseId, artifact };

  if (request.method !== "GET" && request.method !== "HEAD") {
    return {
      response: createErrorResponse("METHOD_NOT_ALLOWED", 405, false),
      logMeta: { ...logMeta, errorCode: "METHOD_NOT_ALLOWED" },
    };
  }

  if (!RELEASE_ID_REGEX.test(releaseId)) {
    return {
      response: createErrorResponse("NOT_FOUND", 404, false),
      logMeta: { ...logMeta, errorCode: "NOT_FOUND" },
    };
  }

  const allowedReleases = (env.ALLOWED_RELEASES || "")
    .split(",")
    .map((r) => r.trim())
    .filter(Boolean);

  if (!allowedReleases.includes(releaseId)) {
    return {
      response: createErrorResponse("NOT_FOUND", 404, false),
      logMeta: { ...logMeta, errorCode: "NOT_FOUND" },
    };
  }

  if (!ALLOWED_ARTIFACTS.has(artifact)) {
    return {
      response: createErrorResponse("NOT_FOUND", 404, false),
      logMeta: { ...logMeta, errorCode: "NOT_FOUND" },
    };
  }

  // Check that manifest.json exists in R2 for this release
  if (artifact !== "manifest.json") {
    const manifestKey = `releases/${releaseId}/manifest.json`;
    const manifestObj = await env.ARTIFACTS_BUCKET.head(manifestKey);
    if (!manifestObj) {
      return {
        response: createErrorResponse("NOT_FOUND", 404, false),
        logMeta: { ...logMeta, errorCode: "NOT_FOUND" },
      };
    }
  }

  const targetKey = `releases/${releaseId}/${artifact}`;
  const isHead = request.method === "HEAD";
  const obj = isHead
    ? await env.ARTIFACTS_BUCKET.head(targetKey)
    : await env.ARTIFACTS_BUCKET.get(targetKey);

  if (!obj) {
    return {
      response: createErrorResponse("NOT_FOUND", 404, false),
      logMeta: { ...logMeta, errorCode: "NOT_FOUND" },
    };
  }

  const contentType = getContentType(artifact);
  const cacheControl =
    artifact === "manifest.json"
      ? "public, max-age=300, stale-while-revalidate=60"
      : "public, max-age=31536000, immutable";

  const etag = obj.httpEtag;
  const headers = new Headers({
    "Content-Type": contentType,
    "Cache-Control": cacheControl,
    ETag: etag,
  });

  const ifNoneMatch = request.headers.get("if-none-match");
  if (ifNoneMatch && matchEtag(ifNoneMatch, etag)) {
    return {
      response: new Response(null, {
        status: 304,
        headers,
      }),
      logMeta,
    };
  }

  if (isHead) {
    return {
      response: new Response(null, {
        status: 200,
        headers,
      }),
      logMeta,
    };
  }

  return {
    response: new Response((obj as R2ObjectBody).body, {
      status: 200,
      headers,
    }),
    logMeta,
  };
}
