import { createErrorResponse } from "./errors";
import { handleGeocode } from "./geocode";
import { handleReleases } from "./releases";
import { Env, StructuredLog } from "./types";

export default {
  async fetch(
    request: Request,
    env: Env,
    _ctx: ExecutionContext
  ): Promise<Response> {
    const start = Date.now();
    const url = new URL(request.url);
    const pathname = url.pathname;

    let response: Response;
    let event = "unknown_route";
    let releaseId: string | undefined;
    let artifact: string | undefined;

    if (pathname.startsWith("/releases/")) {
      event = "releases_request";
      const parts = pathname.split("/").filter(Boolean);
      if (parts.length >= 3) {
        releaseId = parts[1];
        artifact = parts[2];
      }
      response = await handleReleases(request, env, pathname);
    } else if (pathname === "/api/geocode") {
      event = "geocode_request";
      response = await handleGeocode(request, env);
    } else {
      response = createErrorResponse("NOT_FOUND", 404, false);
    }

    let errorCode: string | undefined;
    let candidateCount: number | undefined;

    if (response.headers.get("content-type")?.includes("application/json")) {
      try {
        const cloned = response.clone();
        const data: any = await cloned.json();
        if (data && typeof data === "object") {
          if (data.error && typeof data.error.code === "string") {
            errorCode = data.error.code;
          }
          if (Array.isArray(data.candidates)) {
            candidateCount = data.candidates.length;
          }
        }
      } catch {
        // ignore parse error for logging
      }
    }

    const durationMs = Date.now() - start;
    const log: StructuredLog = {
      event,
      status: response.status,
      durationMs,
      ...(releaseId ? { releaseId } : {}),
      ...(artifact ? { artifact } : {}),
      ...(candidateCount !== undefined ? { candidateCount } : {}),
      ...(errorCode ? { errorCode } : {}),
    };

    console.log(JSON.stringify(log));

    return response;
  },
};
