import { createErrorResponse } from "./errors";
import { handleBenchResult } from "./bench";
import { handleGeocode } from "./geocode";
import { handleReleases } from "./releases";
import { Env, HandlerResult, StructuredLog } from "./types";

function getRawPath(urlStr: string): string {
  const schemeHostMatch = urlStr.match(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^\/?#]+/);
  let pathPart = schemeHostMatch ? urlStr.slice(schemeHostMatch[0].length) : urlStr;
  const qIndex = pathPart.indexOf("?");
  if (qIndex !== -1) {
    pathPart = pathPart.slice(0, qIndex);
  }
  const hIndex = pathPart.indexOf("#");
  if (hIndex !== -1) {
    pathPart = pathPart.slice(0, hIndex);
  }
  return pathPart;
}

function validatePath(rawPath: string, pathname: string): boolean {
  if (rawPath.includes("\\") || pathname.includes("\\")) {
    return false;
  }

  const rawLower = rawPath.toLowerCase();
  const pathLower = pathname.toLowerCase();
  if (
    rawLower.includes("%2f") ||
    pathLower.includes("%2f") ||
    rawLower.includes("%5c") ||
    pathLower.includes("%5c") ||
    rawLower.includes("%2e") ||
    pathLower.includes("%2e")
  ) {
    return false;
  }

  let decoded: string;
  try {
    decoded = decodeURIComponent(rawPath);
  } catch {
    return false;
  }

  const decodedLower = decoded.toLowerCase();
  if (
    decoded.includes("\\") ||
    decodedLower.includes("%2f") ||
    decodedLower.includes("%5c") ||
    decodedLower.includes("%2e") ||
    decoded.includes("..")
  ) {
    return false;
  }

  if (!rawPath.startsWith("/") || !pathname.startsWith("/")) {
    return false;
  }

  const rawSegments = rawPath.slice(1).split("/");
  const pathSegments = pathname.slice(1).split("/");

  if (rawSegments.some((s) => s === "") || pathSegments.some((s) => s === "")) {
    return false;
  }

  return true;
}

export default {
  async fetch(
    request: Request,
    env: Env,
    _ctx: ExecutionContext
  ): Promise<Response> {
    const start = Date.now();
    const rawPath = getRawPath(request.url);
    const url = new URL(request.url);
    const pathname = url.pathname;

    let event = "unknown_route";
    const rawSegments = rawPath.startsWith("/") ? rawPath.slice(1).split("/") : [];
    if (rawSegments[0] === "releases" || rawPath.startsWith("/releases")) {
      event = "releases_request";
    } else if (rawSegments[0] === "api" || rawPath.startsWith("/api")) {
      event =
        rawSegments.length === 2 && rawSegments[1] === "bench-result"
          ? "bench_result_request"
          : "geocode_request";
    }

    let result: HandlerResult;

    if (!validatePath(rawPath, pathname)) {
      result = {
        response: createErrorResponse("NOT_FOUND", 404, false),
        logMeta: { errorCode: "NOT_FOUND" },
      };
    } else if (rawSegments[0] === "releases") {
      if (rawSegments.length !== 3) {
        result = {
          response: createErrorResponse("NOT_FOUND", 404, false),
          logMeta: { errorCode: "NOT_FOUND" },
        };
      } else {
        const [, releaseId, artifact] = rawSegments;
        result = await handleReleases(request, env, releaseId, artifact);
      }
    } else if (rawSegments[0] === "api" && rawSegments.length === 2 && rawSegments[1] === "geocode") {
      result = await handleGeocode(request, env);
    } else if (rawSegments[0] === "api" && rawSegments.length === 2 && rawSegments[1] === "bench-result") {
      result = await handleBenchResult(request, env);
    } else {
      event = "unknown_route";
      result = {
        response: createErrorResponse("NOT_FOUND", 404, false),
        logMeta: { errorCode: "NOT_FOUND" },
      };
    }

    const durationMs = Date.now() - start;
    const log: StructuredLog = {
      event,
      status: result.response.status,
      durationMs,
      ...(result.logMeta?.releaseId ? { releaseId: result.logMeta.releaseId } : {}),
      ...(result.logMeta?.artifact ? { artifact: result.logMeta.artifact } : {}),
      ...(result.logMeta?.candidateCount !== undefined ? { candidateCount: result.logMeta.candidateCount } : {}),
      ...(result.logMeta?.benchmarkKey ? { benchmarkKey: result.logMeta.benchmarkKey } : {}),
      ...(result.logMeta?.benchmarkBytes !== undefined ? { benchmarkBytes: result.logMeta.benchmarkBytes } : {}),
      ...(result.logMeta?.errorCode ? { errorCode: result.logMeta.errorCode } : {}),
    };

    console.log(JSON.stringify(log));

    return result.response;
  },
};
