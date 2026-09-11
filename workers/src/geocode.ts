import { createErrorResponse } from "./errors";
import { GeocoderAdapter } from "./geocoder/adapter";
import { GsiGeocoderAdapter } from "./geocoder/gsi";
import { checkRateLimit } from "./ratelimit";
import { Env, GeocodeRequest, GeocodeResponse } from "./types";

const MAX_PAYLOAD_BYTES = 4096;

export async function handleGeocode(
  request: Request,
  env: Env,
  adapter?: GeocoderAdapter
): Promise<Response> {
  if (request.method !== "POST") {
    return createErrorResponse("METHOD_NOT_ALLOWED", 405, false);
  }

  const contentLength = request.headers.get("content-length");
  if (contentLength && parseInt(contentLength, 10) > MAX_PAYLOAD_BYTES) {
    return createErrorResponse("PAYLOAD_TOO_LARGE", 400, false);
  }

  let arrayBuffer: ArrayBuffer;
  try {
    arrayBuffer = await request.arrayBuffer();
  } catch {
    return createErrorResponse("INVALID_QUERY", 400, false);
  }

  if (arrayBuffer.byteLength > MAX_PAYLOAD_BYTES) {
    return createErrorResponse("PAYLOAD_TOO_LARGE", 400, false);
  }

  const text = new TextDecoder().decode(arrayBuffer);
  let body: GeocodeRequest;
  try {
    body = JSON.parse(text);
  } catch {
    return createErrorResponse("INVALID_QUERY", 400, false);
  }

  if (!body || typeof body !== "object" || typeof body.query !== "string") {
    return createErrorResponse("INVALID_QUERY", 400, false);
  }

  const trimmedQuery = body.query.trim();
  if (trimmedQuery.length === 0 || trimmedQuery.length > 200) {
    return createErrorResponse("INVALID_QUERY", 400, false);
  }

  const rateLimitResult = await checkRateLimit(env, request);
  if (!rateLimitResult.allowed) {
    return createErrorResponse("RATE_LIMITED", 429, true, {
      "Retry-After": "60",
    });
  }

  const activeAdapter = adapter ?? new GsiGeocoderAdapter();
  const controller = new AbortController();
  const timeoutId = setTimeout(() => {
    controller.abort();
  }, 5000);

  try {
    const candidates = await activeAdapter.geocode(trimmedQuery, controller.signal);
    const responseBody: GeocodeResponse = { candidates };

    return new Response(JSON.stringify(responseBody), {
      status: 200,
      headers: {
        "Content-Type": "application/json; charset=utf-8",
        "Cache-Control": "no-store",
      },
    });
  } catch (err: unknown) {
    if (
      (err instanceof Error && err.name === "AbortError") ||
      controller.signal.aborted
    ) {
      return createErrorResponse("GEOCODER_TIMEOUT", 504, true);
    }
    return createErrorResponse("GEOCODER_UNAVAILABLE", 502, true);
  } finally {
    clearTimeout(timeoutId);
  }
}
