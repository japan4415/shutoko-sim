import { createErrorResponse } from "./errors";
import { GeocoderAdapter } from "./geocoder/adapter";
import { GsiGeocoderAdapter } from "./geocoder/gsi";
import { checkRateLimit } from "./ratelimit";
import { Env, GeocodeRequest, GeocodeResponse, HandlerResult } from "./types";

const MAX_PAYLOAD_BYTES = 4096;

export async function handleGeocode(
  request: Request,
  env: Env,
  adapter?: GeocoderAdapter
): Promise<HandlerResult> {
  if (request.method !== "POST") {
    return {
      response: createErrorResponse("METHOD_NOT_ALLOWED", 405, false),
      logMeta: { errorCode: "METHOD_NOT_ALLOWED" },
    };
  }

  const contentLength = request.headers.get("content-length");
  if (contentLength && parseInt(contentLength, 10) > MAX_PAYLOAD_BYTES) {
    return {
      response: createErrorResponse("PAYLOAD_TOO_LARGE", 400, false),
      logMeta: { errorCode: "PAYLOAD_TOO_LARGE" },
    };
  }

  let arrayBuffer: ArrayBuffer;
  try {
    arrayBuffer = await request.arrayBuffer();
  } catch {
    return {
      response: createErrorResponse("INVALID_QUERY", 400, false),
      logMeta: { errorCode: "INVALID_QUERY" },
    };
  }

  if (arrayBuffer.byteLength > MAX_PAYLOAD_BYTES) {
    return {
      response: createErrorResponse("PAYLOAD_TOO_LARGE", 400, false),
      logMeta: { errorCode: "PAYLOAD_TOO_LARGE" },
    };
  }

  const text = new TextDecoder().decode(arrayBuffer);
  let body: GeocodeRequest;
  try {
    body = JSON.parse(text);
  } catch {
    return {
      response: createErrorResponse("INVALID_QUERY", 400, false),
      logMeta: { errorCode: "INVALID_QUERY" },
    };
  }

  if (!body || typeof body !== "object" || typeof body.query !== "string") {
    return {
      response: createErrorResponse("INVALID_QUERY", 400, false),
      logMeta: { errorCode: "INVALID_QUERY" },
    };
  }

  const trimmedQuery = body.query.trim();
  if (trimmedQuery.length === 0 || trimmedQuery.length > 200) {
    return {
      response: createErrorResponse("INVALID_QUERY", 400, false),
      logMeta: { errorCode: "INVALID_QUERY" },
    };
  }

  const rateLimitResult = await checkRateLimit(env, request);
  if (rateLimitResult.status === "unavailable") {
    return {
      response: createErrorResponse("RATE_LIMITER_UNAVAILABLE", 503, true),
      logMeta: { errorCode: "RATE_LIMITER_UNAVAILABLE" },
    };
  }
  if (rateLimitResult.status === "limited") {
    return {
      response: createErrorResponse("RATE_LIMITED", 429, true, {
        "Retry-After": String(rateLimitResult.retryAfter),
      }),
      logMeta: { errorCode: "RATE_LIMITED" },
    };
  }

  const activeAdapter = adapter ?? new GsiGeocoderAdapter();
  const controller = new AbortController();
  const timeoutId = setTimeout(() => {
    controller.abort();
  }, 5000);

  try {
    const candidates = await activeAdapter.geocode(trimmedQuery, controller.signal);
    const responseBody: GeocodeResponse = { candidates };

    return {
      response: new Response(JSON.stringify(responseBody), {
        status: 200,
        headers: {
          "Content-Type": "application/json; charset=utf-8",
          "Cache-Control": "no-store",
        },
      }),
      logMeta: { candidateCount: candidates.length },
    };
  } catch (err: unknown) {
    if (
      (err instanceof Error && err.name === "AbortError") ||
      controller.signal.aborted
    ) {
      return {
        response: createErrorResponse("GEOCODER_TIMEOUT", 504, true),
        logMeta: { errorCode: "GEOCODER_TIMEOUT" },
      };
    }
    return {
      response: createErrorResponse("GEOCODER_UNAVAILABLE", 502, true),
      logMeta: { errorCode: "GEOCODER_UNAVAILABLE" },
    };
  } finally {
    clearTimeout(timeoutId);
  }
}
