import { ErrorResponse } from "./types";

export type ErrorCode =
  | "NOT_FOUND"
  | "METHOD_NOT_ALLOWED"
  | "PAYLOAD_TOO_LARGE"
  | "INVALID_QUERY"
  | "RATE_LIMITED"
  | "RATE_LIMITER_UNAVAILABLE"
  | "GEOCODER_UNAVAILABLE"
  | "GEOCODER_TIMEOUT";

export function createErrorResponse(
  code: ErrorCode,
  status: number,
  retryable: boolean,
  extraHeaders?: Record<string, string>
): Response {
  const body: ErrorResponse = {
    error: {
      code,
      retryable,
    },
  };

  const headers = new Headers({
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
    ...extraHeaders,
  });

  return new Response(JSON.stringify(body), {
    status,
    headers,
  });
}
